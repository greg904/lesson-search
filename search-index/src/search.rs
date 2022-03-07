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
    pub image_digest: Digest,
    pub width: u16,
    pub height: u16,
    pub highlights: Vec<Highlight>,
}

pub fn search(search_index: &SearchIndex, query: &str) -> Vec<MatchPage> {
    let words = normalize_and_extract_words(&query);
    if words.is_empty() {
        return Vec::new();
    }

    let mut pages: BTreeMap<u32, (Vec<u32>, HashMap<String, f32>)> = BTreeMap::new();
    for w in words.iter() {
        if let Some(matches) = search_index.words.get(w) {
            for m in matches {
                let result = &search_index.results[m.result_index as usize];
                let (results, max_scores) = pages.entry(result.page_index).or_default();
                // Limit the amount of rect per page.
                if results.len() >= 20 {
                    continue;
                }
                results.push(m.result_index);
                let max_score = max_scores.entry(w.to_owned()).or_default();
                *max_score = max_score.max(m.score);
            }
        }
    }

    let mut pages: Vec<_> = pages.into_iter().collect();
    pages.sort_by(|(_, page_a), (_, page_b)| {
        let score_a: f32 = page_a.1.values().sum();
        let score_b: f32 = page_b.1.values().sum();
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
                image_digest: page.rendered_image_id.clone(),
                width: page.width,
                height: page.height,
                highlights,
            }
        })
        .take(10)
        .collect()
}
