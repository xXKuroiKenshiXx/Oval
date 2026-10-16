# Oval en iPhone y iPad

Oval funciona en iOS e iPadOS 13 o superior. Cada versión trae `Oval_x.y.z_ios.ipa`
**sin firmar**: Apple solo deja instalar apps firmadas, así que se firma al instalarla
con tu Apple ID (gratis; con una cuenta gratuita hay que reinstalar cada 7 días).

## Instalar desde Linux o Windows con xtool

[xtool](https://github.com/xtool-org/xtool) firma e instala apps sin un Mac. En
Windows se usa desde WSL.

1. Instala xtool y usbmuxd (en WSL o Linux):

   ```bash
   sudo apt install usbmuxd libimobiledevice-utils
   curl -fL "https://github.com/xtool-org/xtool/releases/latest/download/xtool-$(uname -m).AppImage" -o xtool
   chmod +x xtool && sudo mv xtool /usr/local/bin/
   ```

2. Conecta el iPhone por USB, desbloquéalo y acepta «Confiar en este ordenador».
   En Windows, pasa el iPhone a WSL con [usbipd-win](https://learn.microsoft.com/windows/wsl/connect-usb)
   (`usbipd bind` y `usbipd attach --wsl`) y comprueba que `ideviceinfo` lo ve.
3. Inicia sesión con tu Apple ID (una vez): `xtool auth login`.
4. Instala: `xtool install Oval_x.y.z_ios.ipa`.
5. En el iPhone: Ajustes › General › VPN y gestión de dispositivos › confía en tu
   Apple ID. En iOS 16 o superior activa además Ajustes › Privacidad y seguridad ›
   Modo de desarrollador.

También sirven AltStore, SideStore o Sideloadly con el mismo `.ipa`.

## Qué cambia respecto a escritorio

iOS no deja que una app arranque otros programas, así que todo va dentro de Oval:
Python y yt-dlp embebidos, JavaScriptCore para los desafíos de YouTube y AVFoundation
para unir vídeo y audio. Por eso:

- Los vídeos se guardan en H.264 + AAC (MP4) hasta 1080p.
- El audio se guarda como M4A (AAC), sin convertir a MP3.
- Los directos todavía no se pueden grabar.
- Las descargas están en la app Archivos › En mi iPhone › Oval.
- yt-dlp se actualiza solo desde PyPI y se usa a partir del siguiente arranque.

## Compilar

Hace falta un Mac con Xcode (lo hace el workflow `.github/workflows/ios.yml`):

```bash
cargo tauri ios init            # solo la primera vez: crea gen/apple
python3 packaging/ios/prepare.py  # Python para iOS, yt-dlp y ajustes del proyecto
cargo tauri ios build --no-sign --target aarch64
```

`prepare.py` descarga el Python para iOS de
[BeeWare](https://github.com/beeware/Python-Apple-support) (comprobado por hash) y
añade al proyecto `install_python.sh`, que mete la biblioteca estándar en la app y
convierte sus módulos nativos en frameworks, como exige iOS.

Con `OVAL_SELFTEST=<enlace>` la app descarga ese enlace, escribe el resultado y se
cierra; el workflow lo usa en el simulador.
