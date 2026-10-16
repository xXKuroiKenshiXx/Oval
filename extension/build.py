"""Packs the extension for each browser into extension/dist/.

    python extension/build.py

chromium.zip  Chrome, Edge, Brave, Opera, Vivaldi (and the Chrome Web Store)
firefox.zip   Firefox, LibreWolf, Floorp, Zen (and addons.mozilla.org)
safari/       the same files for `xcrun safari-web-extension-converter` on a Mac
"""

import json
import shutil
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DIST = ROOT / "dist"
FILES = ["src/background.js", "src/panel.js", *(f"icons/{s}.png" for s in (16, 32, 48, 96, 128))]


def app_version() -> str:
    """The extension ships with the app, so it takes the app's version."""
    for line in (ROOT.parent / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        if line.startswith("version"):
            return line.split('"')[1]
    raise SystemExit("No version in Cargo.toml")


def manifest(browser: str) -> dict:
    m = json.loads((ROOT / "manifest.json").read_text(encoding="utf-8"))
    m["version"] = app_version()
    if browser == "firefox":
        # Firefox runs background scripts as an event page, not a service worker.
        m["background"] = {"scripts": ["src/background.js"]}
        m["browser_specific_settings"] = {
            "gecko": {
                "id": "oval@xxkuroikenshixx.github.io",
                "strict_min_version": "128.0",
                "data_collection_permissions": {"required": ["none"]},
            }
        }
    return m


def write_zip(name: str, browser: str) -> Path:
    out = DIST / f"oval-extension-{browser}.zip"
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("manifest.json", json.dumps(manifest(browser), indent=2, ensure_ascii=False))
        for f in FILES:
            z.write(ROOT / f, f)
    return out


def main() -> None:
    shutil.rmtree(DIST, ignore_errors=True)
    DIST.mkdir()
    for browser in ("chromium", "firefox"):
        print(write_zip(browser, browser).relative_to(ROOT.parent))
    safari = DIST / "safari"
    for f in FILES:
        (safari / f).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / f, safari / f)
    (safari / "manifest.json").write_text(json.dumps(manifest("safari"), indent=2, ensure_ascii=False), encoding="utf-8")
    print(safari.relative_to(ROOT.parent))


if __name__ == "__main__":
    main()
