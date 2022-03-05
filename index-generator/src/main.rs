use std::{fs::{self, OpenOptions}, path::Path, collections::HashMap, io::{Write, self}};

use image::{codecs::jpeg::JpegEncoder, ColorType};
use mupdf::{pdf::{PdfDocument, PdfObject}, Colorspace, Matrix, TextPageOptions};

struct SearchResult {
    image_index: u32,
    x: f32,
    y: f32,
}

struct Match {
    result_index: u32,
    score: f32,
}

struct SearchIndex {
    image_ids: Vec<String>,
    results: Vec<SearchResult>,
    words: HashMap<String, Vec<Match>>,
}

impl SearchIndex {
    fn new() -> Self {
        Self {
            image_ids: Vec::new(),
            results: Vec::new(),
            words: HashMap::new(),
        }
    }

    fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&self.image_ids.len().to_le_bytes())?;
        for id in self.image_ids.iter() {
            w.write_all(id.as_bytes())?;
        }
        w.write_all(&self.results.len().to_le_bytes())?;
        for r in self.results.iter() {
            w.write_all(&r.image_index.to_le_bytes())?;
            w.write_all(&r.x.to_le_bytes())?;
            w.write_all(&r.y.to_le_bytes())?;
        }
        w.write_all(&self.words.len().to_le_bytes())?;
        for (word, matches) in self.words.iter() {
            w.write_all(&word.len().to_le_bytes())?;
            w.write_all(word.as_bytes())?;
            w.write_all(&matches.len().to_le_bytes())?;
            for m in matches.iter() {
                w.write_all(&m.result_index.to_le_bytes())?;
                w.write_all(&m.score.to_le_bytes())?;
            }
        }
        Ok(())
    }
}

fn process_lesson_pdf<P: AsRef<Path>>(path: P, index: &mut SearchIndex) {
    let doc = PdfDocument::open(path.as_ref().to_str().unwrap()).unwrap();
    for page in doc.pages().unwrap() {
        let page = page.unwrap();
        /*
        let page_obj = doc.find_page(i as i32).unwrap()
            .resolve().unwrap().unwrap();
        let contents = page_obj.get_dict("Contents").unwrap().unwrap();
        let process_stream = |stream_obj: PdfObject| {
            let stream = stream_obj.read_stream().unwrap();
            println!("{}", String::from_utf8_lossy(&stream));
        };
        if contents.is_array().unwrap() {
            for j in 0..contents.len().unwrap() {
                process_stream(contents.get_array(j as i32).unwrap().unwrap());
            }
        } else {
            process_stream(contents);
        }
        */
        let text_page = page.to_text_page(TextPageOptions::empty()).unwrap();
        for b in text_page.blocks() {
            for l in b.lines() {
                let bounds = l.bounds();
                let x = (bounds.x0 + bounds.x1) / 2.;
                let y = (bounds.y0 + bounds.y1) / 2.;
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
                    x,
                    y,
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
