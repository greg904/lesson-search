use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

use mupdf::{pdf::PdfDocument, Colorspace, Matrix, TextPageOptions};
use search_index::{Match, SearchIndex, SearchResult};

type ImageId = String;

struct ImageCache {
    by_path: HashMap<String, Vec<ImageId>>,
}

impl ImageCache {
    fn new() -> Self {
        Self {
            by_path: HashMap::new(),
        }
    }

    fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&(self.by_path.len() as u32).to_le_bytes())?;
        for (path, ids) in self.by_path.iter() {
            w.write_all(&(path.len() as u32).to_le_bytes())?;
            w.write_all(path.as_bytes())?;
            w.write_all(&(ids.len() as u32).to_le_bytes())?;
            for id in ids.iter() {
                w.write_all(&(id.len() as u32).to_le_bytes())?;
                w.write_all(id.as_bytes())?;
            }
        }
        Ok(())
    }

    fn deserialize<R: Read>(r: &mut R) -> io::Result<Self> {
        let mut buf = [0u8; 4];
        r.read_exact(&mut buf)?;
        let pdf_count = u32::from_le_bytes(buf);
        let mut by_path = HashMap::with_capacity(pdf_count as usize);
        for _ in 0..pdf_count {
            r.read_exact(&mut buf)?;
            let path_len = u32::from_le_bytes(buf);
            let mut path = vec![0; path_len as usize];
            r.read_exact(&mut path)?;
            r.read_exact(&mut buf)?;
            let id_count = u32::from_le_bytes(buf);
            let mut ids = Vec::with_capacity(id_count as usize);
            for _ in 0..id_count {
                r.read_exact(&mut buf)?;
                let id_len = u32::from_le_bytes(buf);
                let mut id = vec![0; id_len as usize];
                r.read_exact(&mut id)?;
                ids.push(
                    String::from_utf8(id)
                        .map_err(|_err| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))?,
                );
            }
            let path_str = String::from_utf8(path)
                .map_err(|_err| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))?;
            by_path.insert(path_str, ids);
        }
        Ok(Self { by_path })
    }
}

fn process_lesson_pdf<P: AsRef<Path>>(
    path: P,
    index: &mut SearchIndex,
    image_cache: &mut ImageCache,
) {
    eprintln!("Processing {}...", path.as_ref().display());

    let mut compressor = turbojpeg::Compressor::new().unwrap();
    compressor.set_quality(50);
    compressor.set_subsamp(turbojpeg::Subsamp::Sub2x1);

    let mut image_cache_doc = image_cache
        .by_path
        .entry(
            path.as_ref()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned(),
        )
        .or_default();

    let doc = PdfDocument::open(path.as_ref().to_str().unwrap()).unwrap();
    for (page_index, page) in doc.pages().unwrap().enumerate() {
        let page = page.unwrap();
        let text_page = page.to_text_page(TextPageOptions::empty()).unwrap();
        for b in text_page.blocks() {
            for l in b.lines() {
                let bounds = l.bounds();
                let line: String = l.chars().flat_map(|c| c.char()).collect();
                let words: Vec<String> = deunicode::deunicode(&line)
                    .to_ascii_lowercase()
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .filter(|w| w.len() > 2)
                    .map(|w| w.to_owned())
                    .collect();
                if words.len() < 2 || (words[0] != "theoreme" && words[0] != "definition") {
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
                    index.words.entry(w.to_string()).or_default().push(Match {
                        result_index: result_index as u32,
                        score: 1. / words.len() as f32,
                    });
                }
            }
        }
        let image_id = if page_index >= image_cache_doc.len() {
            // TODO: fix the `alpha` parameter not being a boolean
            let pixmap = page
                .to_pixmap(
                    &Matrix::new_scale(3., 3.),
                    &Colorspace::device_rgb(),
                    0.,
                    false,
                )
                .unwrap();
            let encoded = compressor
                .compress_to_vec(turbojpeg::Image {
                    pixels: pixmap.samples(),
                    width: pixmap.width() as usize,
                    height: pixmap.height() as usize,
                    pitch: (pixmap.width() * 3) as usize,
                    format: turbojpeg::PixelFormat::RGB,
                })
                .unwrap();
            let digest = blake3::hash(&encoded);
            let id = base64::encode_config(digest.as_bytes(), base64::URL_SAFE_NO_PAD);
            fs::write(format!("index/images/{}.jpg", id), encoded).unwrap();

            assert!(image_cache_doc.len() == page_index);
            image_cache_doc.push(id.clone());
            id
        } else {
            image_cache_doc[page_index].clone()
        };
        index.image_ids.push(image_id);
    }
}

fn main() {
    let mut image_cache = match File::open("index/image-cache.bin") {
        Ok(ref mut f) => ImageCache::deserialize(f).unwrap(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => ImageCache::new(),
        Err(e) => panic!("failed to read image cache: {}", e),
    };

    fs::create_dir_all("index/images").unwrap();
    let mut index = SearchIndex::new();
    for entry in fs::read_dir("lessons").unwrap() {
        let entry = entry.unwrap();
        if entry.metadata().unwrap().is_file()
            && entry
                .path()
                .extension()
                .map(|e| e == "pdf")
                .unwrap_or(false)
        {
            process_lesson_pdf(entry.path(), &mut index, &mut image_cache);
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open("index/index.bin")
        .unwrap();
    index.serialize(&mut file).unwrap();

    let mut image_cache_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open("index/image-cache.bin")
        .unwrap();
    image_cache.serialize(&mut image_cache_file).unwrap();
}
