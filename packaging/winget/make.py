"""Writes the winget manifests for a published release.

    python packaging/winget/make.py 0.2.0

Reads the installer's SHA-256 from the GitHub release and writes the three
manifest files into packaging/winget/<version>/, ready to go to
microsoft/winget-pkgs under manifests/x/xXKuroiKenshiXx/Oval/<version>/.
"""
import hashlib
import sys
import urllib.request
from pathlib import Path

ID = "xXKuroiKenshiXx.Oval"
REPO = "https://github.com/xXKuroiKenshiXx/Oval"
SCHEMA = "1.12.0"

version = sys.argv[1]
url = f"{REPO}/releases/download/v{version}/Oval_{version}_x64-setup.exe"
with urllib.request.urlopen(url) as res:
    sha = hashlib.sha256(res.read()).hexdigest().upper()

out = Path(__file__).resolve().parent / version
out.mkdir(exist_ok=True)
head = lambda kind: (
    f"# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.{SCHEMA}.schema.json\n\n"
)

(out / f"{ID}.yaml").write_text(head("version") + f"""PackageIdentifier: {ID}
PackageVersion: {version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: {SCHEMA}
""", encoding="utf-8")

(out / f"{ID}.installer.yaml").write_text(head("installer") + f"""PackageIdentifier: {ID}
PackageVersion: {version}
InstallerType: nullsoft
Scope: user
InstallModes:
- interactive
- silent
UpgradeBehavior: install
Installers:
- Architecture: x64
  InstallerUrl: {url}
  InstallerSha256: {sha}
ManifestType: installer
ManifestVersion: {SCHEMA}
""", encoding="utf-8")

(out / f"{ID}.locale.en-US.yaml").write_text(head("defaultLocale") + f"""PackageIdentifier: {ID}
PackageVersion: {version}
PackageLocale: en-US
Publisher: xXKuroiKenshiXx
PublisherUrl: https://github.com/xXKuroiKenshiXx
PublisherSupportUrl: {REPO}/issues
PackageName: Oval
PackageUrl: https://xxkuroikenshixx.github.io/Oval/
License: MIT
LicenseUrl: {REPO}/blob/main/LICENSE
ShortDescription: Minimal oval video downloader for single videos, whole channels and live streams.
Description: |-
  Paste a link and download. Oval is a round, borderless video downloader built on yt-dlp:
  single videos, whole channels or profiles (pick the videos with a tap) and live streams
  recorded in real time, from YouTube, X, Instagram, TikTok, Twitch, Kick and more.
Moniker: oval
Tags:
- downloader
- video
- video-downloader
- yt-dlp
- live-stream
ReleaseNotesUrl: {REPO}/releases/tag/v{version}
ManifestType: defaultLocale
ManifestVersion: {SCHEMA}
""", encoding="utf-8")
print(out, sha)
