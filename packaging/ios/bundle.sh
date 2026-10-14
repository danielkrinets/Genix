#!/bin/bash
# Собирает Genix.app для iPhone/iPad и неподписанный .ipa.
# Использование: packaging/ios/bundle.sh путь/к/genix (бинарник для aarch64-apple-ios)
set -euo pipefail
BIN="${1:?путь к бинарнику}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/Payload/Genix.app"
rm -rf "$DIST/Payload"
mkdir -p "$APP"
cp "$BIN" "$APP/genix"
cp "$ROOT/packaging/ios/Info.plist" "$APP/Info.plist"
# значок через каталог ресурсов (если есть Xcode)
XC="$DIST/Assets.xcassets/AppIcon.appiconset"
mkdir -p "$XC"
cp "$ROOT/packaging/logo/genix-1024.png" "$XC/icon-1024.png"
cat > "$XC/Contents.json" <<'J'
{ "images": [ { "filename": "icon-1024.png", "idiom": "universal", "platform": "ios", "size": "1024x1024" } ], "info": { "author": "xcode", "version": 1 } }
J
echo '{ "info": { "author": "xcode", "version": 1 } }' > "$DIST/Assets.xcassets/Contents.json"
xcrun actool "$DIST/Assets.xcassets" --compile "$APP" --platform iphoneos --minimum-deployment-target 15.0 \
  --app-icon AppIcon --output-partial-info-plist "$DIST/icon.plist" >/dev/null 2>&1 || echo "значок не собран (нет actool) — продолжаем"
(cd "$DIST" && rm -f Genix-iOS-unsigned.ipa && zip -qr Genix-iOS-unsigned.ipa Payload)
echo "Готово: $DIST/Genix-iOS-unsigned.ipa"
