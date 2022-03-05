use std::{fs::{self, OpenOptions}, path::Path};

use image::{codecs::jpeg::JpegEncoder, ColorType};
use mupdf::{pdf::PdfDocument, Colorspace, Matrix, TextPageOptions};
use search_index::{SearchResult, SearchIndex, Match};

fn process_lesson_pdf<P: AsRef<Path>>(path: P, index: &mut SearchIndex) {
    let doc = PdfDocument::open(path.as_ref().to_str().unwrap()).unwrap();
    for page in doc.pages().unwrap() {
        let page = page.unwrap();
        let text_page = page.to_text_page(TextPageOptions::empty()).unwrap();
        for b in text_page.blocks() {
            for l in b.lines() {
                let bounds = l.bounds();
                let line: String = l.chars()
                    .flat_map(|c| c.char())
                    .collect();
                let words: Vec<String> = deunicode::deunicode(&line)
                    .to_ascii_lowercase()
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .filter(|w| w.len() > 2)
                    .map(|w| w.to_owned())
                    .collect();
                if words.len() < 2 || words[0] != "theoreme" {
                    continue;
                }
                let result_index = index.results.len();
                index.results.push(SearchResult {
                    image_index: index.image_ids.len() as u32,
                    x: bounds.x0 as i16,
                    y: bounds.y0 as i16,
                    width: (bounds.x1 - bounds.x0) as u16,
                    height: (bounds.y1 - bounds.y0) as u16,
                });
                for w in words.iter() {
                    index.words.entry(w.to_string())
                        .or_default()
                        .push(Match {
                            result_index: result_index as u32,
                            score: 1. / words.len() as f32,
                        });
                }
            }
        }
        // TODO: fix the `alpha` parameter not being a boolean
        let pixmap = page.to_pixmap(&Matrix::new_scale(3., 3.), &Colorspace::device_rgb(), 0., false).unwrap();
        let mut encoded = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut encoded, 50);
        encoder.encode(pixmap.samples(), pixmap.width(), pixmap.height(), ColorType::Rgb8).unwrap();
        let digest = blake3::hash(&encoded);
        let id = base64::encode_config(digest.as_bytes(), base64::URL_SAFE_NO_PAD);
        index.image_ids.push(id.clone());
        fs::write(format!("index/images/{}.jpg", id), encoded).unwrap();
    }
}

fn main() {
    fs::create_dir_all("index/images").unwrap();
    let mut index = SearchIndex::new();
    for entry in fs::read_dir("lessons").unwrap() {
        let entry = entry.unwrap();
        if entry.metadata().unwrap().is_file() {
            process_lesson_pdf(entry.path(), &mut index);
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open("index/index.bin")
        .unwrap();
    index.serialize(&mut file).unwrap();
}
