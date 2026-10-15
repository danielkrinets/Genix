#!/bin/bash
# Подписывает dist/Payload/Genix.app сертификатом Apple и (если есть ключ App Store Connect)
# отправляет сборку в TestFlight. Всё берётся из переменных окружения (секреты GitHub):
#   IOS_CERT_P12        — сертификат «Apple Distribution» в .p12, закодированный base64
#   IOS_CERT_PASSWORD   — пароль от .p12
#   IOS_PROFILE         — профиль App Store для com.danielkrinets.genix (.mobileprovision), base64
#   ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_P8 — ключ App Store Connect API (необязательно; без него только подпись)
#   BUILD_NUMBER        — номер сборки (растёт с каждым запуском)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/Payload/Genix.app"
WORK="$(mktemp -d)"
PB=/usr/libexec/PlistBuddy

# --- служебные поля, которые App Store ждёт от сборок Xcode ---
plist="$APP/Info.plist"
set_key() { $PB -c "Delete :$1" "$plist" 2>/dev/null || true; $PB -c "Add :$1 $2 $3" "$plist"; }
[ -f "$DIST/icon.plist" ] && $PB -c "Merge $DIST/icon.plist" "$plist" 2>/dev/null || true
SDK_VER="$(xcrun --sdk iphoneos --show-sdk-version)"
SDK_BUILD="$(xcrun --sdk iphoneos --show-sdk-build-version)"
XC_VER="$(xcodebuild -version | awk '/Xcode/{print $2}')"
XC_BUILD="$(xcodebuild -version | awk '/Build version/{print $3}')"
XC_NUM="$(echo "$XC_VER" | awk -F. '{printf "%d%d%d0", $1, ($2==""?0:$2), ($3==""?0:$3)}')"
set_key CFBundleVersion string "${BUILD_NUMBER:-1}"
set_key DTPlatformName string iphoneos
set_key DTPlatformVersion string "$SDK_VER"
set_key DTPlatformBuild string "$SDK_BUILD"
set_key DTSDKName string "iphoneos$SDK_VER"
set_key DTSDKBuild string "$SDK_BUILD"
set_key DTXcode string "$XC_NUM"
set_key DTXcodeBuild string "$XC_BUILD"
set_key DTCompiler string com.apple.compilers.llvm.clang.1_0
set_key BuildMachineOSBuild string "$(sw_vers -buildVersion)"
$PB -c "Delete :CFBundleSupportedPlatforms" "$plist" 2>/dev/null || true
$PB -c "Add :CFBundleSupportedPlatforms array" -c "Add :CFBundleSupportedPlatforms:0 string iPhoneOS" "$plist"

# --- сертификат во временную связку ключей ---
KC="$WORK/genix.keychain-db"
KCPASS="$(uuidgen)"
echo "$IOS_CERT_P12" | base64 --decode > "$WORK/cert.p12"
security create-keychain -p "$KCPASS" "$KC"
security set-keychain-settings -lut 3600 "$KC"
security unlock-keychain -p "$KCPASS" "$KC"
security import "$WORK/cert.p12" -k "$KC" -P "$IOS_CERT_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$KCPASS" "$KC" >/dev/null
security list-keychains -d user -s "$KC" $(security list-keychains -d user | tr -d '"')
IDENTITY="$(security find-identity -v -p codesigning "$KC" | awk 'NR==1{print $2}')"
[ -n "$IDENTITY" ] || { echo "В .p12 нет сертификата для подписи"; exit 1; }

# --- профиль и права приложения ---
echo "$IOS_PROFILE" | base64 --decode > "$APP/embedded.mobileprovision"
security cms -D -i "$APP/embedded.mobileprovision" > "$WORK/profile.plist"
$PB -x -c "Print :Entitlements" "$WORK/profile.plist" > "$WORK/ent.plist"
codesign --force --sign "$IDENTITY" --keychain "$KC" --entitlements "$WORK/ent.plist" --timestamp=none "$APP"
codesign --verify --verbose "$APP"

(cd "$DIST" && rm -f Genix-iOS.ipa && zip -qry Genix-iOS.ipa Payload)
echo "Подписано: $DIST/Genix-iOS.ipa"

# --- отправка в TestFlight ---
if [ -n "${ASC_KEY_ID:-}" ] && [ -n "${ASC_ISSUER_ID:-}" ] && [ -n "${ASC_KEY_P8:-}" ]; then
  mkdir -p ~/.appstoreconnect/private_keys
  if echo "$ASC_KEY_P8" | grep -q "BEGIN PRIVATE KEY"; then
    printf '%s\n' "$ASC_KEY_P8" > ~/.appstoreconnect/private_keys/AuthKey_$ASC_KEY_ID.p8
  else
    echo "$ASC_KEY_P8" | base64 --decode > ~/.appstoreconnect/private_keys/AuthKey_$ASC_KEY_ID.p8
  fi
  xcrun altool --upload-app -f "$DIST/Genix-iOS.ipa" -t ios --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  echo "Сборка ${BUILD_NUMBER:-1} отправлена в TestFlight"
else
  echo "Ключа App Store Connect нет — только подписанный .ipa"
fi
security delete-keychain "$KC" || true
