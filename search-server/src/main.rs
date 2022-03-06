use std::{collections::BTreeMap, env, fs::File};

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
            let words: Vec<String> = search_index::normalize(&query)
                .split(' ')
                .map(|w| w.to_owned())
                .collect();
            let mut scores: BTreeMap<u32, f32> = BTreeMap::new();
            for w in words.iter() {
                if let Some(matches) = search_index.words.get(w) {
                    for m in matches {
                        *scores.entry(m.result_index).or_default() += m.score;
                    }
                }
            }
            let mut sorted: Vec<_> = scores.into_iter().collect();
            sorted.sort_by(|(_, s_a), (_, s_b)| s_b.partial_cmp(s_a).unwrap());
            let mut pages: Vec<Page> = Vec::new();
            for (r, _) in sorted.iter() {
                let result = &search_index.results[*r as usize];
                let page = &search_index.pages[result.page_index as usize];
                let document_name = &search_index.documents[page.document_index as usize];
                let page_index = match pages
                    .iter()
                    .position(|p| p.rendered_image_id == page.rendered_image_id)
                {
                    Some(i) => i,
                    None => {
                        let i = pages.len();
                        // Limit page count.
                        if i > 20 {
                            break;
                        }
                        pages.push(Page {
                            document_name: document_name.to_owned(),
                            page_nr: page.page_nr,
                            rendered_image_id: page.rendered_image_id.clone(),
                            width: page.width,
                            height: page.height,
                            rects: Vec::new(),
                        });
                        i
                    }
                };
                pages[page_index].rects.push(Rect {
                    x: result.x,
                    y: result.y,
                    width: result.width,
                    height: result.height,
                });
            }
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
