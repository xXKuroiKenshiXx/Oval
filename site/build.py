"""Assembles the GitHub Pages site into _site/.

The landing page lives in site/; the interactive demo is the real UI from ui/
with site/mock.js standing in for the Rust backend.
"""
import shutil
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
out = root / "_site"
shutil.rmtree(out, ignore_errors=True)
(out / "demo").mkdir(parents=True)

for name in ("index.html", "site.css", "site.js"):
    shutil.copy(root / "site" / name, out / name)
shutil.copy(root / "icons" / "icon.png", out / "icon.png")
shutil.copy(root / "docs" / "demo.mp4", out / "demo.mp4")

# A still frame for the <video> poster.
if (root / "docs" / "poster.jpg").exists():
    shutil.copy(root / "docs" / "poster.jpg", out / "poster.jpg")
elif shutil.which("ffmpeg"):
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-ss", "40", "-i", str(out / "demo.mp4"),
         "-frames:v", "1", "-q:v", "4", str(out / "poster.jpg")],
        check=False,
    )

html = (root / "ui" / "index.html").read_text(encoding="utf-8")
marker = '<script src="app.js"></script>'
if marker not in html:
    sys.exit("ui/index.html no longer loads app.js the expected way")
html = html.replace(marker, '<script src="mock.js"></script>\n  ' + marker)
html = html.replace("<title>Oval</title>", "<title>Oval · demo</title>")
(out / "demo" / "index.html").write_text(html, encoding="utf-8")
for name in ("style.css", "app.js"):
    shutil.copy(root / "ui" / name, out / "demo" / name)
shutil.copy(root / "site" / "mock.js", out / "demo" / "mock.js")
(out / ".nojekyll").write_text("")
print("site ->", out)
