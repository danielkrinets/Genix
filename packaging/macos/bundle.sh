#!/bin/bash
# Собирает «Genix.app» из готового бинарника и упаковывает в zip.
# Использование: packaging/macos/bundle.sh путь/к/genix
set -euo pipefail
BIN="${1:?путь к бинарнику genix}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/Genix.app"
rm -rf "$DIST"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/genix"
chmod +x "$APP/Contents/MacOS/genix"
cp "$ROOT/packaging/macos/Info.plist" "$APP/Contents/Info.plist"
iconutil -c icns "$ROOT/packaging/macos/AppIcon.iconset" -o "$APP/Contents/Resources/AppIcon.icns"
# подпись «ad-hoc»: без неё macOS на Apple Silicon не запустит программу
codesign --force --deep --sign - "$APP"
(cd "$DIST" && ditto -c -k --keepParent "Genix.app" "Genix-macOS.zip")
echo "Готово: $DIST/Genix-macOS.zip"
