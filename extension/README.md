# Extensión de Oval

Un panel flotante para mandar a Oval lo que estás viendo: el vídeo entero, un clip
con entrada y salida, la transcripción o la miniatura. Funciona en YouTube y en
cualquier página con vídeo que yt-dlp sepa descargar.

La extensión no descarga nada por su cuenta: habla con la app de Oval abierta en el
mismo equipo (Windows o Linux), que hace el trabajo y muestra las descargas en su lista.

## Uso

1. Abre Oval.
2. Pulsa el botón de Oval en la barra del navegador (o <kbd>Alt</kbd>+<kbd>Shift</kbd>+<kbd>O</kbd>).
3. En el panel:
   - **[ IN** y **OUT ]** marcan el tramo en el momento actual del vídeo; también
     puedes arrastrar los tiradores. **Preciso** corta en el fotograma exacto.
   - **Mandar a la cola** descarga el tramo como `Título [id] 15m04s-15m22s.mp4`.
   - **Vídeo completo**, **Transcripción** (Markdown, texto, SRT o VTT; solo el tramo
     si hay uno marcado) y **Miniatura**.
   - **Ver en YouTube** abre el panel de transcripción de YouTube.
   - La **nota** (TL;DR) se añade al principio de la transcripción en Markdown.
   - **Destino**: por defecto la carpeta de Oval; el lápiz abre un selector de carpeta.

Clic derecho en un enlace, un vídeo o la página: **Descargar con Oval**.

## Instalar

```bash
python extension/build.py
```

- **Chrome, Edge, Brave, Opera, Vivaldi**: `chrome://extensions` → modo desarrollador →
  «Cargar descomprimida» con `extension/` (o arrastra `dist/oval-extension-chromium.zip`).
- **Firefox y derivados**: `about:debugging` → «Este Firefox» → «Cargar complemento
  temporal» con `dist/oval-extension-firefox.zip`. Para que quede instalada hay que
  firmarla en addons.mozilla.org.
- **Safari** (Mac): `xcrun safari-web-extension-converter extension/dist/safari`.

## Cómo se conecta

Oval escucha en `127.0.0.1` en el primer puerto libre de 17177, 27177 y 37177
(`src/bridge.rs`) y solo acepta peticiones de extensiones: las páginas web no pueden
usarlo. Las rutas están documentadas al principio de `src/bridge.rs`.
