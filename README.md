<p align="center">
  <img src="icons/128x128.png" width="96" alt="Oval">
</p>

<h1 align="center">Oval</h1>

<p align="center">
  Un descargador de vídeos minimalista, redondo y rápido.<br>
  Pega un enlace, pulsa descargar. Nada más.<br>
  Vídeos sueltos, canales enteros y directos en tiempo real, en Windows, Linux y Android.
</p>

<p align="center">
  <a href="https://xxkuroikenshixx.github.io/Oval/"><b>Página web y demo</b></a> ·
  <a href="https://github.com/xXKuroiKenshiXx/Oval/releases/latest"><b>Descargar</b></a>
</p>

<p align="center">
  <a href="docs/demo.mp4"><img src="docs/demo.webp" width="800" alt="Oval descargando vídeos, un canal y un directo"></a>
</p>

<p align="center">
  <sub>Grabación real de la app · <a href="docs/demo.mp4">ver en MP4</a> ·
  contenido: películas abiertas de la <a href="https://studio.blender.org">Blender Foundation</a> (CC BY) y directo de la NASA</sub>
</p>

## Qué hace

Tres pestañas, una para cada caso:

- **Enlace**: un vídeo de YouTube, Kick (videos grabados y clips), X/Twitter, Instagram, TikTok, Facebook, Vimeo, Reddit o
  [cualquiera de los más de mil sitios compatibles](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md).
- **Canal**: una cuenta entera (canal de YouTube, perfil de TikTok, playlist…). Aparecen todas las
  miniaturas; toca las que **no** quieras y se marcan en rojo. `Shift` + clic marca un rango.
  Busca por título dentro del canal, y lo que ya tienes en la carpeta sale **en amarillo** (y queda
  fuera salvo que lo toques), aunque lo hayas bajado en otra sesión. Lo que quedó a medias porque
  se cortó o cerraste la app sale con borde punteado y sigue desde donde iba. En YouTube eliges
  entre Vídeos, Shorts y Directos; en Kick y Twitch, entre Vídeos grabados y Clips.
- **Directo**: graba en tiempo real un directo de Twitch, Kick, YouTube, TikTok y otros. Detén
  cuando quieras y queda guardado en MP4. Si se corta la conexión, lo grabado no se pierde.

Y además:

- **Sin preguntas**: no hay diálogo de "guardar como". Todo va a `Descargas/Oval`
  (cámbialo con el lápiz junto a la carpeta y se recuerda).
  Solo Canal crea subcarpetas: `Oval/Canal/elpepe.mp4`. Los enlaces individuales, audio
  y directos quedan directamente en la carpeta seleccionada.
- **Rápido**: fragmentos en paralelo, motor precargado al abrir y arranque casi instantáneo.
- **Tiempo estimado, velocidad y tamaño** de cada descarga en vivo.
- **Pausar, reanudar y cancelar**. Al reanudar continúa donde se quedó; al cancelar borra los
  archivos parciales.
- **Descargas simultáneas**: elige cuántas a la vez (1 a 8). Los directos no ocupan hueco.
- **Calidad**: MP4 hasta 4K o limitado a 1440p/1080p/720p/480p/360p, o MP3 a 320/192/128 kbps.
- **Ventana ovalada sin bordes** que puedes redimensionar desde el tirador de abajo a la derecha
  (doble clic en el nombre para volver al tamaño original). Fuera del óvalo los clics pasan a lo
  que haya detrás.
- **Windows, Linux y Android**.
- **Resistente a los bloqueos de YouTube**: si YouTube pide "confirmar que no eres un bot", Oval
  cambia solo a otros reproductores de YouTube durante un rato y reintenta, sin pedirte sesión.

Si pegas un canal o un directo en la pestaña Enlace, Oval lo lleva solo a la pestaña correcta.

## Descargar

Ve a [Releases](https://github.com/xXKuroiKenshiXx/Oval/releases):

| Sistema | Archivo |
| --- | --- |
| Windows 10/11 | `Oval_x.y.z_x64-setup.exe` (instalador) o `Oval-x.y.z-portable.exe` |
| Linux (cualquier distro) | `Oval_x.y.z_amd64.AppImage` o `Oval_x.y.z_x86_64.flatpak` |
| Debian, Ubuntu, Mint | `Oval_x.y.z_amd64.deb` |
| Fedora, openSUSE | `Oval-x.y.z-1.x86_64.rpm` |
| Arch, Manjaro, EndeavourOS | `oval-bin-x.y.z-1-x86_64.pkg.tar.zst` |
| Void Linux | `oval-x.y.z_1.x86_64.xbps` |
| Android 7+ | `Oval_x.y.z_android.apk` (permite instalar apps de fuera de Play Store) |

Cómo instalar cada paquete de Linux, desde la carpeta donde lo descargaste:

```bash
flatpak install --user Oval_*.flatpak          # Flatpak
sudo apt install ./Oval_*_amd64.deb            # Debian, Ubuntu, Mint
sudo dnf install ./Oval-*.x86_64.rpm           # Fedora (openSUSE: sudo zypper install)
sudo pacman -U oval-bin-*.pkg.tar.zst          # Arch
xbps-rindex -a oval-*.xbps && sudo xbps-install --repository="$PWD" oval   # Void
```

En Windows hace falta WebView2 (viene con Windows 11; el instalador lo añade si falta).
En Android las descargas van a `Descargas/Oval`; los directos se guardan en `.ts`, que abre
cualquier reproductor (VLC, MX Player…). Mientras descarga aparece la notificación «Descargando…»
y la descarga sigue a toda velocidad aunque apagues la pantalla.
En Linux, dale permiso de ejecución al AppImage (`chmod +x Oval_*.AppImage`). Para que el óvalo
se vea transparente hace falta un escritorio con compositor (GNOME, KDE y la mayoría lo tienen).

## Uso

| Acción | Cómo |
| --- | --- |
| Descargar | Pega el enlace (o arrástralo al óvalo) y pulsa <kbd>Enter</kbd> |
| Varios enlaces | Pégalos juntos separados por espacios o saltos de línea |
| Audio | Cambia a **MP3** antes de descargar |
| Calidad | Toca el selector **Máx** |
| Pausar / reanudar | Botón ⏸ / ▶ de cada descarga |
| Grabar un directo | Pestaña **Directo**, pega el enlace y pulsa el botón rojo |
| Terminar una grabación | Botón ■ (guarda) o × (descarta) |
| Abrir el archivo | Clic en una descarga terminada |
| Quitar de la lista | × en una descarga terminada (el archivo se queda) |
| Borrar el archivo | 🗑 dos veces en una descarga terminada (va a la papelera en Windows y Linux) |
| Cuenta completa | Pestaña **Canal**, pega el enlace y toca las miniaturas que no quieras |
| Cambiar carpeta | Lápiz junto a la carpeta, abajo |
| Cambiar tamaño | Tirador abajo a la derecha del óvalo |

Al abrir Oval, si tienes un enlace copiado aparece ya en la caja.

### Límites conocidos

Oval puede bajar lo que puede bajar yt-dlp. Algunas cosas dependen de cada sitio:

- **YouTube** limita por conexión: si descargas muchísimo seguido puede frenarte. Oval reintenta
  con otros reproductores (los vídeos que no se pueden insertar en otras webs pueden quedar en
  360p durante ese rato) y espera un poco entre peticiones. No hay forma de garantizar descargas
  ilimitadas: el límite lo pone YouTube a tu IP.

- Perfiles completos de **X/Twitter** e **Instagram** suelen pedir iniciar sesión, así que la
  pestaña Canal funciona mejor con YouTube, TikTok y playlists.
- Los directos de **Instagram** no siempre están disponibles en yt-dlp.

## Cómo funciona

Oval está hecho en Rust con [Tauri 2](https://tauri.app). Las descargas las hace
[yt-dlp](https://github.com/yt-dlp/yt-dlp) y la unión de audio y vídeo
[FFmpeg](https://ffmpeg.org).

En el primer arranque Oval baja ambos a su carpeta de datos (`%LOCALAPPDATA%\Oval\bin` en
Windows, `~/.local/share/Oval/bin` en Linux). Si ya tienes FFmpeg instalado lo usa. yt-dlp se
actualiza solo una vez al día, porque los sitios cambian a menudo y un motor viejo es la causa más
común de fallos. Ninguno de los dos va incluido en el instalador. Si el equipo no tiene Deno ni
Node, Oval también baja [Deno](https://deno.com): yt-dlp lo necesita para resolver los desafíos de
JavaScript de YouTube.

En Android, yt-dlp, Python y FFmpeg van dentro de la app gracias a
[youtubedl-android](https://github.com/JunkFood02/youtubedl-android), y QuickJS hace de motor de
JavaScript.

Los vídeos se guardan en H.264 + AAC dentro de MP4 siempre que exista esa versión (hasta 1080p en
YouTube), así se abren en cualquier reproductor o editor. Por encima de 1080p se usa el códec que
ofrezca el sitio.

Los directos se graban en MPEG-TS, que sigue siendo reproducible aunque se corte de golpe, y al
detener se pasan a MP4 sin recodificar.

## Compilar

Necesitas [Rust](https://rustup.rs) 1.80 o superior. En Linux, además, las librerías de Tauri:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

Para probarlo:

```bash
cargo run
```

Para generar el instalador (NSIS en Windows; AppImage y .deb en Linux):

```bash
cargo install tauri-cli --version "^2" --locked
cargo tauri build
```

En Linux usa `cargo tauri build --bundles appimage,deb,rpm`. Los paquetes quedan en
`target/release/bundle/`. Las recetas del Flatpak, Arch y Void están en `packaging/` y reempaquetan
el `.deb`.

Para Android hacen falta Node.js, JDK 17, el SDK (paquete `platforms;android-37.0`) y el NDK 27; el proyecto ya está en
`gen/android`:

```bash
cargo tauri android build --apk --target aarch64 armv7
```

Para firmar, crea `gen/android/keystore.properties` (`storeFile`, `storePassword`, `keyAlias`,
`keyPassword`); ese archivo no se sube al repositorio.

### Estructura

```
src/
  main.rs        comandos que usa la interfaz
  downloads.rs   cola, concurrencia, pausa/reanudar, grabación de directos
  channel.rs     listado de canales, perfiles y playlists
  tools.rs       descarga y actualización de yt-dlp y FFmpeg
  settings.rs    ajustes (carpeta, simultáneas)
  platform.rs    clics a través del óvalo, procesos en Windows y Linux
  engine.rs      ejecuta yt-dlp: proceso en escritorio, motor integrado en Android
plugins/oval-engine/   puente con youtubedl-android (Kotlin)
gen/android/     proyecto de Android
ui/              HTML, CSS y JS de la interfaz, sin dependencias
site/            página web (GitHub Pages) con la demo interactiva
packaging/       Flatpak, Arch (PKGBUILD), Void y metadatos de Linux
```

### Publicar una versión

Sube la versión en `Cargo.toml` y `tauri.conf.json`, anota los cambios en `CHANGELOG.md` y empuja
una etiqueta `vX.Y.Z`. GitHub Actions compila Windows, Linux (AppImage, .deb, .rpm, Flatpak,
Arch y Void) y Android y publica todos los archivos. La APK solo se firma si el repositorio tiene los secretos `ANDROID_KEYSTORE_B64` y
`ANDROID_KEYSTORE_PASSWORD`. La página web se publica sola en cada cambio de `site/` o `ui/`.

## Aviso

Descarga y graba solo contenido que tengas derecho a guardar y respeta los términos de cada sitio.

## Licencia

[MIT](LICENSE). yt-dlp (Unlicense) y FFmpeg (GPL) se descargan aparte y mantienen sus licencias.
