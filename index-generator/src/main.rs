use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::{Mutex, RwLock},
};

use mupdf::{pdf::PdfDocument, Colorspace, Matrix, Outline, TextPageOptions};
use rayon::prelude::*;
use search_index::{Match, SearchIndex, SearchResult, Page};

type ImageId = String;

#[derive(Debug)]
struct ImageCacheDocument {
    by_page: HashMap<u32, ImageId>,
}

impl ImageCacheDocument {
    fn new() -> Self {
        Self {
            by_page: HashMap::new(),
        }
    }
}

struct ImageCache {
    by_path: HashMap<String, ImageCacheDocument>,
}

fn deserialize_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn deserialize_string<R: Read>(r: &mut R) -> io::Result<String> {
    let len = deserialize_u32(r)?;

    let mut s = vec![0u8; len as usize];
    r.read_exact(&mut s)?;

    Ok(String::from_utf8(s)
        .map_err(|_err| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))?)
}

impl ImageCache {
    fn new() -> Self {
        Self {
            by_path: HashMap::new(),
        }
    }

    fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&(self.by_path.len() as u32).to_le_bytes())?;
        for (path, doc) in self.by_path.iter() {
            w.write_all(&(path.len() as u32).to_le_bytes())?;
            w.write_all(path.as_bytes())?;
            w.write_all(&(doc.by_page.len() as u32).to_le_bytes())?;
            for (page, id) in doc.by_page.iter() {
                w.write_all(&page.to_le_bytes())?;
                w.write_all(&(id.len() as u32).to_le_bytes())?;
                w.write_all(id.as_bytes())?;
            }
        }
        Ok(())
    }

    fn deserialize<R: Read>(r: &mut R) -> io::Result<Self> {
        let doc_count = deserialize_u32(r)?;
        let mut by_path = HashMap::with_capacity(doc_count as usize);
        for _ in 0..doc_count {
            let path = deserialize_string(r)?;
            let page_count = deserialize_u32(r)?;
            let mut by_page = HashMap::with_capacity(page_count as usize);
            for _ in 0..page_count {
                let page = deserialize_u32(r)?;
                let id = deserialize_string(r)?;
                by_page.insert(
                    page,
                    id
                );
            }
            by_path.insert(path, ImageCacheDocument { by_page });
        }
        Ok(Self { by_path })
    }
}

fn find_first_useful_outline(outlines: &[Outline]) -> Option<&Outline> {
    let o = outlines
        .iter()
        .filter(|o| !o.title.eq_ignore_ascii_case("table des matières"))
        .next()?;
    if o.down.is_empty() {
        return Some(o);
    }
    find_first_useful_outline(&o.down)
}

fn build_search_index_from_document<P: AsRef<Path>>(
    document_path: P,
    image_cache: &RwLock<ImageCache>,
) -> SearchIndex {
    eprintln!("Processing {}...", document_path.as_ref().display());

    let mut encoder = jpegxl_rs::encoder_builder()
        .lossless(true)
        .speed(jpegxl_rs::encode::EncoderSpeed::Tortoise)
        .build()
        .unwrap();

    let document_path_str = document_path.as_ref().as_os_str().to_str().unwrap();

    {
        let mut c = image_cache.write().unwrap();
        if !c.by_path.contains_key(document_path_str) {
            c.by_path
                .insert(document_path_str.to_owned(), ImageCacheDocument::new());
        }
    }

    let doc = PdfDocument::open(document_path.as_ref().to_str().unwrap()).unwrap();

    // Ignore header and table of content by looking at the first outline that is not the table of
    // content, if there is one.
    let outlines = doc.outlines().unwrap();
    let first_useful_outline = find_first_useful_outline(&outlines);
    let content_start = first_useful_outline.map(|o| (o.page.unwrap(), o.y));

    let mut search_index = SearchIndex::new();
    search_index.documents.push(document_path_str.to_owned());

    for (page_nr, page) in doc.pages().unwrap().enumerate() {
        let page = page.unwrap();

        const SCALE: f32 = 3.;

        // Scan the page for text.
        if let Some((start_page_nr, _start_y)) = content_start {
            if page_nr < start_page_nr as usize {
                continue;
            }
        }
        let text_page = page.to_text_page(TextPageOptions::empty()).unwrap();
        let page_index = search_index.pages.len() as u32;
        let mut has_result = false;
        for b in text_page.blocks() {
            for l in b.lines() {
                let bounds = l.bounds();
                if let Some((start_page_nr, start_y)) = content_start {
                    if page_nr == start_page_nr as usize && bounds.y1 < start_y {
                        continue;
                    }
                }
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
                let result_index = search_index.results.len();
                search_index.results.push(SearchResult {
                    page_index,
                    x: (bounds.x0 * SCALE) as i16,
                    y: (bounds.y0 * SCALE) as i16,
                    width: ((bounds.x1 - bounds.x0) * SCALE) as u16,
                    height: ((bounds.y1 - bounds.y0) * SCALE) as u16,
                });
                for w in words.iter() {
                    search_index
                        .words
                        .entry(w.to_string())
                        .or_default()
                        .push(Match {
                            result_index: result_index as u32,
                            score: 1. / words.len() as f32,
                        });
                }
                has_result = true;
            }
        }
        if !has_result {
            continue;
        }

        // Encode image for the page or reuse existing one.
        let image_id = {
            let c = image_cache.read().unwrap();
            let d = c.by_path.get(document_path_str).unwrap();
            d.by_page.get(&(page_nr as u32)).cloned()
        }
        .unwrap_or_else(|| {
            eprintln!(
                "Encoding page {} of {}...",
                page_index,
                document_path.as_ref().display()
            );
            // TODO: fix the `alpha` parameter not being a boolean
            let pixmap = page
                .to_pixmap(
                    &Matrix::new_scale(SCALE, SCALE),
                    &Colorspace::device_rgb(),
                    0.,
                    false,
                )
                .unwrap();
            let encoded = encoder
                .encode::<u8, u8>(pixmap.samples(), pixmap.width(), pixmap.height())
                .unwrap();
            let digest = blake3::hash(&encoded);
            let id = base64::encode_config(digest.as_bytes(), base64::URL_SAFE_NO_PAD);
            fs::write(format!("index/images/{}.jxl", id), &*encoded).unwrap();

            let mut c = image_cache.write().unwrap();
            let d = c.by_path.get_mut(document_path_str).unwrap();
            d.by_page.insert(page_index as u32, id.clone());

            id
        });
        search_index.pages.push(Page {
            document_index: 0,
            page_nr: page_nr as u16,
            rendered_image_id: image_id,
        });
    }

    if search_index.pages.is_empty() {
        search_index.documents.clear();
    }

    search_index
}

fn main() {
    let image_cache = match File::open("index/image-cache.bin") {
        Ok(ref mut f) => ImageCache::deserialize(f).unwrap(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => ImageCache::new(),
        Err(e) => panic!("failed to read image cache: {}", e),
    };
    let image_cache = RwLock::new(image_cache);

    fs::create_dir_all("index/images").unwrap();

    let index = Mutex::new(SearchIndex::new());
    fs::read_dir("lessons")
        .unwrap()
        .par_bridge()
        .map(|e| e.unwrap())
        .filter(|e| {
            e.metadata().unwrap().is_file()
                && e.path().extension().map(|e| e == "pdf").unwrap_or(false)
        })
        .map(|e| build_search_index_from_document(e.path(), &image_cache))
        .for_each(|mut partial_index| {
            // Merge partial index into global index.
            let mut i = index.lock().unwrap();

            let document_index_base = i.documents.len() as u16;
            i.documents.extend_from_slice(&partial_index.documents);

            for p in partial_index.pages.iter_mut() {
                p.document_index += document_index_base;
            }
            let page_index_base = i.pages.len() as u32;
            i.pages.extend_from_slice(&partial_index.pages);

            for r in partial_index.results.iter_mut() {
                r.page_index += page_index_base;
            }
            let result_index_base = i.results.len() as u32;
            i.results.extend_from_slice(&partial_index.results);

            for (word, mut partial_matches) in partial_index.words.into_iter() {
                let matches = i.words.entry(word).or_default();
                for m in partial_matches.iter_mut() {
                    m.result_index += result_index_base;
                }
                matches.extend_from_slice(&partial_matches);
            }
        });

    let mut index_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open("index/index.bin")
        .unwrap();
    index.lock().unwrap().serialize(&mut index_file).unwrap();

    let mut image_cache_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open("index/image-cache.bin")
        .unwrap();
    image_cache
        .read()
        .unwrap()
        .serialize(&mut image_cache_file)
        .unwrap();
}
