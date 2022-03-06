use std::{collections::{BTreeMap, HashMap}, env, fs::File};

use serde::Serialize;

use http_server::{HttpServer, Response};
use search_index::SearchIndex;

mod http_server;

#[derive(Serialize)]
struct Rect {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    document_name: String,
    page_nr: u16,
    rendered_image_id: String,
    width: u16,
    height: u16,
    rects: Vec<Rect>,
}

fn main() {
    let addr = env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    let search_index_path =
        env::var("INDEX_FILE").unwrap_or_else(|_| "db/search-index.bin".to_owned());
    let cors_origin =
        env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:8000".to_owned());

    let mut search_index_file = File::open(search_index_path).unwrap();
    let search_index = SearchIndex::deserialize(&mut search_index_file).unwrap();

    let server = HttpServer::bind(addr).unwrap();
    server
        .serve(|req| {
            let query = urlencoding::decode(&req.url[1..]).unwrap();
            let normalized = search_index::normalize(&query);
            let pages = if normalized.is_empty() {
                Vec::new()
            } else {
                let words: Vec<String> = search_index::normalize(&query)
                    .split(' ')
                    .map(|w| w.to_owned())
                    .collect();

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

                pages.into_iter()
                    .map(|(page_index, page_search)| {
                        let (result_indices, _score) = page_search;
                        let rects = result_indices.into_iter()
                            .map(|r| {
                                let result = &search_index.results[r as usize];
                                Rect {
                                    x: result.x,
                                    y: result.y,
                                    width: result.width,
                                    height: result.height,
                                }
                            })
                            .collect();
                        let page = &search_index.pages[page_index as usize];
                        let document_name = &search_index.documents[page.document_index as usize];
                        Page {
                            document_name: document_name.to_owned(),
                            page_nr: page.page_nr,
                            rendered_image_id: page.rendered_image_id.clone(),
                            width: page.width,
                            height: page.height,
                            rects,
                        }
                    })
                    .take(10)
                    .collect()
            };
            let body: Vec<u8> = serde_json::to_string(&pages).unwrap().into();
            let mut headers = vec![("Content-Length".to_string(), body.len().to_string())];
            if !cors_origin.is_empty() {
                headers.push((
                    "Access-Control-Allow-Origin".to_owned(),
                    cors_origin.clone(),
                ));
            }
            Response {
                status_code: 200,
                headers,
                body,
            }
        })
        .unwrap();
}
