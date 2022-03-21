use std::{
    collections::{BTreeMap, HashMap},
    env,
    fs::File,
};

use search_index::index::SearchIndex;
use serde::Serialize;

use http_server::{HttpServer, Response};

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
    rendered_avif: String,
    rendered_jpeg: String,
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
            let pages: Vec<_> = search_index::search::search(&search_index, &query)
                .into_iter()
                .map(|p| Page {
                    document_name: p.document_digest,
                    page_nr: p.number,
                    rendered_avif: p.rendered_avif,
                    rendered_jpeg: p.rendered_jpeg,
                    width: p.width,
                    height: p.height,
                    rects: p.highlights.into_iter().map(|h| Rect {
                        x: h.x,
                        y: h.y,
                        width: h.width,
                        height: h.height,
                    }).collect(),
                })
                .collect();
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
