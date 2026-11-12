//! Texterkennung (OCR) eines Bildausschnitts über das Programm `tesseract`.

use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use image::{DynamicImage, ImageFormat, imageops::FilterType};

/// Ausschnitte, deren Höhe darunter liegt, werden vor der Erkennung vergrößert –
/// Tesseract arbeitet bei sehr kleiner Schrift deutlich schlechter.
const MIN_HEIGHT: u32 = 200;
const MAX_UPSCALE: u32 = 4;

/// Bevorzugte Sprachen (sofern installiert). Überschreibbar mit `SIV_OCR_LANG`, z. B. "fra+eng".
const PREFERRED_LANGS: &[&str] = &["deu", "eng"];

/// Erkennt den Text im Ausschnitt `[x, y, breite, höhe]` (in Bildpixeln) der Bilddatei.
pub fn recognize(path: &Path, region: [u32; 4]) -> Result<String, String> {
    let img = crate::loader::decode_still(path).map_err(|e| e.to_string())?;
    let [x, y, w, h] = region;
    let x = x.min(img.width().saturating_sub(1));
    let y = y.min(img.height().saturating_sub(1));
    let w = w.min(img.width() - x).max(1);
    let h = h.min(img.height() - y).max(1);
    let crop = img.crop_imm(x, y, w, h);
    recognize_image(&crop)
}

fn recognize_image(img: &DynamicImage) -> Result<String, String> {
    let mut img = DynamicImage::ImageLuma8(img.to_luma8());
    if img.height() < MIN_HEIGHT {
        let factor = MIN_HEIGHT.div_ceil(img.height()).min(MAX_UPSCALE);
        if factor > 1 {
            img = img.resize(img.width() * factor, img.height() * factor, FilterType::CatmullRom);
        }
    }

    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).map_err(|e| e.to_string())?;

    let mut cmd = Command::new("tesseract");
    cmd.args(["stdin", "stdout"]);
    if let Some(langs) = languages() {
        cmd.args(["-l", &langs]);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "tesseract ist nicht installiert (sudo apt install tesseract-ocr tesseract-ocr-deu)"
                    .to_string()
            } else {
                format!("tesseract konnte nicht gestartet werden: {e}")
            }
        })?;

    // Schreiben in eigenem Thread, damit eine volle stdout-Pipe nicht blockiert.
    let mut stdin = child.stdin.take().expect("stdin ist piped");
    let writer = std::thread::spawn(move || stdin.write_all(&png));
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tesseract: {}", err.trim()));
    }
    Ok(clean(&String::from_utf8_lossy(&output.stdout)))
}

/// Sprachen für `-l`: `SIV_OCR_LANG` oder die installierten bevorzugten Sprachen.
fn languages() -> Option<String> {
    if let Ok(l) = std::env::var("SIV_OCR_LANG")
        && !l.trim().is_empty()
    {
        return Some(l.trim().to_string());
    }
    let out = Command::new("tesseract").arg("--list-langs").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let installed: Vec<&str> = text.lines().map(str::trim).collect();
    let langs: Vec<&str> =
        PREFERRED_LANGS.iter().copied().filter(|l| installed.contains(l)).collect();
    (!langs.is_empty()).then(|| langs.join("+"))
}

/// Entfernt Leerzeichen am Zeilenende, mehrfache Leerzeilen und das abschließende Seitenende (\f).
fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in text.replace('\u{c}', "").lines().map(str::trim_end) {
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_tesseract_output() {
        assert_eq!(
            clean("Hallo  \nWelt\n\n\n\nZweiter Absatz\n\n\u{c}"),
            "Hallo\nWelt\n\nZweiter Absatz"
        );
        assert_eq!(clean("\n\n  \n\u{c}"), "");
    }

    /// Erkennt den Text in `$SIV_OCR_TEST` (falls gesetzt; Format "bild.png:x,y,w,h").
    #[test]
    fn recognize_test_image() {
        let Ok(spec) = std::env::var("SIV_OCR_TEST") else { return };
        let (path, region) = spec.rsplit_once(':').expect("bild.png:x,y,w,h");
        let r: Vec<u32> = region.split(',').map(|v| v.parse().unwrap()).collect();
        crate::loader::register_decoders();
        let text = recognize(Path::new(path), [r[0], r[1], r[2], r[3]]).unwrap();
        println!("{text}");
    }
}
