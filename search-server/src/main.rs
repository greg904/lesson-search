use std::fs::File;

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
            let mut results: Vec<QueryResponseItem> = Vec::new();
            'search: for w in words.iter() {
                if let Some(matches) = search_index.words.get(w) {
                    for m in matches {
                        let result = &search_index.results[m.result_index as usize];
                        let image_id = &search_index.image_ids[result.image_index as usize];
                        results.push(QueryResponseItem {
                            image_id: image_id.clone(),
                            x: result.x,
                            y: result.y,
                            width: result.width,
                            height: result.height,
                        });
                        if results.len() > 5 {
                            break 'search;
                        }
                    }
                }
            }
            let body = serde_json::to_string(&results).unwrap().into();
            Response {
                status_code: 200,
                headers: Vec::new(),
                body,
            }
        })
        .unwrap();
}
