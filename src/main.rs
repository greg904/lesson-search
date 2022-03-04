use std::borrow::Borrow;

use http_server::{HttpServer, Response};
use pdf_search::PdfSearcher;
use ravif::{ColorSpace, Img};
use rgb::FromSlice;

mod http_server;
mod pdf_search;

fn main() {
    let pdf_searcher = PdfSearcher::new("16_integrales_a_parametres.pdf").unwrap();
    let server = HttpServer::bind("127.0.0.1:3000").unwrap();
    server
        .serve(|req| {
            let query = urlencoding::decode(&req.url[1..]).unwrap();
            let results = pdf_searcher.search(&query).unwrap();
            let mut body = Vec::new();
            for result in results.into_iter() {
                let (encoded, _) = ravif::encode_rgb(
                    Img::new(
                        (&*result.rgb).as_rgb(),
                        result.width as usize,
                        result.height as usize,
                    ),
                    &ravif::Config {
                        quality: 50.,
                        alpha_quality: 0.,
                        speed: 10,
                        premultiplied_alpha: false,
                        color_space: ColorSpace::RGB,
                        threads: 0,
                    },
                )
                .unwrap();
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
