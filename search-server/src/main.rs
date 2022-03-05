use std::{collections::BTreeMap, fs::File};

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
struct PageMatch {
    image_id: String,
    rects: Vec<Rect>,
}

fn main() {
    let mut search_index_file = File::open("index/index.bin").unwrap();
    let search_index = SearchIndex::deserialize(&mut search_index_file).unwrap();

    let server = HttpServer::bind("127.0.0.1:3000").unwrap();
    server
        .serve(|req| {
            let query = urlencoding::decode(&req.url[1..]).unwrap();
            let words: Vec<String> = deunicode::deunicode(&query)
                .to_ascii_lowercase()
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|w| w.len() > 2)
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
            let mut pages: Vec<PageMatch> = Vec::new();
            for (r, _) in sorted.iter() {
                let result = &search_index.results[*r as usize];
                let image_id = &search_index.image_ids[result.image_index as usize];
                let page_index = match pages.iter().position(|p| &p.image_id == image_id) {
                    Some(i) => i,
                    None => {
                        let i = pages.len();
                        // Limit page count.
                        if i > 5 {
                            break;
                        }
                        pages.push(PageMatch {
                            image_id: image_id.clone(),
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
            let body = serde_json::to_string(&pages).unwrap().into();
            Response {
                status_code: 200,
                headers: Vec::new(),
                body,
            }
        })
        .unwrap();
}
