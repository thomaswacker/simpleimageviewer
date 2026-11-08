//! Dekodieren von Bilddateien in RGBA-Pixel für die Anzeige.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use eframe::egui::ColorImage;
use image::codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder};
use image::{
    AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, ImageResult,
    RgbaImage, imageops::FilterType,
};

/// Dateiendungen, die beim Blättern im Ordner berücksichtigt werden.
const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "webp", "avif", "heic", "heif", "hif",
    "jxl", "bmp", "dib", "tif", "tiff", "ico", "tga", "qoi", "pnm", "pbm", "pgm", "ppm", "pam",
    "exr", "hdr", "dds", "ff",
];

pub struct Loaded {
    /// Einzelbilder mit Anzeigedauer. Bei Standbildern genau ein Eintrag.
    pub frames: Vec<(ColorImage, Duration)>,
    pub original_size: [usize; 2],
}

/// Animationen werden nur bis zu dieser Gesamtgröße (RGBA im Speicher) geladen.
const MAX_ANIMATION_BYTES: usize = 1536 * 1024 * 1024;

/// Registriert die zusätzlichen Dekoder (HEIC/HEIF/AVIF über libheif, JPEG XL über jxl-oxide)
/// beim `image`-Crate.
pub fn register_decoders() {
    libheif_rs::integration::image::register_all_decoding_hooks();
    jxl_oxide::integration::register_image_decoding_hook();
}

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Lädt ein Bild (bei GIF, WebP und APNG ggf. alle Einzelbilder der Animation).
/// Ist es größer als die maximale Texturgröße der Grafikkarte, wird es dafür
/// verkleinert (die Originalgröße wird mitgeliefert).
pub fn load(path: &Path, max_side: usize) -> Result<Loaded, String> {
    let max_side = max_side.max(1) as u32;

    let (frames, original_size) = match decode_animation(path).map_err(|e| e.to_string())? {
        Some(frames) => {
            let (w, h) = frames[0].0.dimensions();
            let frames = frames
                .into_iter()
                .map(|(img, delay)| (to_color_image(DynamicImage::ImageRgba8(img), max_side), delay))
                .collect();
            (frames, [w as usize, h as usize])
        }
        None => {
            let img = decode_still(path).map_err(|e| e.to_string())?;
            let size = [img.width() as usize, img.height() as usize];
            (vec![(to_color_image(img, max_side), Duration::ZERO)], size)
        }
    };
    Ok(Loaded { frames, original_size })
}

fn to_color_image(img: DynamicImage, max_side: u32) -> ColorImage {
    let img = if img.width() > max_side || img.height() > max_side {
        img.resize(max_side, max_side, FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
}

/// Liefert `Some(frames)`, wenn die Datei eine Animation mit mehr als einem Bild ist.
fn decode_animation(path: &Path) -> ImageResult<Option<Vec<(RgbaImage, Duration)>>> {
    let format = ImageReader::open(path)?.with_guessed_format()?.format();
    let open = || -> ImageResult<BufReader<File>> { Ok(BufReader::new(File::open(path)?)) };

    let frames = match format {
        Some(ImageFormat::Gif) => GifDecoder::new(open()?)?.into_frames(),
        Some(ImageFormat::WebP) => {
            let decoder = WebPDecoder::new(open()?)?;
            if !decoder.has_animation() {
                return Ok(None);
            }
            decoder.into_frames()
        }
        Some(ImageFormat::Png) => {
            let decoder = PngDecoder::new(open()?)?;
            if !decoder.is_apng()? {
                return Ok(None);
            }
            decoder.apng()?.into_frames()
        }
        _ => return Ok(None),
    };

    let mut result = Vec::new();
    let mut bytes = 0usize;
    for frame in frames {
        let frame = frame?;
        let (num, den) = frame.delay().numer_denom_ms();
        let mut delay = Duration::from_secs_f64(num as f64 / den.max(1) as f64 / 1000.0);
        // Wie Browser: sehr kurze Verzögerungen (0–10 ms) als 100 ms behandeln.
        if delay < Duration::from_millis(11) {
            delay = Duration::from_millis(100);
        }
        let buffer = frame.into_buffer();
        bytes += buffer.as_raw().len();
        result.push((buffer, delay));
        if bytes > MAX_ANIMATION_BYTES {
            break;
        }
    }

    Ok(if result.len() > 1 { Some(result) } else { None })
}

fn decode_still(path: &Path) -> ImageResult<DynamicImage> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    reader.no_limits();

    // Bei eingebauten Formaten (JPEG, PNG, WebP, TIFF …) die EXIF-Ausrichtung anwenden.
    // libheif und jxl-oxide liefern das Bild bereits korrekt gedreht.
    let builtin = matches!(reader.format(), Some(f) if f != ImageFormat::Avif);

    let mut decoder = reader.into_decoder()?;
    let orientation = if builtin { decoder.orientation().ok() } else { None };
    let mut img = DynamicImage::from_decoder(decoder)?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dekodiert alle Bilder aus `$SIV_TEST_DIR` (falls gesetzt).
    #[test]
    fn decode_test_dir() {
        let Ok(dir) = std::env::var("SIV_TEST_DIR") else { return };
        register_decoders();
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let loaded = load(&path, 8192).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            println!(
                "{}: {:?}, {} Bild(er), {:?}",
                path.display(),
                loaded.original_size,
                loaded.frames.len(),
                loaded.frames.iter().map(|f| f.1).sum::<Duration>()
            );
        }
    }
}
