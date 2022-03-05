use std::fs::File;

use http_server::{HttpServer, Response};
use pdf_search::PdfSearcher;
use search_index::SearchIndex;

mod http_server;
mod pdf_search;

fn main() {
    let mut search_index_file = File::open("index/index.bin").unwrap();
    let search_index = SearchIndex::deserialize(&mut search_index_file).unwrap();

    let pdf_searcher = PdfSearcher::new("16_integrales_a_parametres.pdf").unwrap();
    let server = HttpServer::bind("127.0.0.1:3000").unwrap();
    server
        .serve(|req| {
            let query = urlencoding::decode(&req.url[1..]).unwrap();
            let results = pdf_searcher.search(&query).unwrap();
            let mut body = Vec::new();
            for result in results.into_iter() {
                let mut encoded = Vec::new();
                let mut encoder = JpegEncoder::new(&mut encoded);
                encoder.encode(&*result.rgb, result.width, result.height, ColorType::Rgb8).unwrap();
                body.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
                body.extend_from_slice(&encoded);
            }
            Response {
                status_code: 200,
                headers: Vec::new(),
                body,
            }
        })
        .unwrap();
}
