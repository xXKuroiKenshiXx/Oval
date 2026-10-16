#!/bin/bash
# Xcode build phase (added by prepare.py): puts Python's standard library and
# yt-dlp inside Oval.app. iOS only loads native code from frameworks, so every
# compiled module (.so) becomes its own framework with a .fwork pointer left
# in its place, as CPython's iOS support expects (PEP 730).
set -euo pipefail

XCFRAMEWORK="$PROJECT_DIR/Python.xcframework"
APP="$CODESIGNING_FOLDER_PATH"
case "$EFFECTIVE_PLATFORM_NAME" in
  -iphonesimulator) SLICE=ios-arm64_x86_64-simulator ;;
  -iphoneos) SLICE=ios-arm64 ;;
  *) echo "Unsupported platform $EFFECTIVE_PLATFORM_NAME"; exit 1 ;;
esac
ARCH="${ARCHS%% *}"

# Standard library, without what an app never imports.
mkdir -p "$APP/python/lib"
rsync -a --delete "$XCFRAMEWORK/lib/" "$APP/python/lib/" \
  --exclude 'libpython*.dylib' --exclude 'test/' --exclude 'idlelib/' \
  --exclude 'tkinter/' --exclude 'turtledemo/' --exclude '__pycache__/'
rsync -a "$XCFRAMEWORK/$SLICE/lib-$ARCH/" "$APP/python/lib/" --exclude 'libpython*.dylib'
rsync -a --delete "$PROJECT_DIR/app_packages/" "$APP/app_packages/" --exclude '__pycache__/'

# Unsigned builds sign ad hoc; the real signature comes when installing.
IDENTITY="${EXPANDED_CODE_SIGN_IDENTITY:--}"

LIB_DYNLOAD=$(ls -d "$APP"/python/lib/python3.*/lib-dynload)
find "$LIB_DYNLOAD" -name "*.so" | while read -r so; do
  module=$(basename "$so" | cut -d. -f1)
  framework="Frameworks/$module.framework"
  mkdir -p "$APP/$framework"
  cp "$XCFRAMEWORK/build/iOS-dylib-Info-template.plist" "$APP/$framework/Info.plist"
  plutil -replace CFBundleExecutable -string "$module" "$APP/$framework/Info.plist"
  plutil -replace CFBundleIdentifier -string "$(echo "$PRODUCT_BUNDLE_IDENTIFIER.$module" | tr _ -)" "$APP/$framework/Info.plist"
  mv "$so" "$APP/$framework/$module"
  echo "$framework/$module" > "${so%.so}.fwork"
  relative="${so#"$APP"/}"
  echo "${relative%.so}.fwork" > "$APP/$framework/$module.origin"
  /usr/bin/codesign --force --sign "$IDENTITY" --timestamp=none "$APP/$framework"
done
