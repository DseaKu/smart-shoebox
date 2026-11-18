//! Built-in OCR: `hayro` renders a page to an image, Tesseract (compiled into the
//! program) reads the text. No external program is needed.
//!
//! The German and English language data are part of the program. Other languages
//! work if their `<lang>.traineddata` file is in the folder from [`data_dir`].

use crate::Res;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use kreuzberg_tesseract::TesseractAPI;
use std::fs;
use std::path::{Path, PathBuf};

/// Language data that is part of the program (Tesseract "fast" models, Apache 2.0).
const BUILT_IN: [(&str, &[u8]); 2] = [
    ("deu", include_bytes!("../assets/tessdata/deu.traineddata")),
    ("eng", include_bytes!("../assets/tessdata/eng.traineddata")),
];

/// Pages are rendered at this resolution, which suits Tesseract best.
const DPI: f32 = 300.0;
/// Upper limit for the longer side of a rendered page, for very large pages.
const MAX_SIDE: f32 = 5000.0;
/// A page read upright with less confidence than this (0-100) is also tried in
/// the other three orientations.
const UPRIGHT_CONFIDENCE: i32 = 60;
/// Another orientation must be this much more confident to replace upright.
const TURN_MARGIN: i32 = 10;

/// The OCR result of one page.
pub struct PageText {
    /// Page number, starting at 1.
    pub page: usize,
    pub text: String,
    /// Clockwise turn (0, 90, 180 or 270 degrees) that makes the page upright.
    pub turn: u16,
}

/// The folder for Tesseract language data. The built-in languages are written
/// there on first use; other `<lang>.traineddata` files can be added by hand.
pub fn data_dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join("shoebox-test-tessdata");
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Smart Shoebox")
        .join("tessdata")
}

/// Reads the blank pages of `texts` (one text per page of `path`) with OCR and
/// puts the results in their place. Returns the pages that are not upright,
/// with the clockwise turn that fixes them.
pub fn fill_blank_pages(
    path: &Path,
    language: &str,
    texts: &mut [String],
) -> Res<Vec<(usize, u16)>> {
    let blank: Vec<usize> = (1..=texts.len())
        .filter(|&n| crate::pages::is_blank(&texts[n - 1]))
        .collect();
    if blank.is_empty() {
        return Ok(Vec::new());
    }
    let mut turns = Vec::new();
    for page in read_pages(path, language, &blank)? {
        texts[page.page - 1] = page.text;
        if page.turn != 0 {
            turns.push((page.page, page.turn));
        }
    }
    Ok(turns)
}

/// Reads the given pages (numbers start at 1) of a PDF with OCR.
pub fn read_pages(path: &Path, language: &str, pages: &[usize]) -> Res<Vec<PageText>> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let engine = engine(language)?;
    crate::pdf::guarded(path, || -> Res<Vec<PageText>> {
        let pdf =
            Pdf::new(bytes).map_err(|e| format!("the PDF cannot be rendered for OCR ({e:?})"))?;
        let all = pdf.pages();
        let cache = RenderCache::new();
        let settings = InterpreterSettings::default();
        let mut result = Vec::new();
        for &number in pages {
            let page = all
                .get(number.wrapping_sub(1))
                .ok_or_else(|| format!("the PDF has no page {number}"))?;
            let (width, height) = page.render_dimensions();
            let scale = (DPI / 72.0).min(MAX_SIDE / width.max(height).max(1.0));
            let pixmap = render(
                page,
                &cache,
                &settings,
                &RenderSettings::default(),
                &PixmapSettings {
                    x_scale: scale,
                    y_scale: scale,
                    bg_color: WHITE,
                },
            );
            let image = Gray::from_rgba(
                pixmap.width().into(),
                pixmap.height().into(),
                pixmap.data_as_u8_slice(),
            );
            let (text, turn) = read_upright(&engine, &image)?;
            result.push(PageText {
                page: number,
                text,
                turn,
            });
        }
        Ok(result)
    })
}

/// A Tesseract engine for `language` ("deu", "deu+eng", ...).
fn engine(language: &str) -> Res<TesseractAPI> {
    let dir = data_dir();
    install_built_in(&dir)?;
    for lang in language.split('+') {
        if !dir.join(format!("{lang}.traineddata")).is_file() {
            return Err(format!(
                "OCR language \"{lang}\" is not available. Built in: deu, eng. \
                 For others, put {lang}.traineddata into {}",
                dir.display()
            )
            .into());
        }
    }
    let api = TesseractAPI::new().map_err(|e| format!("cannot start OCR: {e}"))?;
    api.init(&dir, language)
        .map_err(|e| format!("cannot start OCR for \"{language}\": {e}"))?;
    // The resolution is known, and Tesseract's own messages would only clutter
    // the output.
    let quiet = if cfg!(windows) { "NUL" } else { "/dev/null" };
    for (name, value) in [("user_defined_dpi", "300"), ("debug_file", quiet)] {
        api.set_variable(name, value)
            .map_err(|e| format!("cannot set up OCR ({name}): {e}"))?;
    }
    Ok(api)
}

/// Writes the built-in language files to `dir` if they are missing or differ.
/// Writes a temporary file first, so a second program running at the same time
/// never reads half a file.
fn install_built_in(dir: &Path) -> Res<()> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    for (lang, data) in BUILT_IN {
        let file = dir.join(format!("{lang}.traineddata"));
        if fs::metadata(&file).is_ok_and(|m| m.len() == data.len() as u64) {
            continue;
        }
        let tmp = dir.join(format!("{lang}.traineddata.{}.part", std::process::id()));
        fs::write(&tmp, data).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &file).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    Ok(())
}

/// Reads the page upright. If Tesseract is not confident, the page may be
/// scanned sideways or upside down: the other orientations are compared on a
/// half-size image (fast), and the best one is read in full.
fn read_upright(engine: &TesseractAPI, image: &Gray) -> Res<(String, u16)> {
    let (text, confidence) = recognize(engine, image)?;
    if confidence >= UPRIGHT_CONFIDENCE || text.trim().is_empty() {
        return Ok((text, 0));
    }
    let small = image.half();
    let mut upright = 0;
    let mut best = (0, i32::MIN);
    for turn in [0, 90, 180, 270] {
        let (_, confidence) = recognize(engine, &small.turned(turn))?;
        if turn == 0 {
            upright = confidence;
        }
        if confidence > best.1 {
            best = (turn, confidence);
        }
    }
    if best.0 == 0 || best.1 < upright + TURN_MARGIN {
        return Ok((text, 0));
    }
    let (turned_text, _) = recognize(engine, &image.turned(best.0))?;
    Ok((turned_text, best.0))
}

/// The text and the mean word confidence (0-100) of one image.
fn recognize(engine: &TesseractAPI, image: &Gray) -> Res<(String, i32)> {
    let (width, height) = (image.width as i32, image.height as i32);
    engine
        .set_image(&image.pixels, width, height, 1, width)
        .map_err(|e| format!("OCR failed: {e}"))?;
    let text = engine
        .get_utf8_text()
        .map_err(|e| format!("OCR failed: {e}"))?;
    let confidence = engine.mean_text_conf().unwrap_or(0);
    Ok((text, confidence))
}

/// An 8-bit grayscale image, row by row.
#[derive(Clone)]
struct Gray {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Gray {
    /// From RGBA pixels on a white background.
    fn from_rgba(width: usize, height: usize, rgba: &[u8]) -> Self {
        let pixels = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8)
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Half the width and height; each pixel is the mean of a 2×2 block.
    fn half(&self) -> Self {
        let (width, height) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let at = |x: usize, y: usize| {
            self.pixels[y.min(self.height - 1) * self.width + x.min(self.width - 1)] as u32
        };
        let mut pixels = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let sum = at(2 * x, 2 * y)
                    + at(2 * x + 1, 2 * y)
                    + at(2 * x, 2 * y + 1)
                    + at(2 * x + 1, 2 * y + 1);
                pixels.push((sum / 4) as u8);
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// The image turned clockwise by `degrees` (0, 90, 180 or 270).
    fn turned(&self, degrees: u16) -> Self {
        let (w, h) = (self.width, self.height);
        let mut pixels = vec![0; w * h];
        match degrees {
            90 => {
                for y in 0..h {
                    for x in 0..w {
                        pixels[x * h + (h - 1 - y)] = self.pixels[y * w + x];
                    }
                }
            }
            180 => pixels = self.pixels.iter().rev().copied().collect(),
            270 => {
                for y in 0..h {
                    for x in 0..w {
                        pixels[(w - 1 - x) * h + y] = self.pixels[y * w + x];
                    }
                }
            }
            _ => return self.clone(),
        }
        let (width, height) = if degrees == 180 { (w, h) } else { (h, w) };
        Self {
            width,
            height,
            pixels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TestBox, pdf};
    use lopdf::{Document, Object, Stream, dictionary};

    /// Renders a text PDF to page images and writes them as an image-only PDF,
    /// like a scanner does. The pages in `upside_down` (1-based) are turned 180°.
    fn scan_pdf(path: &Path, pages: &[&str], upside_down: &[usize]) {
        let text_pdf = path.with_extension("text.pdf");
        pdf(&text_pdf, pages);
        let rendered = Pdf::new(fs::read(&text_pdf).unwrap()).unwrap();
        let cache = RenderCache::new();
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids: Vec<Object> = Vec::new();
        for (i, page) in rendered.pages().iter().enumerate() {
            let scale = 200.0 / 72.0;
            let pixmap = render(
                page,
                &cache,
                &InterpreterSettings::default(),
                &RenderSettings::default(),
                &PixmapSettings {
                    x_scale: scale,
                    y_scale: scale,
                    bg_color: WHITE,
                },
            );
            let mut image = Gray::from_rgba(
                pixmap.width().into(),
                pixmap.height().into(),
                pixmap.data_as_u8_slice(),
            );
            if upside_down.contains(&(i + 1)) {
                image = image.turned(180);
            }
            let mut stream = Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image",
                    "Width" => image.width as i64, "Height" => image.height as i64,
                    "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                },
                image.pixels,
            );
            stream.compress().unwrap();
            let image_id = doc.add_object(stream);
            let content = b"q 595 0 0 842 0 0 cm /Im1 Do Q".to_vec();
            let content_id = doc.add_object(Stream::new(dictionary! {}, content));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
                "Resources" => dictionary! { "XObject" => dictionary! { "Im1" => image_id } },
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            });
            kids.push(page_id.into());
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => kids.clone(), "Count" => kids.len() as i64 },
            ),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc.save(path).unwrap();
        let _ = fs::remove_file(text_pdf);
    }

    #[test]
    fn reads_a_scan_and_turns_upside_down_pages() {
        let t = TestBox::new("ocr-scan");
        let file = t.dir.join("scan.pdf");
        let letter = "Stadtwerke Musterstadt GmbH\nRechnung fuer Strom und Wasser\n\
                      Sehr geehrte Damen und Herren,\nbitte zahlen Sie den Betrag\n\
                      bis zum Ende des Monats auf unser Konto.";
        let contract = "Mietvertrag fuer die Wohnung\nDie Kaution betraegt drei Monatsmieten.\n\
                        Der Vermieter erhaelt die Miete jeden Monat\nim Voraus auf sein Konto.";
        scan_pdf(&file, &[letter, contract], &[2]);
        // Really a scan: no page has a text layer.
        assert!(
            crate::pdf::text_by_pages(&file)
                .unwrap()
                .iter()
                .all(|t| crate::pages::is_blank(t))
        );

        let texts = crate::pagetext::read_pdf(&file, "deu").unwrap();
        assert!(texts[0].contains("Stadtwerke"), "{}", texts[0]);
        assert!(texts[1].contains("Kaution"), "{}", texts[1]);

        let scan = crate::scan::Scan::read(&file, "deu").unwrap();
        assert_ne!(scan.source(), file.as_path());
        let doc = Document::load(scan.source()).unwrap();
        let rotate: Vec<i64> = doc
            .get_pages()
            .values()
            .map(|id| {
                let page = doc.get_dictionary(*id).unwrap();
                page.get(b"Rotate").and_then(Object::as_i64).unwrap_or(0)
            })
            .collect();
        assert_eq!(rotate, [0, 180]);
    }

    #[test]
    fn turns_clockwise() {
        // 2×1 image: [a b] turned 90° clockwise is a column [a; b].
        let image = Gray {
            width: 2,
            height: 1,
            pixels: vec![1, 2],
        };
        let r = image.turned(90);
        assert_eq!((r.width, r.height, r.pixels.clone()), (1, 2, vec![1, 2]));
        assert_eq!(image.turned(180).pixels, vec![2, 1]);
        let l = image.turned(270);
        assert_eq!((l.width, l.height, l.pixels), (1, 2, vec![2, 1]));
        assert_eq!(r.turned(270).pixels, image.pixels);
    }

    #[test]
    fn unknown_language_is_a_clear_error() {
        let err = engine("xyz").err().unwrap().to_string();
        assert!(err.contains("xyz.traineddata"), "{err}");
    }
}
