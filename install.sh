#!/usr/bin/env bash
# Baut den Bildbetrachter und installiert ihn für den aktuellen Benutzer.
#   ./install.sh             installieren + als Standardprogramm für Bilder setzen
#   ./install.sh --no-default  nur installieren (erscheint unter "Öffnen mit")
#   ./install.sh --uninstall   wieder entfernen
set -euo pipefail
cd "$(dirname "$0")"

APP=simple-image-viewer
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor"
DESKTOP="$APP_DIR/$APP.desktop"

refresh() {
    update-desktop-database "$APP_DIR" 2>/dev/null || true
    command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
    rm -f "$BIN_DIR/$APP" "$DESKTOP" "$ICON_DIR/scalable/apps/$APP.svg" "$ICON_DIR/256x256/apps/$APP.png"
    refresh
    echo "Entfernt. (Einträge in ~/.config/mimeapps.list ggf. in den Systemeinstellungen anpassen.)"
    exit 0
fi

cargo build --release
mkdir -p "$BIN_DIR" "$APP_DIR"
install -m 755 "target/release/$APP" "$BIN_DIR/$APP"
install -Dm 644 "assets/$APP.svg" "$ICON_DIR/scalable/apps/$APP.svg"
install -Dm 644 "assets/$APP.png" "$ICON_DIR/256x256/apps/$APP.png"
gtk-update-icon-cache -q -t "$ICON_DIR" 2>/dev/null || true
sed "s|@BIN@|$BIN_DIR/$APP|" "$APP.desktop" > "$DESKTOP"
refresh
echo "Installiert: $BIN_DIR/$APP"

if [[ "${1:-}" != "--no-default" ]]; then
    [[ -f "$HOME/.config/mimeapps.list" ]] && cp "$HOME/.config/mimeapps.list" "$HOME/.config/mimeapps.list.bak-$APP"
    mimes=$(sed -n 's/^MimeType=//p' "$APP.desktop" | tr ';' ' ')
    xdg-mime default "$APP.desktop" $mimes
    refresh
    echo "Als Standardprogramm für Bilder gesetzt (Backup: ~/.config/mimeapps.list.bak-$APP)."
fi
