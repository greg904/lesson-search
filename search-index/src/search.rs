use std::collections::{BTreeMap, HashMap};

use crate::{index::SearchIndex, normalize::normalize_and_extract_words};

/// A rectangle containing words from the query.
pub struct Highlight {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// A document or image digest.
pub type Digest = String;

pub type PageNumber = u16;

/// A page that matches the query.
pub struct MatchPage {
    pub document_digest: Digest,
    pub number: PageNumber,
    pub rendered_avif: Digest,
    pub rendered_jpeg: Digest,
    pub width: u16,
    pub height: u16,
    pub highlights: Vec<Highlight>,
}

const TILE_SIZE: u32 = 64;
const HOTSPOT_RADIUS: f32 = 100.;

#[derive(Clone, Default)]
struct PageHotspotTile {
    max_score_per_word: HashMap<String, f32>,
}

impl PageHotspotTile {
    fn total_score(&self) -> f32 {
        self.max_score_per_word.values().map(|s| s.sqrt()).sum()
    }
}

struct PageHotspotImage {
    tiled_height: u32,
    tiles: Vec<PageHotspotTile>,
}

impl PageHotspotImage {
    fn new(height: u32) -> Self {
        let tiled_height = (height + TILE_SIZE - 1) / TILE_SIZE;
        Self {
            tiled_height,
            tiles: vec![Default::default(); tiled_height as usize],
        }
    }

    fn update_score(&mut self, y: f32, word: String, score: f32) {
        let tile_y_min = (y - HOTSPOT_RADIUS) as i32 / TILE_SIZE as i32;
        let tile_y_max = (y + HOTSPOT_RADIUS + TILE_SIZE as f32 - 1.) as i32 / TILE_SIZE as i32;
        for ty in tile_y_min..=tile_y_max {
            if ty < 0 || ty >= self.tiled_height as i32 {
                continue;
            }
            let distance = (y - (ty as f32 + 0.5) * TILE_SIZE as f32).abs();
            // Clamp the distance so that we always get the maximum score on the tile that we're
            // in.
            let distance = distance.max(TILE_SIZE as f32 / 2.);
            let factor = (HOTSPOT_RADIUS - distance).max(0.) / HOTSPOT_RADIUS;
            let factor = (factor * factor).max(0.25);
            let tile = &mut self.tiles[ty as usize];
            let max_score = tile.max_score_per_word.entry(word.clone()).or_default();
            *max_score = max_score.max(score * factor);
        }
    }

    fn maximum_score(&self) -> f32 {
        self.tiles.iter()
            .map(|t| t.total_score())
            .max_by(|x, y| x.partial_cmp(y).unwrap())
            .unwrap_or(0.)
    }
}

pub fn search(search_index: &SearchIndex, query: &str) -> Vec<MatchPage> {
    let words = normalize_and_extract_words(&query);
    if words.is_empty() {
        return Vec::new();
    }

    let mut pages: BTreeMap<u32, (Vec<u32>, PageHotspotImage)> = BTreeMap::new();
    for w in words.into_iter() {
        // Prefix key search.
        for (word, matches) in search_index.words.range(w.clone()..) {
            if !word.starts_with(&w) {
                break;
            }
            let score_multiplier = (w.len() as f32) / (word.len() as f32);
            for m in matches {
                let result = &search_index.results[m.result_index as usize];
                let page = &search_index.pages[result.page_index as usize];
                let (results, hotspot_image) = pages.entry(result.page_index)
                    .or_insert_with(|| (Vec::new(), PageHotspotImage::new(page.height.into())));
                // Limit the amount of rect per page.
                if results.len() < 50 && !results.contains(&m.result_index) {
                    results.push(m.result_index);
                }
                let y = result.y as f32 + result.height as f32 / 2.;
                hotspot_image.update_score(y, w.to_owned(), m.score * score_multiplier);
            }
        }
    }

    let mut pages: Vec<_> = pages.into_iter().collect();
    pages.sort_by(|(_, page_a), (_, page_b)| {
        let score_a: f32 = page_a.1.maximum_score();
        let score_b: f32 = page_b.1.maximum_score();
        score_b.partial_cmp(&score_a).unwrap()
    });

    pages
        .into_iter()
        .map(|(page_index, page_search)| {
            let (result_indices, _score) = page_search;
            let highlights = result_indices
                .into_iter()
                .map(|r| {
                    let result = &search_index.results[r as usize];
                    Highlight {
                        x: result.x,
                        y: result.y,
                        width: result.width,
                        height: result.height,
                    }
                })
                .collect();
            let page = &search_index.pages[page_index as usize];
            let document_digest = search_index.documents[page.document_index as usize].to_owned();
            MatchPage {
                document_digest,
                number: page.page_nr,
                rendered_avif: page.rendered_avif.clone(),
                rendered_jpeg: page.rendered_jpeg.clone(),
                width: page.width,
                height: page.height,
                highlights,
            }
        })
        .take(5)
        .collect()
}
