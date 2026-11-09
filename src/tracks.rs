//! Audio tracks of a video (YouTube dubs and other multi-language uploads).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AudioTrack {
    /// yt-dlp language code ("es-US"), used to pick the stream.
    pub lang: String,
    /// "Spanish (US)".
    pub label: String,
    /// The language spoken in the upload (not a dub).
    pub original: bool,
}

#[cfg_attr(not(desktop), allow(dead_code))]
#[derive(Default, Deserialize)]
#[serde(default)]
struct Probe {
    formats: Vec<Format>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Format {
    language: Option<String>,
    format_note: Option<String>,
    vcodec: Option<String>,
    acodec: Option<String>,
}

/// The distinct audio languages of `url`; empty when there is only one.
#[cfg(desktop)]
pub async fn list(app: &tauri::AppHandle, url: &str) -> Result<Vec<AudioTrack>, String> {
    let mut args: Vec<String> = ["--no-playlist", "--skip-download"]
        .map(String::from)
        .to_vec();
    args.extend(crate::downloads::youtube_args(url));
    let probe: Probe = crate::extras::dump_json(app, args, url).await?;
    Ok(tracks(&probe.formats))
}

#[cfg_attr(not(desktop), allow(dead_code))]
fn tracks(formats: &[Format]) -> Vec<AudioTrack> {
    let mut out: Vec<AudioTrack> = Vec::new();
    for f in formats {
        let audio_only = f.vcodec.as_deref() == Some("none")
            && f.acodec.as_deref().is_some_and(|a| a != "none");
        let Some(lang) = f.language.as_deref().filter(|l| valid_lang(l)) else {
            continue;
        };
        if !audio_only {
            continue;
        }
        let note = f.format_note.as_deref().unwrap_or("");
        let original = note.contains("original");
        let label = label(note).unwrap_or_else(|| lang.to_string());
        match out.iter_mut().find(|t| t.lang == lang) {
            Some(t) => {
                t.original |= original;
                if t.label == t.lang {
                    t.label = label;
                }
            }
            None => out.push(AudioTrack { lang: lang.into(), label, original }),
        }
    }
    if out.len() < 2 {
        return Vec::new();
    }
    // Original first, then alphabetical.
    out.sort_by(|a, b| b.original.cmp(&a.original).then(a.label.cmp(&b.label)));
    out
}

/// "Spanish (US) original (default), medium, DRC" -> "Spanish (US)".
#[cfg_attr(not(desktop), allow(dead_code))]
fn label(note: &str) -> Option<String> {
    let head = note.split(',').next()?.trim();
    let head = head
        .replace("original (default)", "")
        .replace("(default)", "")
        .replace("original", "");
    let head = head.trim();
    let generic = ["low", "medium", "high", "ultralow", ""];
    (!generic.contains(&head)).then(|| head.to_string())
}

/// Language codes go into a yt-dlp format filter, so keep them plain.
pub fn valid_lang(lang: &str) -> bool {
    !lang.is_empty() && lang.len() <= 15 && lang.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Prefers the audio in `lang`, falling back to the normal choice when the
/// site has no stream in that language: "bv*+ba/b" ->
/// "bv*+ba[language=es]/bv*+ba/b".
pub fn with_language(format: &str, lang: &str) -> String {
    let filter = format!("[language={lang}]");
    let preferred: Vec<String> = format
        .split('/')
        .filter_map(|part| {
            if let Some(i) = part.find("+ba") {
                let at = i + 3;
                Some(format!("{}{filter}{}", &part[..at], &part[at..]))
            } else if part.starts_with("ba") {
                Some(format!("ba{filter}{}", &part[2..]))
            } else {
                None
            }
        })
        .collect();
    if preferred.is_empty() {
        return format.to_string();
    }
    format!("{}/{format}", preferred.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(lang: &str, note: &str) -> Format {
        Format {
            language: Some(lang.into()),
            format_note: Some(note.into()),
            vcodec: Some("none".into()),
            acodec: Some("mp4a.40.2".into()),
        }
    }

    #[test]
    fn lists_each_language_once_with_the_original_first() {
        let formats = vec![
            audio("en-US", "English (US), medium"),
            audio("es-US", "Spanish (US) original (default), low"),
            audio("es-US", "Spanish (US) original (default), medium, DRC"),
            audio("pt-BR", "Portuguese (BR), medium"),
            Format { vcodec: Some("avc1".into()), ..audio("fr", "1080p") },
        ];
        let t = tracks(&formats);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0], AudioTrack { lang: "es-US".into(), label: "Spanish (US)".into(), original: true });
        assert_eq!(t[1].label, "English (US)");
        assert!(!t[1].original);
    }

    #[test]
    fn a_single_language_shows_no_choice() {
        assert!(tracks(&[audio("es", "medium"), audio("es", "low")]).is_empty());
    }

    #[test]
    fn the_chosen_language_is_tried_before_the_usual_format() {
        assert_eq!(with_language("bv*+ba/b", "en-US"), "bv*+ba[language=en-US]/bv*+ba/b");
        assert_eq!(with_language("ba/b", "es"), "ba[language=es]/ba/b");
        assert_eq!(
            with_language("bv*[height<=720]+ba/b[height<=720]", "es"),
            "bv*[height<=720]+ba[language=es]/bv*[height<=720]+ba/b[height<=720]"
        );
        assert_eq!(with_language("ba[ext=m4a]/ba/b", "es"), "ba[language=es][ext=m4a]/ba[language=es]/ba[ext=m4a]/ba/b");
    }

    #[test]
    fn rejects_codes_that_could_break_the_filter() {
        assert!(valid_lang("es-419"));
        assert!(!valid_lang("es]/b"));
        assert!(!valid_lang(""));
    }
}
