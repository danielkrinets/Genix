#!/bin/bash
# Собирает AppImage из готового бинарника.
# Использование: packaging/linux/appimage.sh путь/к/genix x86_64|aarch64
set -euo pipefail
BIN="${1:?путь к бинарнику genix}"
ARCH="${2:?x86_64 или aarch64}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DIST="$ROOT/dist"
APPDIR="$DIST/Genix.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$DIST"
cp "$BIN" "$APPDIR/usr/bin/genix"
chmod +x "$APPDIR/usr/bin/genix"
cp "$ROOT/packaging/macos/AppIcon.iconset/icon_512x512.png" "$APPDIR/genix.png"
cat > "$APPDIR/genix.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Genix
Comment=Parametric 3D CAD
Exec=genix
Icon=genix
Categories=Graphics;Engineering;
Terminal=false
EOF
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
exec "$(dirname "$(readlink -f "$0")")/usr/bin/genix" "$@"
EOF
chmod +x "$APPDIR/AppRun"
TOOL="$DIST/appimagetool"
curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$(uname -m).AppImage"
chmod +x "$TOOL"
ARCH="$ARCH" "$TOOL" --appimage-extract-and-run "$APPDIR" "$DIST/Genix-linux-$ARCH.AppImage"
rm -f "$TOOL"
echo "Готово: $DIST/Genix-linux-$ARCH.AppImage"
