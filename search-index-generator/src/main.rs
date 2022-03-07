use std::{
    env,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::{Mutex, RwLock},
};

use mupdf::{pdf::PdfDocument, Colorspace, Matrix, Outline, TextPageOptions};
use rayon::prelude::*;
use search_index::index::{Match, Page, SearchIndex, SearchResult};

mod page_render_cache;

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
    cache: &RwLock<page_render_cache::DocumentMap>,
) -> SearchIndex {
    eprintln!("Processing {}...", document_path.display());

    let document_name = document_path.file_name().unwrap().to_str().unwrap();

    {
        let mut c = cache.write().unwrap();
        if !c.0.contains_key(document_name) {
            c.0.insert(
                document_name.to_owned(),
                page_render_cache::PageImageMap::new(),
            );
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

    const SCALE: f32 = 1.8;

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

                let mut words = search_index::normalize::normalize_and_extract_words(&line);
                if words.is_empty() {
                    continue;
                }

                let mut score = 1.;
                // Heuristic for font size.
                let font_size = ((bounds.x1 - bounds.x0) * (bounds.y1 - bounds.y0)
                    / (line.len() as f32))
                    .sqrt();
                const AVG_FONT_SIZE: f32 = 10.;
                const FONT_SIZE_STD: f32 = 2.;
                let mut importance = 1. / (1. + (-(font_size - AVG_FONT_SIZE) / FONT_SIZE_STD).clamp(-10., 10.).exp());
                // Boost certain patterns. Note that the words are stemmed.
                if words.len() >= 2
                    && (words[0] == "theorem"
                        || words[0] == "definit"
                        || words[0] == "propriet"
                        || words[0] == "method")
                {
                    importance = importance.max(0.95);
                }
                score += importance;

                // Remove duplicate words to prevent counting them multiple times for a single line.
                words.sort();
                words.dedup();

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
        let cached_page = {
            cache
                .read()
                .unwrap()
                .0
                .get(document_name)
                .and_then(|d| d.0.get(&(page_nr as u16)).cloned())
        };

        search_index.pages.push(Page {
            document_index: 0,
            page_nr: page_nr as u16,
            width: cached_page.as_ref().map(|p| p.width).unwrap_or(0),
            height: cached_page.as_ref().map(|p| p.height).unwrap_or(0),
            rendered_image_id: cached_page
                .map(|p| p.digest)
                .unwrap_or_else(|| String::new()),
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
            let pixels = libavif::RgbPixels::new(width, height, &samples).unwrap();
            let image = pixels.to_image(libavif::YuvFormat::Yuv444);
            let mut encoder = libavif::Encoder::new();
            encoder.set_quantizer(40);
            encoder.set_speed(0);
            let encoded = &*encoder.encode(&image).unwrap();
            let digest = blake3::hash(&encoded);
            let digest = base64::encode_config(digest.as_bytes(), base64::URL_SAFE_NO_PAD);
            fs::write(
                rendered_pages_path.join(format!("{}.avif", digest)),
                &*encoded,
            )
            .unwrap();

            {
                let mut c = cache.write().unwrap();
                let d = c.0.get_mut(document_name).unwrap();
                d.0.insert(
                    p.page_nr,
                    page_render_cache::Image {
                        digest: digest.clone(),
                        width: width as u16,
                        height: height as u16,
                    },
                );
            }

            p.rendered_image_id = digest;
            p.width = width as u16;
            p.height = height as u16;
        });

    if search_index.pages.is_empty() {
        search_index.documents.clear();
    }

    search_index
}

pub(crate) fn build_search_index(lessons_dir: &Path, out_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(&out_dir)?;

    let cache = match File::open(out_dir.join("document-render-cache.bin")) {
        Ok(ref mut f) => page_render_cache::DocumentMap::deserialize(f)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => page_render_cache::DocumentMap::new(),
        Err(e) => return Err(e),
    };
    let cache = RwLock::new(cache);

    let rendered_pages_path = out_dir.join("rendered-pages");
    if let Err(e) = fs::create_dir(&rendered_pages_path) {
        if e.kind() != io::ErrorKind::AlreadyExists {
            return Err(e);
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
        .map(|e| build_search_index_from_document(&e.path(), &rendered_pages_path, &cache))
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
        .open(out_dir.join("search-index.bin"))?;
    search_index
        .lock()
        .unwrap()
        .serialize(&mut search_index_file)?;

    let mut cache_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(out_dir.join("document-render-cache.bin"))?;
    cache.read().unwrap().serialize(&mut cache_file)?;

    Ok(())
}

fn main() -> io::Result<()> {
    let lessons_dir: PathBuf = env::var_os("LESSONS_DIR")
        .unwrap_or_else(|| "lessons".into())
        .into();
    let out_dir: PathBuf = env::var_os("OUT_DIR").unwrap_or_else(|| "db".into()).into();
    build_search_index(&lessons_dir, &out_dir)
}

#[cfg(test)]
mod tests {
    use std::{path::Path, fs::File};

    use search_index::{index::SearchIndex, search::search};

    use super::build_search_index;

    #[test]
    fn good_results() {
        build_search_index(Path::new("../lessons"), Path::new("../db-test")).unwrap();
        let search_index = SearchIndex::deserialize(&mut File::open("../db-test/search-index.bin").unwrap()).unwrap();

        let cs_results = search(&search_index, "cs");
        assert!(!cs_results.is_empty());
        assert_eq!(cs_results[0].document_digest, "23_rev_Espaces_prehilbertiens.pdf");
        assert_eq!(cs_results[0].number, 4);

        let orthogonal_supplementaire_results = search(&search_index, "orthogonal supplémentaire");
        assert!(!orthogonal_supplementaire_results.is_empty());
        assert_eq!(orthogonal_supplementaire_results[0].document_digest, "23_rev_Espaces_prehilbertiens.pdf");
        assert_eq!(orthogonal_supplementaire_results[0].number, 16);

        let proj_results = search(&search_index, "Expression du projeté en base orthonormale");
        assert!(!proj_results.is_empty());
        assert_eq!(proj_results[0].document_digest, "23_rev_Espaces_prehilbertiens.pdf");
        assert_eq!(proj_results[0].number, 20);

        let dist_results = search(&search_index, "Distance à un sous­espace de dimension finie.");
        assert!(!dist_results.is_empty());
        assert_eq!(dist_results[0].document_digest, "23_rev_Espaces_prehilbertiens.pdf");
        assert_eq!(dist_results[0].number, 23);

        let total_series_results = search(&search_index, "Caractérisation des suites totales par des projecteurs orthogonaux.");
        assert!(!total_series_results.is_empty());
        assert_eq!(total_series_results[0].document_digest, "24_Espaces_prehilbertiens_suite.pdf");
        assert_eq!(total_series_results[0].number, 2);

        let endom_sym_results = search(&search_index, "Matrice en base orthonormale d’un endomorphisme symétrique");
        assert!(!endom_sym_results.is_empty());
        assert_eq!(endom_sym_results[0].document_digest, "24_Espaces_prehilbertiens_suite.pdf");
        assert_eq!(endom_sym_results[0].number, 9);

        let proj_ortho_results = search(&search_index, "Les projections orthogonales sont les projections symétriques");
        assert!(!proj_ortho_results.is_empty());
        assert_eq!(proj_ortho_results[0].document_digest, "24_Espaces_prehilbertiens_suite.pdf");
        assert_eq!(proj_ortho_results[0].number, 10);

        let sp_theorem_results = search(&search_index, "Caractérisation des suites totales par des projecteurs orthogonaux.");
        assert!(!sp_theorem_results.is_empty());
        assert_eq!(sp_theorem_results[0].document_digest, "24_Espaces_prehilbertiens_suite.pdf");
        assert_eq!(sp_theorem_results[0].number, 2);
    }
}
