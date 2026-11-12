//! Einfacher Bildbetrachter.
//!
//! Tasten:
//!   ←/→, Bild↑/Bild↓   vorheriges / nächstes Bild im selben Ordner
//!   Pos1 / Ende        erstes / letztes Bild
//!   Mausrad            Zoom (um den Mauszeiger)
//!   +/-                Zoom (um die Fenstermitte)
//!   1                  ursprünglicher Zoom (100 % bzw. eingepasst)
//!   Leertaste          Animation anhalten / fortsetzen
//!   Linke Maustaste    Bild verschieben
//!   Rechte Maustaste / Umschalt + linke Maustaste ziehen
//!                      Bereich auswählen, Text erkennen (OCR) und in die Zwischenablage kopieren
//!   F11 / F            Vollbild umschalten
//!   Esc / Q            Beenden (bzw. OCR-Auswahl abbrechen)

mod loader;
mod ocr;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, Event, Key, MouseWheelUnit, PointerButton, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextureHandle, Vec2, ViewportCommand, pos2, vec2,
};

const APP_ID: &str = "simple-image-viewer";
const BACKGROUND: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);
const MIN_SCALE: f32 = 0.01;
const MAX_SCALE: f32 = 64.0;
/// Zoomfaktor pro Mausrad-Raste.
const WHEEL_STEP: f32 = 1.2;
/// Anzeigedauer von Hinweisen (z. B. "Text kopiert").
const NOTICE_DURATION: Duration = Duration::from_secs(3);
const SELECTION_COLOR: Color32 = Color32::from_rgb(0x3d, 0xae, 0xe9);

fn main() -> eframe::Result {
    loader::register_decoders();

    let arg = std::env::args_os().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bildbetrachter")
            .with_app_id(APP_ID)
            .with_maximized(true)
            .with_min_inner_size([200.0, 150.0])
            .with_icon(window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| Ok(Box::new(Viewer::new(&cc.egui_ctx, arg)))),
    )
}

/// Fenstersymbol (unter Wayland nimmt KDE stattdessen das Symbol aus der .desktop-Datei).
fn window_icon() -> egui::IconData {
    let png = include_bytes!("../assets/simple-image-viewer.png");
    let img = image::load_from_memory(png).expect("eingebettetes Icon").into_rgba8();
    egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
}

/// Ergebnis eines Ladevorgangs.
enum Slot {
    Loading,
    Ready {
        /// Einzelbilder mit Anzeigedauer (Standbild: genau eins).
        frames: Vec<(TextureHandle, Duration)>,
        /// Originalgröße in Bildpixeln (die Textur kann bei sehr großen Bildern kleiner sein).
        size: [usize; 2],
    },
    Failed(String),
}

struct Viewer {
    files: Vec<PathBuf>,
    index: usize,
    cache: HashMap<PathBuf, Slot>,
    tx: Sender<(PathBuf, Result<loader::Loaded, String>)>,
    rx: Receiver<(PathBuf, Result<loader::Loaded, String>)>,
    /// `None` = automatischer Zoom (100 % oder eingepasst).
    /// `Some(s)` = manueller Zoom, `s` = physische Pixel pro Bildpixel.
    zoom: Option<f32>,
    /// Verschiebung der Bildmitte gegenüber der Fenstermitte (in Punkten).
    offset: Vec2,
    title: String,
    fullscreen: bool,
    message: Option<String>,
    /// Startzeitpunkt der Animation des aktuellen Bildes.
    anim_start: Instant,
    /// Animation angehalten (Leertaste): angezeigtes Einzelbild.
    paused_at: Option<Duration>,
    /// OCR-Auswahl in Bildschirmkoordinaten (Startpunkt, aktueller Punkt).
    selection: Option<(Pos2, Pos2)>,
    /// Laufende Texterkennung.
    ocr: Option<Receiver<Result<String, String>>>,
    /// Kurzer Hinweis am unteren Fensterrand und Zeitpunkt seines Erscheinens.
    notice: Option<(String, Instant)>,
}

impl Viewer {
    fn new(ctx: &egui::Context, arg: Option<PathBuf>) -> Self {
        let (tx, rx) = channel();
        let mut viewer = Self {
            files: Vec::new(),
            index: 0,
            cache: HashMap::new(),
            tx,
            rx,
            zoom: None,
            offset: Vec2::ZERO,
            title: String::new(),
            fullscreen: false,
            message: None,
            anim_start: Instant::now(),
            paused_at: None,
            selection: None,
            ocr: None,
            notice: None,
        };

        match arg {
            None => viewer.message = Some("Keine Datei angegeben.".into()),
            Some(path) => {
                let path = path.canonicalize().unwrap_or(path);
                if path.is_dir() {
                    viewer.files = list_images(&path);
                    if viewer.files.is_empty() {
                        viewer.message = Some(format!("Keine Bilder in {}", path.display()));
                    }
                } else {
                    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                    let mut files = list_images(&dir);
                    // Datei auch anzeigen, wenn sie eine unbekannte Endung hat.
                    if !files.contains(&path) {
                        files.push(path.clone());
                        files.sort_by(|a, b| natural_cmp(a, b));
                    }
                    viewer.index = files.iter().position(|p| *p == path).unwrap_or(0);
                    viewer.files = files;
                }
            }
        }

        viewer.request_around(ctx);
        viewer
    }

    fn current_path(&self) -> Option<&PathBuf> {
        self.files.get(self.index)
    }

    /// Startet das Laden des aktuellen Bildes und seiner Nachbarn,
    /// verwirft alles andere aus dem Cache.
    fn request_around(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() {
            return;
        }
        let n = self.files.len();
        let wanted: Vec<PathBuf> = [self.index, (self.index + 1) % n, (self.index + n - 1) % n]
            .into_iter()
            .map(|i| self.files[i].clone())
            .collect();

        let keep: HashSet<&PathBuf> = wanted.iter().collect();
        self.cache.retain(|p, _| keep.contains(p));

        for path in wanted {
            if self.cache.contains_key(&path) {
                continue;
            }
            self.cache.insert(path.clone(), Slot::Loading);
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let max_side = ctx.input(|i| i.max_texture_side);
                let result = loader::load(&path, max_side);
                let _ = tx.send((path, result));
                ctx.request_repaint();
            });
        }
    }

    fn go_to(&mut self, ctx: &egui::Context, index: usize) {
        if self.files.is_empty() || index == self.index {
            return;
        }
        self.index = index;
        self.selection = None;
        self.reset_view();
        self.restart_animation();
        self.request_around(ctx);
    }

    fn step(&mut self, ctx: &egui::Context, forward: bool) {
        let n = self.files.len();
        if n == 0 {
            return;
        }
        let next = if forward { (self.index + 1) % n } else { (self.index + n - 1) % n };
        self.go_to(ctx, next);
    }

    fn reset_view(&mut self) {
        self.zoom = None;
        self.offset = Vec2::ZERO;
    }

    fn restart_animation(&mut self) {
        self.anim_start = Instant::now();
        self.paused_at = None;
    }

    fn receive_loaded(&mut self, ctx: &egui::Context) {
        while let Ok((path, result)) = self.rx.try_recv() {
            // Nur übernehmen, wenn das Bild noch gebraucht wird.
            let Some(slot) = self.cache.get_mut(&path) else { continue };
            *slot = match result {
                Ok(loaded) => {
                    let options = egui::TextureOptions::LINEAR
                        .with_mipmap_mode(Some(egui::TextureFilter::Linear));
                    let name = path.to_string_lossy();
                    let frames = loaded
                        .frames
                        .into_iter()
                        .enumerate()
                        .map(|(i, (image, delay))| {
                            (ctx.load_texture(format!("{name}#{i}"), image, options), delay)
                        })
                        .collect();
                    Slot::Ready { frames, size: loaded.original_size }
                }
                Err(e) => Slot::Failed(e),
            };
            // Animation beginnt, sobald das aktuelle Bild fertig geladen ist.
            if self.current_path() == Some(&path) {
                self.restart_animation();
            }
        }
    }

    /// Startet die Texterkennung für einen Ausschnitt `[x, y, breite, höhe]` des aktuellen Bildes.
    fn start_ocr(&mut self, ctx: &egui::Context, region: [u32; 4]) {
        let Some(path) = self.current_path().cloned() else { return };
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(ocr::recognize(&path, region));
            ctx.request_repaint();
        });
        self.ocr = Some(rx);
    }

    /// Übernimmt das Ergebnis der Texterkennung und kopiert den Text in die Zwischenablage.
    fn receive_ocr(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.ocr else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Texterkennung abgebrochen".into()),
        };
        self.ocr = None;
        let notice = match result {
            Ok(text) if text.is_empty() => "Kein Text erkannt".to_string(),
            Ok(text) => {
                let chars = text.chars().count();
                let lines = text.lines().count();
                ctx.copy_text(text);
                let plural = if lines == 1 { "" } else { "n" };
                format!("Text kopiert ({chars} Zeichen, {lines} Zeile{plural})")
            }
            Err(e) => format!("Texterkennung fehlgeschlagen: {e}"),
        };
        self.notice = Some((notice, Instant::now()));
    }

    /// Hinweis am unteren Fensterrand (OCR läuft bzw. Ergebnis).
    fn draw_notice(&mut self, ctx: &egui::Context, painter: &egui::Painter, area: Rect) {
        let text = if self.ocr.is_some() {
            "Texterkennung läuft …".to_string()
        } else {
            let Some((text, since)) = &self.notice else { return };
            let elapsed = since.elapsed();
            if elapsed >= NOTICE_DURATION {
                self.notice = None;
                return;
            }
            ctx.request_repaint_after(NOTICE_DURATION - elapsed);
            text.clone()
        };
        let font = egui::FontId::proportional(15.0);
        let galley = painter.layout_no_wrap(text, font, Color32::from_gray(230));
        let pad = vec2(14.0, 8.0);
        let size = galley.size() + pad * 2.0;
        let center = pos2(area.center().x, area.bottom() - 30.0 - size.y / 2.0);
        let rect = Rect::from_center_size(center, size);
        painter.rect_filled(rect, 6.0, Color32::from_black_alpha(200));
        painter.galley(rect.min + pad, galley, Color32::WHITE);
    }

    fn update_title(&mut self, ctx: &egui::Context, scale: Option<f32>) {
        let title = match self.current_path() {
            None => "Bildbetrachter".to_string(),
            Some(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                let mut t = format!("{name}  ({}/{})", self.index + 1, self.files.len());
                if let Some(Slot::Ready { size, frames }) = self.cache.get(path) {
                    t += &format!("  –  {}×{}", size[0], size[1]);
                    if frames.len() > 1 {
                        t += &format!("  –  Animation ({} Bilder)", frames.len());
                        if self.paused_at.is_some() {
                            t += ", angehalten";
                        }
                    }
                }
                if let Some(s) = scale {
                    t += &format!("  –  {:.0} %", s * 100.0);
                }
                t + "  –  Bildbetrachter"
            }
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl eframe::App for Viewer {
    /// Läuft auch, wenn das Fenster verdeckt ist: hält den Animations-Takt am Leben,
    /// damit die Animation beim Wiederauftauchen sofort weiterläuft.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.paused_at.is_some() {
            return;
        }
        if let Some(Slot::Ready { frames, .. }) = self.current_path().and_then(|p| self.cache.get(p))
            && let (_, Some(wait)) = current_frame(frames, self.anim_start.elapsed())
        {
            ctx.request_repaint_after(wait);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive_loaded(&ctx);
        self.receive_ocr(&ctx);

        // --- Tastatur -------------------------------------------------------
        let (next, prev, first, last, reset, zoom_in, zoom_out, fullscreen, quit, pause) =
            ctx.input(|i| {
                (
                    i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::PageDown),
                    i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::PageUp),
                    i.key_pressed(Key::Home),
                    i.key_pressed(Key::End),
                    i.key_pressed(Key::Num1),
                    i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals),
                    i.key_pressed(Key::Minus),
                    i.key_pressed(Key::F11) || i.key_pressed(Key::F),
                    i.key_pressed(Key::Escape) || i.key_pressed(Key::Q),
                    i.key_pressed(Key::Space),
                )
            });

        if quit {
            if self.selection.is_some() {
                self.selection = None;
            } else if self.fullscreen {
                self.fullscreen = false;
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
            } else {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
        if fullscreen {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
        }
        if next {
            self.step(&ctx, true);
        } else if prev {
            self.step(&ctx, false);
        } else if first {
            self.go_to(&ctx, 0);
        } else if last && !self.files.is_empty() {
            self.go_to(&ctx, self.files.len() - 1);
        }
        if reset {
            self.reset_view();
        }
        if pause {
            match self.paused_at.take() {
                Some(elapsed) => self.anim_start = Instant::now() - elapsed,
                None => self.paused_at = Some(self.anim_start.elapsed()),
            }
        }

        // --- Zeichenfläche --------------------------------------------------
        let area = ui.max_rect();
        let response = ui.allocate_rect(area, Sense::click_and_drag());
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, BACKGROUND);

        let ppp = ctx.pixels_per_point();
        let center = area.center();

        let slot = self.current_path().and_then(|p| self.cache.get(p));
        let mut shown_scale = None;
        let mut ocr_region = None;

        match slot {
            Some(Slot::Ready { frames, size }) => {
                let img = vec2(size[0] as f32, size[1] as f32);

                // Automatischer Zoom: 100 % (1 Bildpixel = 1 Bildschirmpixel),
                // bei zu großen Bildern eingepasst.
                let fit = (area.width() * ppp / img.x)
                    .min(area.height() * ppp / img.y)
                    .min(1.0);
                let mut scale = self.zoom.unwrap_or(fit);

                // Zoom per Mausrad (um den Mauszeiger) bzw. +/- (um die Mitte).
                let mut factor = 1.0_f32;
                let mut anchor = center;
                if response.hovered() {
                    ctx.input(|i| {
                        for ev in &i.events {
                            if let Event::MouseWheel { unit, delta, .. } = ev {
                                let notches = match unit {
                                    MouseWheelUnit::Line => delta.y,
                                    MouseWheelUnit::Point => delta.y / 50.0,
                                    MouseWheelUnit::Page => delta.y * 3.0,
                                };
                                factor *= WHEEL_STEP.powf(notches);
                            }
                        }
                        if let Some(p) = i.pointer.hover_pos() {
                            anchor = p;
                        }
                    });
                }
                if zoom_in {
                    factor *= WHEEL_STEP;
                    anchor = center;
                }
                if zoom_out {
                    factor /= WHEEL_STEP;
                    anchor = center;
                }
                if factor != 1.0 {
                    let new_scale = (scale * factor).clamp(MIN_SCALE, MAX_SCALE);
                    // Punkt unter dem Anker bleibt an seiner Stelle.
                    let rel = anchor - center;
                    self.offset = rel - (rel - self.offset) * (new_scale / scale);
                    scale = new_scale;
                    self.zoom = Some(scale);
                }

                // Rechte Maustaste bzw. Umschalt + linke Maustaste ziehen: OCR-Bereich
                // auswählen; sonst linke Maustaste ziehen: Bild verschieben.
                let shift = ctx.input(|i| i.modifiers.shift);
                if self.selection.is_none()
                    && (response.drag_started_by(PointerButton::Secondary)
                        || (shift && response.drag_started_by(PointerButton::Primary)))
                    && let Some(origin) = ctx.input(|i| i.pointer.press_origin())
                {
                    self.selection = Some((origin, origin));
                }
                if let Some((_, end)) = &mut self.selection {
                    if let Some(p) = response.interact_pointer_pos() {
                        *end = p;
                    }
                } else if response.dragged_by(PointerButton::Primary) {
                    self.offset += response.drag_delta();
                }

                let size_pts = img * scale / ppp;
                self.offset = clamp_offset(self.offset, size_pts, area.size());

                let mut min = center + self.offset - size_pts / 2.0;
                // Auf ganze Bildschirmpixel ausrichten -> scharfe Darstellung bei 100 %.
                min = pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
                let rect = Rect::from_min_size(min, size_pts);

                let elapsed = self.paused_at.unwrap_or_else(|| self.anim_start.elapsed());
                let (texture, wait) = current_frame(frames, elapsed);
                if let Some(wait) = wait.filter(|_| self.paused_at.is_none()) {
                    ctx.request_repaint_after(wait);
                }

                painter.image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                shown_scale = Some(scale);

                if let Some((start, end)) = self.selection {
                    let sel = Rect::from_two_pos(start, end).intersect(rect);
                    if sel.is_positive() {
                        painter.rect_filled(sel, 0.0, SELECTION_COLOR.gamma_multiply(0.2));
                        let stroke = Stroke::new(1.0, SELECTION_COLOR);
                        painter.rect_stroke(sel, 0.0, stroke, StrokeKind::Inside);
                    }
                    if response.drag_stopped() {
                        self.selection = None;
                        // Bildschirmpunkte in Bildpixel umrechnen.
                        let min = (sel.min - rect.min) / scale * ppp;
                        let size = sel.size() / scale * ppp;
                        if sel.is_positive() && size.x >= 4.0 && size.y >= 4.0 && self.ocr.is_none()
                        {
                            let [x, y] = [min.x, min.y].map(|v| v.round().max(0.0) as u32);
                            let [w, h] = [size.x, size.y].map(|v| v.round() as u32);
                            ocr_region = Some([x, y, w, h]);
                        }
                    }
                }
            }
            Some(Slot::Failed(err)) => {
                let name = self
                    .current_path()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                centered_text(
                    &painter,
                    center,
                    &format!("{name} kann nicht geöffnet werden:\n{err}"),
                );
            }
            Some(Slot::Loading) => {
                // Kurz leer lassen statt zu flackern; Text erst nach etwas Zeit wäre
                // aufwendiger – ein dezenter Hinweis genügt.
                centered_text(&painter, center, "Lade …");
            }
            None => {
                if let Some(msg) = &self.message {
                    centered_text(
                        &painter,
                        center,
                        &format!("{msg}\n\nAufruf: simple-image-viewer <Bild oder Ordner>"),
                    );
                }
            }
        }

        // Doppelklick: Vollbild umschalten.
        if response.double_clicked() {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
        }

        if !matches!(slot, Some(Slot::Ready { .. })) {
            self.selection = None;
        }
        if let Some(region) = ocr_region {
            self.start_ocr(&ctx, region);
        }
        self.draw_notice(&ctx, &painter, area);
        self.update_title(&ctx, shown_scale);
    }
}

/// Wählt das zur verstrichenen Zeit passende Einzelbild einer (Endlos-)Animation.
/// Liefert außerdem die Zeit bis zum nächsten Bildwechsel (`None` bei Standbildern).
fn current_frame<T>(frames: &[(T, Duration)], elapsed: Duration) -> (&T, Option<Duration>) {
    let total: Duration = frames.iter().map(|f| f.1).sum();
    if frames.len() < 2 || total.is_zero() {
        return (&frames[0].0, None);
    }
    let mut t = Duration::from_nanos((elapsed.as_nanos() % total.as_nanos()) as u64);
    for (frame, delay) in frames {
        if t < *delay {
            return (frame, Some(*delay - t));
        }
        t -= *delay;
    }
    (&frames[0].0, Some(frames[0].1))
}

/// Verhindert, dass das Bild aus dem Fenster geschoben wird.
fn clamp_offset(offset: Vec2, image: Vec2, view: Vec2) -> Vec2 {
    let limit = |img: f32, view: f32, off: f32| {
        if img <= view {
            0.0
        } else {
            let max = (img - view) / 2.0;
            off.clamp(-max, max)
        }
    };
    vec2(limit(image.x, view.x, offset.x), limit(image.y, view.y, offset.y))
}

fn centered_text(painter: &egui::Painter, pos: Pos2, text: &str) {
    painter.text(
        pos,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(16.0),
        Color32::from_gray(180),
    );
}

/// Alle unterstützten Bilddateien eines Ordners, natürlich sortiert.
fn list_images(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && loader::is_supported(p))
        .collect();
    files.sort_by(|a, b| natural_cmp(a, b));
    files
}

/// Natürliche Sortierung: "bild2" vor "bild10", Groß-/Kleinschreibung egal.
fn natural_cmp(a: &Path, b: &Path) -> std::cmp::Ordering {
    let a = a.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let b = b.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(&b),
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (na, nb) = (take(&mut ai), take(&mut bi));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort() {
        let mut v: Vec<PathBuf> = ["b10.jpg", "B2.jpg", "a.jpg", "b1.jpg"]
            .iter()
            .map(PathBuf::from)
            .collect();
        v.sort_by(|a, b| natural_cmp(a, b));
        let names: Vec<_> = v.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(names, ["a.jpg", "b1.jpg", "B2.jpg", "b10.jpg"]);
    }

    #[test]
    fn animation_frame_selection() {
        let ms = Duration::from_millis;
        let frames = [("a", ms(100)), ("b", ms(50)), ("c", ms(100))];
        assert_eq!(current_frame(&frames, ms(0)), (&"a", Some(ms(100))));
        assert_eq!(current_frame(&frames, ms(120)), (&"b", Some(ms(30))));
        assert_eq!(current_frame(&frames, ms(150)), (&"c", Some(ms(100))));
        assert_eq!(current_frame(&frames, ms(260)), (&"a", Some(ms(90))));
        assert_eq!(current_frame(&[("x", Duration::ZERO)], ms(5)), (&"x", None));
    }

    #[test]
    fn offset_is_clamped() {
        let o = clamp_offset(vec2(500.0, 500.0), vec2(1000.0, 100.0), vec2(800.0, 600.0));
        assert_eq!(o, vec2(100.0, 0.0));
    }
}
