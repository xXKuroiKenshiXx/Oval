"""Gets gen/apple ready to build Oval for iOS (run on a Mac, after
`tauri ios init`; the iOS workflow does both).

- Python.xcframework: CPython for iOS from BeeWare's Python-Apple-support.
- app_packages/: yt-dlp, its JavaScript challenge solver and certifi.
- project.yml: embeds Python, adds the install_python.sh build phase and lets
  the Files app show the downloads; then the Xcode project is regenerated.

Downloads are pinned and checked by hash. The app updates yt-dlp by itself.
"""

import hashlib
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
APPLE = ROOT / "gen" / "apple"

PYTHON_SUPPORT = (
    "https://github.com/beeware/Python-Apple-support/releases/download/3.13-b16/Python-3.13-iOS-support.b16.tar.gz",
    "22b96f3d0c3663225f078556b036d2ee6ce3ff98c885706585240681335c71ea",
)
PACKAGES = ["yt-dlp==2026.8.19", "yt-dlp-ejs==0.8.0", "certifi"]


def python_framework() -> None:
    target = APPLE / "Python.xcframework"
    if target.is_dir():
        return
    url, sha256 = PYTHON_SUPPORT
    archive = APPLE / "python-support.tar.gz"
    print(f"Downloading {url}")
    urllib.request.urlretrieve(url, archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != sha256:
        archive.unlink()
        sys.exit(f"Unexpected hash for the Python support package: {digest}")
    with tarfile.open(archive) as tar:
        members = [m for m in tar.getmembers() if m.name.startswith("Python.xcframework/")]
        tar.extractall(APPLE, members=members, filter="tar")
    archive.unlink()


def app_packages() -> None:
    target = APPLE / "app_packages"
    subprocess.run(
        [sys.executable, "-m", "pip", "install", "--quiet", "--upgrade", "--no-deps",
         "--only-binary=:all:", "--target", str(target), *PACKAGES],
        check=True,
    )


def project() -> None:
    spec_path = APPLE / "project.yml"
    spec = yaml.safe_load(spec_path.read_text(encoding="utf-8"))
    name = next(t for t in spec["targets"] if t.endswith("_iOS"))
    target = spec["targets"][name]

    deps = target.setdefault("dependencies", [])
    if not any(d.get("framework") == "Python.xcframework" for d in deps):
        deps.append({"framework": "Python.xcframework", "embed": True})

    scripts = target.setdefault("postCompileScripts", [])
    if not any(s.get("name") == "Install Python" for s in scripts):
        scripts.append({
            "name": "Install Python",
            "script": 'bash "$PROJECT_DIR/../../packaging/ios/install_python.sh"',
            "basedOnDependencyAnalysis": False,
        })

    props = target.setdefault("info", {}).setdefault("properties", {})
    # Downloads live in Documents: "On My iPhone > Oval" in the Files app.
    props["UIFileSharingEnabled"] = True
    props["LSSupportsOpeningDocumentsInPlace"] = True

    spec_path.write_text(yaml.safe_dump(spec, sort_keys=False, allow_unicode=True), encoding="utf-8")
    subprocess.run(["xcodegen", "generate", "--spec", str(spec_path)], cwd=APPLE, check=True)


def main() -> None:
    if not (APPLE / "project.yml").is_file():
        sys.exit("gen/apple is missing: run `tauri ios init` first")
    python_framework()
    app_packages()
    project()


if __name__ == "__main__":
    main()
