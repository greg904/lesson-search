use std::{collections::BTreeMap, fs::File};

use serde::Serialize;

use http_server::{HttpServer, Response};
use search_index::SearchIndex;

mod http_server;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryResponseItem {
    image_id: String,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
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
            let mut score_by_result_index: BTreeMap<u32, f32> = BTreeMap::new();
            for w in words.iter() {
                if let Some(matches) = search_index.words.get(w) {
                    for m in matches {
                        *score_by_result_index.entry(m.result_index).or_default() += m.score;
                    }
                }
            }
            let mut sorted: Vec<_> = score_by_result_index.into_iter().collect();
            sorted.sort_by(|(_, s_a), (_, s_b)| s_b.partial_cmp(s_a).unwrap());
            let results: Vec<QueryResponseItem> = sorted
                .iter()
                .map(|(r, _)| {
                    let result = &search_index.results[*r as usize];
                    let image_id = &search_index.image_ids[result.image_index as usize];
                    QueryResponseItem {
                        image_id: image_id.clone(),
                        x: result.x,
                        y: result.y,
                        width: result.width,
                        height: result.height,
                    }
                })
                .take(5)
                .collect();
            let body = serde_json::to_string(&results).unwrap().into();
            Response {
                status_code: 200,
                headers: Vec::new(),
                body,
            }
        })
        .unwrap();
}
