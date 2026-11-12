# Simple Image Viewer

<img src="assets/simple-image-viewer.svg" width="96" align="right" alt="Icon">

Einfacher, schneller Bildbetrachter für Linux (KDE/Wayland und X11), geschrieben in Rust mit [egui](https://github.com/emilk/egui).

- Formate: JPEG, PNG, GIF, WebP, **AVIF**, **HEIC/HEIF**, **JPEG XL**, BMP, TIFF, ICO, TGA, QOI, PNM, EXR, HDR, DDS
- Animierte GIF, WebP und APNG werden abgespielt
- Startet maximiert; Bilder werden in 100 % angezeigt (1 Bildpixel = 1 Bildschirmpixel), zu große Bilder werden eingepasst
- EXIF-Ausrichtung wird berücksichtigt
- Nachbarbilder werden im Hintergrund vorgeladen
- Texterkennung (OCR): Bereich mit der Maus auswählen, der erkannte Text landet in der Zwischenablage

## Bedienung

| Taste / Aktion | Funktion |
|---|---|
| ← / →, Bild↑ / Bild↓ | vorheriges / nächstes Bild im Ordner |
| Pos1 / Ende | erstes / letztes Bild |
| Mausrad, `+` / `-` | Zoom |
| `1` | ursprünglicher Zoom (100 % bzw. eingepasst) |
| Linke Maustaste ziehen | Bild verschieben |
| Rechte Maustaste ziehen, Umschalt + linke Maustaste ziehen | Bereich auswählen → Text erkennen (OCR) und in die Zwischenablage kopieren |
| Leertaste | Animation anhalten / fortsetzen |
| F11, `F`, Doppelklick | Vollbild |
| Esc / `Q` | OCR-Auswahl abbrechen, Vollbild verlassen bzw. beenden |

## Installation

Benötigt Rust und libheif (für HEIC/AVIF), unter Debian/Ubuntu:

```sh
sudo apt install libheif-dev libheif-plugin-dav1d libheif-plugin-libde265
sudo apt install tesseract-ocr tesseract-ocr-deu   # optional, für die Texterkennung
./install.sh               # bauen, installieren, als Standard-Bildbetrachter setzen
./install.sh --no-default  # nur installieren ("Öffnen mit" in Dolphin)
./install.sh --uninstall   # entfernen
```

Installiert wird nach `~/.local/bin`, `~/.local/share/applications` und `~/.local/share/icons`.

Aufruf von Hand: `simple-image-viewer <Bild oder Ordner>`

Die Texterkennung nutzt Deutsch und Englisch, sofern installiert; andere Sprachen über die Umgebungsvariable `SIV_OCR_LANG` (z. B. `SIV_OCR_LANG=fra+eng`). Unter Wayland bleibt der kopierte Text nach dem Schließen des Betrachters nur erhalten, wenn ein Zwischenablage-Verlauf läuft (bei KDE: Klipper, standardmäßig aktiv).

## Lizenz

MIT
