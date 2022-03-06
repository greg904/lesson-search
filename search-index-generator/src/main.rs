use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, RwLock},
};

use mupdf::{pdf::PdfDocument, Colorspace, Matrix, Outline, TextPageOptions};
use rayon::prelude::*;
use search_index::{Match, Page, SearchIndex, SearchResult};

type ImageId = String;

type PageRenderCache = HashMap<u16, ImageId>;

struct DocumentRenderCache {
    by_path: HashMap<String, PageRenderCache>,
}

fn deserialize_u16<R: Read>(r: &mut R) -> io::Result<u16> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
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

    String::from_utf8(s).map_err(|_err| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))
}

impl DocumentRenderCache {
    fn new() -> Self {
        Self {
            by_path: HashMap::new(),
        }
    }

    fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&(self.by_path.len() as u16).to_le_bytes())?;
        for (path, pages) in self.by_path.iter() {
            w.write_all(&(path.len() as u32).to_le_bytes())?;
            w.write_all(path.as_bytes())?;
            w.write_all(&(pages.len() as u16).to_le_bytes())?;
            for (page, id) in pages.iter() {
                w.write_all(&page.to_le_bytes())?;
                w.write_all(&(id.len() as u32).to_le_bytes())?;
                w.write_all(id.as_bytes())?;
            }
        }
        Ok(())
    }

    fn deserialize<R: Read>(r: &mut R) -> io::Result<Self> {
        let doc_count = deserialize_u16(r)?;
        let mut by_path = HashMap::with_capacity(doc_count as usize);
        for _ in 0..doc_count {
            let path = deserialize_string(r)?;
            let page_count = deserialize_u16(r)?;
            let mut page_cache = HashMap::with_capacity(page_count as usize);
            for _ in 0..page_count {
                let page = deserialize_u16(r)?;
                let id = deserialize_string(r)?;
                page_cache.insert(page, id);
            }
            by_path.insert(path, page_cache);
        }
        Ok(Self { by_path })
    }
}

fn find_first_useful_outline(outlines: &[Outline]) -> Option<&Outline> {
    let o = outlines
        .iter()
        .find(|o| !o.title.eq_ignore_ascii_case("table des matières"))?;
    if o.down.is_empty() {
        return Some(o);
    }
    find_first_useful_outline(&o.down)
}

fn build_search_index_from_document(
    document_path: &Path,
    rendered_pages_path: &Path,
    document_render_cache: &RwLock<DocumentRenderCache>,
) -> SearchIndex {
    eprintln!("Processing {}...", document_path.display());

    let document_name = document_path.file_name().unwrap().to_str().unwrap();

    {
        let mut c = document_render_cache.write().unwrap();
        if !c.by_path.contains_key(document_name) {
            c.by_path
                .insert(document_name.to_owned(), PageRenderCache::new());
        }
    }

    let doc = PdfDocument::open(document_path.to_str().unwrap()).unwrap();

    // Ignore header and table of content by looking at the first outline that is not the table of
    // content, if there is one.
    let outlines = doc.outlines().unwrap();
    let first_useful_outline = find_first_useful_outline(&outlines);
    let content_start = first_useful_outline.map(|o| (o.page.unwrap(), o.y));

    let mut search_index = SearchIndex::new();
    search_index.documents.push(document_name.to_owned());

    const SCALE: f32 = 2.;

    for (page_nr, page) in doc.pages().unwrap().enumerate() {
        let page = page.unwrap();

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
                if line.is_empty() {
                    continue;
                }

                let words: Vec<String> = deunicode::deunicode(&line)
                    .to_ascii_lowercase()
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .map(|w| w.to_owned())
                    .collect();
                if words.is_empty() {
                    continue;
                }

                let mut score = 1.;
                // Heuristic for font size.
                score *= (bounds.x1 - bounds.x0) * (bounds.y1 - bounds.y0) / (line.len() as f32);
                // Boost certain patterns.
                if words.len() >= 2 && (words[0] == "theoreme" || words[0] == "definition") {
                    score *= 5.;
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
                            score,
                        });
                }
                has_result = true;
            }
        }
        if !has_result {
            continue;
        }

        // Try to reuse existing render for the page.
        let rendered_image_id = {
            let c = document_render_cache.read().unwrap();
            c.by_path
                .get(document_name)
                .and_then(|d| d.get(&(page_nr as u16)).cloned())
        }
        .unwrap_or_else(|| String::new());

        search_index.pages.push(Page {
            document_index: 0,
            page_nr: page_nr as u16,
            rendered_image_id,
        });
    }

    // Render pages that weren't already cached.
    search_index
        .pages
        .iter_mut()
        .filter(|p| p.rendered_image_id.is_empty())
        .map(|p| {
            // TODO: fix the `alpha` parameter not being a boolean
            let pixmap = doc
                .load_page(p.page_nr as i32)
                .unwrap()
                .to_pixmap(
                    &Matrix::new_scale(SCALE, SCALE),
                    &Colorspace::device_rgb(),
                    0.,
                    false,
                )
                .unwrap();
            (
                p,
                pixmap.width(),
                pixmap.height(),
                pixmap.samples().to_owned(),
            )
        })
        .collect::<Vec<_>>()
        .into_par_iter()
        .for_each(|(p, width, height, samples)| {
            eprintln!(
                "Encoding page {} of {}...",
                p.page_nr + 1,
                document_path.display()
            );
            let mut encoder = jpegxl_rs::encoder_builder()
                .lossless(true)
                .speed(jpegxl_rs::encode::EncoderSpeed::Tortoise)
                .decoding_speed(2)
                .build()
                .unwrap();
            let encoded = encoder.encode::<u8, u8>(&samples, width, height).unwrap();
            let digest = blake3::hash(&encoded);
            let id = base64::encode_config(digest.as_bytes(), base64::URL_SAFE_NO_PAD);
            fs::write(rendered_pages_path.join(format!("{}.jxl", id)), &*encoded).unwrap();

            {
                let mut c = document_render_cache.write().unwrap();
                let d = c.by_path.get_mut(document_name).unwrap();
                d.insert(p.page_nr, id.clone());
            }

            p.rendered_image_id = id;
        });

    if search_index.pages.is_empty() {
        search_index.documents.clear();
    }

    search_index
}

fn main() {
    let lessons_dir: PathBuf = env::var_os("LESSONS_DIR")
        .unwrap_or_else(|| "lessons".into())
        .into();
    let out_dir: PathBuf = env::var_os("OUT_DIR").unwrap_or_else(|| "db".into()).into();

    fs::create_dir_all(&out_dir).unwrap();

    let document_render_cache = match File::open(out_dir.join("document-render-cache.bin")) {
        Ok(ref mut f) => DocumentRenderCache::deserialize(f).unwrap(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => DocumentRenderCache::new(),
        Err(e) => panic!("failed to read page render cache: {}", e),
    };
    let document_render_cache = RwLock::new(document_render_cache);

    let rendered_pages_path = out_dir.join("rendered-pages");
    if let Err(e) = fs::create_dir(&rendered_pages_path) {
        if e.kind() != io::ErrorKind::AlreadyExists {
            panic!("failed to create rendered-pages directory");
        }
    }

    let search_index = Mutex::new(SearchIndex::new());
    fs::read_dir(lessons_dir)
        .unwrap()
        .par_bridge()
        .map(|e| e.unwrap())
        .filter(|e| {
            e.metadata().unwrap().is_file()
                && e.path().extension().map(|e| e == "pdf").unwrap_or(false)
        })
        .map(|e| {
            build_search_index_from_document(
                &e.path(),
                &rendered_pages_path,
                &document_render_cache,
            )
        })
        .for_each(|mut partial_index| {
            // Merge partial index into global index.
            let mut i = search_index.lock().unwrap();

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

    let mut search_index_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(out_dir.join("search-index.bin"))
        .unwrap();
    search_index
        .lock()
        .unwrap()
        .serialize(&mut search_index_file)
        .unwrap();

    let mut document_render_cache_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(out_dir.join("document-render-cache.bin"))
        .unwrap();
    document_render_cache
        .read()
        .unwrap()
        .serialize(&mut document_render_cache_file)
        .unwrap();
}
