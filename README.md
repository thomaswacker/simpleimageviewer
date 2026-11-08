# Simple Image Viewer

<img src="assets/simple-image-viewer.svg" width="96" align="right" alt="Icon">

Einfacher, schneller Bildbetrachter für Linux (KDE/Wayland und X11), geschrieben in Rust mit [egui](https://github.com/emilk/egui).

- Formate: JPEG, PNG, GIF, WebP, **AVIF**, **HEIC/HEIF**, **JPEG XL**, BMP, TIFF, ICO, TGA, QOI, PNM, EXR, HDR, DDS
- Animierte GIF, WebP und APNG werden abgespielt
- Startet maximiert; Bilder werden in 100 % angezeigt (1 Bildpixel = 1 Bildschirmpixel), zu große Bilder werden eingepasst
- EXIF-Ausrichtung wird berücksichtigt
- Nachbarbilder werden im Hintergrund vorgeladen

## Bedienung

| Taste / Aktion | Funktion |
|---|---|
| ← / →, Bild↑ / Bild↓ | vorheriges / nächstes Bild im Ordner |
| Pos1 / Ende | erstes / letztes Bild |
| Mausrad, `+` / `-` | Zoom |
| `1` | ursprünglicher Zoom (100 % bzw. eingepasst) |
| Linke Maustaste ziehen | Bild verschieben |
| Leertaste | Animation anhalten / fortsetzen |
| F11, `F`, Doppelklick | Vollbild |
| Esc / `Q` | Vollbild verlassen bzw. beenden |

## Installation

Benötigt Rust und libheif (für HEIC/AVIF), unter Debian/Ubuntu:

```sh
sudo apt install libheif-dev libheif-plugin-dav1d libheif-plugin-libde265
./install.sh               # bauen, installieren, als Standard-Bildbetrachter setzen
./install.sh --no-default  # nur installieren ("Öffnen mit" in Dolphin)
./install.sh --uninstall   # entfernen
```

Installiert wird nach `~/.local/bin`, `~/.local/share/applications` und `~/.local/share/icons`.

Aufruf von Hand: `simple-image-viewer <Bild oder Ordner>`

## Lizenz

MIT
