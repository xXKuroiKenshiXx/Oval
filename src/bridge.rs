//! Local server for the browser extension (desktop only). It listens on
//! 127.0.0.1 and only answers browser extensions: web pages can't reach it,
//! because their requests carry a web origin or need a CORS preflight that is
//! refused. Every request and answer is JSON.
//!
//! | Request                | Body                                              |
//! | ---------------------- | ------------------------------------------------- |
//! | `GET  /v1/hello`       |                                                   |
//! | `POST /v1/download`    | url, title?, mode?, quality?, section?, precise?, folder?, live? |
//! | `POST /v1/live`        | url: is it a live stream, since when, its recording |
//! | `GET  /v1/jobs?ids=1,2`|                                                   |
//! | `POST /v1/cancel`      | id                                                |
//! | `POST /v1/stop`        | id: end a live recording and keep it              |
//! | `POST /v1/transcript`  | url, format, lang?, section?, note?, folder?      |
//! | `POST /v1/thumbnail`   | url, folder?                                      |
//! | `POST /v1/pick-folder` | folder?                                           |
//! | `POST /v1/reveal`      | path?                                             |

use std::io::Read;
use std::path::PathBuf;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::downloads::{self, Mode, NewJob, Queue, Section};
use crate::extras::{self, str_field};
use crate::transcript::Format;

/// Fixed so the extension knows where to look (`extension/src/background.js`
/// tries them in this order). Some systems reserve ports, hence the spares.
pub const PORTS: [u16; 3] = [17177, 27177, 37177];

static PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

/// Starts the server in the background on the first free port. If none is
/// free (other Ovals already serve the extension) this one goes without.
pub fn start(app: AppHandle) {
    let Some((server, port)) = PORTS
        .iter()
        .find_map(|&port| Server::http(("127.0.0.1", port)).ok().map(|s| (s, port)))
    else {
        eprintln!("Oval: no free port for the browser extension");
        return;
    };
    let _ = PORT.set(port);
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let app = app.clone();
            std::thread::spawn(move || handle(&app, request));
        }
    });
}

fn is_extension(origin: &str) -> bool {
    [
        "chrome-extension://",
        "moz-extension://",
        "safari-web-extension://",
    ]
    .iter()
    .any(|scheme| origin.starts_with(scheme))
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// Requests a web page could send don't get through: a web origin is refused
/// and so is anything aimed at another host name (DNS rebinding).
fn allowed(request: &Request) -> bool {
    let port = PORT.get().copied().unwrap_or_default();
    let host_ok = header(request, "Host")
        .is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"));
    let origin_ok = header(request, "Origin").map_or(true, is_extension);
    let client_ok = header(request, "X-Oval-Client").is_some();
    host_ok && origin_ok && client_ok
}

fn respond(request: Request, status: u16, body: Value) {
    let origin = header(&request, "Origin")
        .filter(|o| is_extension(o))
        .map(str::to_string);
    let mut response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(
            Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap(),
        );
    if let Some(origin) = origin {
        for (k, v) in [
            ("Access-Control-Allow-Origin", origin.as_str()),
            ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
            (
                "Access-Control-Allow-Headers",
                "Content-Type, X-Oval-Client",
            ),
            ("Access-Control-Allow-Private-Network", "true"),
            ("Vary", "Origin"),
        ] {
            response.add_header(Header::from_bytes(k, v).unwrap());
        }
    }
    let _ = request.respond(response);
}

fn handle(app: &AppHandle, mut request: Request) {
    if *request.method() == Method::Options {
        let ok = header(&request, "Origin").is_some_and(is_extension);
        return respond(request, if ok { 204 } else { 403 }, Value::Null);
    }
    if !allowed(&request) {
        return respond(
            request,
            403,
            json!({ "error": "Solo para la extensión de Oval" }),
        );
    }
    let mut body = String::new();
    let _ = request.as_reader().take(1 << 20).read_to_string(&mut body);
    let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let result =
        tauri::async_runtime::block_on(route(app, request.method().clone(), path, query, &body));
    match result {
        Ok(value) => respond(request, 200, value),
        Err((status, error)) => respond(request, status, json!({ "error": error })),
    }
}

type Answer = Result<Value, (u16, String)>;

fn bad(msg: impl Into<String>) -> (u16, String) {
    (400, msg.into())
}

async fn route(app: &AppHandle, method: Method, path: &str, query: &str, body: &Value) -> Answer {
    match (method, path) {
        (Method::Get, "/v1/hello") => Ok(json!({
            "app": "oval",
            "version": env!("CARGO_PKG_VERSION"),
            "folder": downloads::download_root(app),
        })),
        (Method::Post, "/v1/download") => download(app, body),
        (Method::Post, "/v1/live") => {
            let url = str_field(body, "url").ok_or_else(|| bad("Falta el enlace"))?;
            let info = crate::live::info(app, url).await.map_err(|e| (502, e))?;
            Ok(json!(info))
        }
        (Method::Get, "/v1/jobs") => {
            let ids: Vec<u64> = query
                .strip_prefix("ids=")
                .unwrap_or("")
                .split(',')
                .filter_map(|id| id.parse().ok())
                .collect();
            let jobs: Vec<_> = app
                .state::<Queue>()
                .list()
                .into_iter()
                .filter(|j| ids.contains(&j.id))
                .collect();
            Ok(json!(jobs))
        }
        (Method::Post, "/v1/cancel") => {
            let id = body
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| bad("Falta el id"))?;
            app.state::<Queue>().cancel(app, id);
            Ok(json!({ "ok": true }))
        }
        (Method::Post, "/v1/stop") => {
            let id = body
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| bad("Falta el id"))?;
            app.state::<Queue>().stop_live(app, id);
            Ok(json!({ "ok": true }))
        }
        (Method::Post, "/v1/transcript") => {
            let url = str_field(body, "url").ok_or_else(|| bad("Falta el enlace"))?;
            let format: Format = body
                .get("format")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| bad("Formato desconocido"))?
                .unwrap_or(Format::Md);
            let folder = folder(app, body)?;
            let req = extras::TranscriptRequest {
                url,
                format,
                lang: str_field(body, "lang"),
                section: section(body),
                note: str_field(body, "note").unwrap_or(""),
                folder: &folder,
            };
            let path = extras::save_transcript(app, req)
                .await
                .map_err(|e| (502, e))?;
            Ok(json!({ "path": path }))
        }
        (Method::Post, "/v1/thumbnail") => {
            let url = str_field(body, "url").ok_or_else(|| bad("Falta el enlace"))?;
            let folder = folder(app, body)?;
            let path = extras::save_thumbnail(app, url, &folder)
                .await
                .map_err(|e| (502, e))?;
            Ok(json!({ "path": path }))
        }
        (Method::Post, "/v1/pick-folder") => {
            let start = str_field(body, "folder").map(PathBuf::from);
            let picked = crate::pick_folder(app, "Dónde guardar lo de la extensión", start).await;
            Ok(json!({ "path": picked }))
        }
        (Method::Post, "/v1/reveal") => {
            let path = str_field(body, "path")
                .map(PathBuf::from)
                .unwrap_or_else(|| downloads::download_root(app));
            if !path.exists() {
                return Err((404, "Ya no existe".into()));
            }
            crate::platform::reveal(&path);
            Ok(json!({ "ok": true }))
        }
        _ => Err((404, "No existe".into())),
    }
}

fn section(body: &Value) -> Option<Section> {
    body.get("section")
        .cloned()
        .and_then(|v| serde_json::from_value::<Section>(v).ok())
        .filter(Section::is_valid)
}

/// The folder the extension chose, or Oval's own. It must be absolute.
fn folder(app: &AppHandle, body: &Value) -> Result<PathBuf, (u16, String)> {
    match str_field(body, "folder") {
        Some(f) => {
            let path = PathBuf::from(f);
            if path.is_absolute() {
                Ok(path)
            } else {
                Err(bad("La carpeta debe ser una ruta completa"))
            }
        }
        None => Ok(downloads::download_root(app)),
    }
}

fn download(app: &AppHandle, body: &Value) -> Answer {
    let url = str_field(body, "url").ok_or_else(|| bad("Falta el enlace"))?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(bad("El enlace no es válido"));
    }
    let mode = match str_field(body, "mode") {
        Some("audio") => Mode::Audio,
        _ => Mode::Video,
    };
    let custom = str_field(body, "folder").is_some();
    let item = NewJob {
        url: url.into(),
        title: str_field(body, "title").map(String::from),
        thumbnail: str_field(body, "thumbnail").map(String::from),
        section: section(body),
        precise: body
            .get("precise")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        dir: if custom {
            Some(folder(app, body)?)
        } else {
            None
        },
    };
    let quality = str_field(body, "quality");
    // Record a live stream from now on, like the app's Directo tab.
    let live = body.get("live").and_then(Value::as_bool).unwrap_or(false);
    let jobs = app
        .state::<Queue>()
        .add(app, vec![item], mode, quality, None, live);
    Ok(json!(jobs.first()))
}
