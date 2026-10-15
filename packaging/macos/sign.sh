#!/bin/bash
# Подписывает dist/Genix.app сертификатом «Developer ID Application» и проходит нотаризацию Apple,
# чтобы программа открывалась двойным щелчком без предупреждений. Переменные (секреты GitHub):
#   MAC_CERT_P12, MAC_CERT_PASSWORD        — сертификат Developer ID Application (.p12, base64) и пароль
#   ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_P8  — ключ App Store Connect API (тот же, что для TestFlight)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/Genix.app"
WORK="$(mktemp -d)"

KC="$WORK/genix-mac.keychain-db"
KCPASS="$(uuidgen)"
echo "$MAC_CERT_P12" | base64 --decode > "$WORK/cert.p12"
security create-keychain -p "$KCPASS" "$KC"
security set-keychain-settings -lut 3600 "$KC"
security unlock-keychain -p "$KCPASS" "$KC"
security import "$WORK/cert.p12" -k "$KC" -P "$MAC_CERT_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$KCPASS" "$KC" >/dev/null
security list-keychains -d user -s "$KC" $(security list-keychains -d user | tr -d '"')
IDENTITY="$(security find-identity -v -p codesigning "$KC" | awk '/Developer ID Application/{print $2; exit}')"
[ -n "$IDENTITY" ] || { echo "В .p12 нет сертификата «Developer ID Application»"; exit 1; }

# hardened runtime + метка времени — обязательны для нотаризации
codesign --force --options runtime --timestamp --sign "$IDENTITY" --keychain "$KC" "$APP"
codesign --verify --strict --verbose=2 "$APP"

if [ -n "${ASC_KEY_ID:-}" ] && [ -n "${ASC_ISSUER_ID:-}" ] && [ -n "${ASC_KEY_P8:-}" ]; then
  KEY="$WORK/AuthKey.p8"
  if echo "$ASC_KEY_P8" | grep -q "BEGIN PRIVATE KEY"; then printf '%s\n' "$ASC_KEY_P8" > "$KEY"
  else echo "$ASC_KEY_P8" | base64 --decode > "$KEY"; fi
  (cd "$DIST" && rm -f notarize.zip && ditto -c -k --keepParent Genix.app notarize.zip)
  xcrun notarytool submit "$DIST/notarize.zip" --key "$KEY" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER_ID" --wait --timeout 30m
  xcrun stapler staple "$APP"
  spctl --assess --type execute --verbose "$APP"
  rm -f "$DIST/notarize.zip"
  echo "Подписано и нотаризовано"
else
  echo "Ключа App Store Connect нет — подписано без нотаризации"
fi

(cd "$DIST" && rm -f Genix-macOS.zip && ditto -c -k --keepParent Genix.app Genix-macOS.zip)
security delete-keychain "$KC" || true
