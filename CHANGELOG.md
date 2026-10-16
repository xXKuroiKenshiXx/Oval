# Changelog

## [Unreleased]

## [0.5.2] - 2026-10-16

### Added
- Extensión: directos de Twitch y Kick. En un canal en directo el panel marca entrada y
  salida sobre el directo, baja el último minuto, 5, 15 o 60 minutos, todo desde el inicio
  o lo graba desde ahora (y ✕ detiene la grabación y la guarda). Los tramos salen de la
  grabación que Twitch y Kick guardan mientras emiten.
- Extensión: «Abrir Oval» arranca la app si está cerrada, sea la instalada o la portable,
  y cualquier botón la abre solo cuando hace falta. Oval registra el enlace `oval://` cada
  vez que se abre, así que sigue funcionando aunque muevas el portable.

### Changed
- Solo se abre un Oval a la vez: abrirlo otra vez trae al frente el que ya está abierto.

## [0.5.1] - 2026-10-16

### Fixed
- Kick: los vídeos con los enlaces nuevos de Kick (`kick.com/canal/videos/01a1…`) ya se
  descargan, también por tramos desde la extensión, en lugar de fallar con «HTTP Error 404».
- Kick: la transcripción usa los subtítulos automáticos de Kick (español, inglés o portugués)
  y la miniatura se baja aunque Kick bloquee su enlace principal.
- Transcripciones sin frases repetidas cuando los subtítulos se escriben palabra a palabra.

## [0.5.0] - 2026-10-16

### Added
- iPhone y iPad (iOS 13 o superior): `.ipa` sin firmar para instalar con xtool, AltStore o
  Sideloadly. yt-dlp corre dentro de la app sobre un Python embebido, con JavaScriptCore para
  YouTube y AVFoundation para unir vídeo y audio. Vídeo en MP4 hasta 1080p y audio en M4A; los
  directos aún no se pueden grabar.
- Extensión para Chrome, Edge, Brave, Opera, Firefox y Safari: panel flotante con clip
  (entrada, salida y corte preciso), vídeo completo, transcripción en Markdown, texto, SRT o
  VTT (con nota TL;DR), miniatura y carpeta de destino. También «Descargar con Oval» en el
  menú del clic derecho.
- Servidor local para la extensión, que solo acepta peticiones de extensiones del navegador.

## [0.4.1] - 2026-10-12

**Actualización de emergencia de 0.4.1**: reemplaza los paquetes anteriores de esta misma
versión porque el primer arreglo no solucionó todos los casos.

### Fixed
- Rutas y metadatos transmitidos como JSON: acentos, caracteres Unicode y separadores en títulos
  ya no convierten un archivo existente en «No se encontró el archivo descargado».
- Solo Canal crea subcarpetas. Videos individuales, audio y directos se guardan directamente en
  la carpeta seleccionada, sin crear una carpeta por video.
- Pegado de enlaces dentro de texto, enlaces de Markdown, parámetros y formatos compartidos.
- Validación previa de formatos: si una URL de CDN falla, el motor intenta otra variante
  descargable, comprobado con videos de TikTok.

## [0.4.0] - 2026-10-12

### Added
- Kick y Twitch: videos grabados y clips en Canal, miniaturas, selección y descargas.
- Kick: clips paginados y enlaces antiguos de videos.
- Número de versión abajo del círculo.

### Changed
- Buscador del canal con color violeta distintivo.
- Toda descarga terminada se valida y se coloca en una carpeta con el mismo nombre:
  `elpepe/elpepe.mp4`, incluidos directos y audio. Los archivos existentes no se sobrescriben.
- La detección de archivos ya descargados también revisa estas subcarpetas.

## [0.3.0] - 2026-10-11

### Added
- Papelera en las descargas terminadas: dos toques y el archivo se borra (a la papelera de reciclaje
  en Windows y Linux). La × sigue quitándola solo de la lista.

### Fixed
- Android: las descargas ya no se frenan con la pantalla apagada o la app en segundo plano. Mientras
  descarga, Oval mantiene un servicio con la notificación «Descargando…», la CPU despierta y el
  Wi-Fi fuera del ahorro de energía.
- Descargas más estables en todas las versiones: si la conexión se corta, reintenta solas con pausas
  crecientes en lugar de fallar.
- Windows: el PC ya no se suspende en mitad de una descarga.
- Windows: un vídeo de un canal ya no puede quedar fuera de la carpeta del canal; al terminar se
  comprueba dónde quedó, se mueve dentro si hace falta y se borra la carpeta vacía con su nombre.
  Los nombres de carpeta ya no terminan en punto o espacio ni usan nombres reservados de Windows.

## [0.2.1] - 2026-10-11

### Fixed
- Android: la app se cerraba al abrirla por primera vez (pantalla negra). La versión publicada
  optimizaba con R8 la librería que descomprime el motor y fallaba en cualquier teléfono.
- Android: la interfaz no aparecía en teléfonos con un WebView antiguo (Android 7 a 10 sin
  actualizar).
- YouTube: cuando frenaba la conexión a veces respondía «This video is not available»; ahora
  Oval usa siempre los reproductores alternativos y reintenta solo.
- AppImage: yt-dlp, FFmpeg y Deno ya no heredan las librerías internas de la AppImage.
- Flatpak: se vuelve a construir en GitHub Actions.

## [0.2.0] - 2026-10-11

### Added
- Paquetes para más sistemas Linux: Flatpak, .rpm (Fedora, openSUSE), Arch Linux y Void Linux.
- Canal: los vídeos que quedaron a medias (descarga cortada o app cerrada) se marcan con borde
  punteado amarillo y siguen desde donde iban.
- Canal: los archivos sin `[id]` en el nombre también cuentan como descargados si el título coincide.

### Changed
- Canal: las miniaturas ya descargadas se marcan en amarillo mientras carga la lista, y al terminar
  una descarga su miniatura pasa a amarillo.
- Canal: «Todos» ya no vuelve a seleccionar lo que ya está descargado.
- Web: la demo interactiva se distingue mejor y tiene un recorrido de tres pasos.

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
- Si YouTube bloquea ("confirma que no eres un bot"), Oval usa otros reproductores de YouTube
  durante 30 minutos y reintenta la descarga solo.
- Deno se descarga en segundo plano si el equipo no tiene un motor de JavaScript.
- Pestaña Canal: buscador por título y vídeos ya descargados marcados en amarillo.
- Versión para Android (APK) con el motor integrado.
- Página web en GitHub Pages con la interfaz real en modo demo.
