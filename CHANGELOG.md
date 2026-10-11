# Changelog

## [Unreleased]

## [0.1.0] - 2026-10-10

### Added
- Ventana ovalada sin bordes y redimensionable; los clics fuera del óvalo pasan a lo que haya detrás.
- Pestaña Enlace: descarga de cualquier enlace compatible con yt-dlp, directo a `Descargas/Oval`.
- Pestaña Canal: lista los vídeos de un canal, perfil o playlist; las miniaturas descartadas se
  marcan en rojo, con selección por rango usando Shift. En YouTube, Vídeos, Shorts o Directos.
- Pestaña Directo: grabación en tiempo real (Twitch, Kick, YouTube, TikTok...) que se guarda en
  MP4 al detener y conserva lo grabado si se corta la conexión.
- MP4 (máxima, 4K, 1440p, 1080p, 720p, 480p, 360p) o MP3 (320, 192, 128 kbps).
- Progreso, velocidad y tiempo estimado; pausar, reanudar, reintentar y cancelar.
- Descargas simultáneas configurables (1 a 8).
- Carpeta de destino configurable.
- Motor (yt-dlp, FFmpeg) descargado en el primer arranque y actualizado a diario.
- Paquetes para Windows (instalador y portable) y Linux (AppImage y .deb).
- En la pestaña Canal, las miniaturas y duraciones que el listado rápido no trae (PeerTube y otros
  sitios) se completan en segundo plano.
- Los vídeos que llegan en AVI, FLV, MOV o WebM se reempaquetan a MP4 sin recodificar.
- Mensaje claro cuando YouTube frena la conexión por exceso de descargas.
