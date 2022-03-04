use mupdf::{Colorspace, Matrix, Pixmap, TextPageOptions, pdf::PdfDocument};

pub(crate) struct PdfSearcher {
    doc: PdfDocument,
    page_pixmaps: Vec<Pixmap>,
}

pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

type PdfError = mupdf::Error;

impl PdfSearcher {
    pub fn new(path: &str) -> Result<Self, PdfError> {
        let doc = PdfDocument::open(path)?;
        let page_pixmaps = doc.pages()?
            .map(|p| p?.to_pixmap(&Matrix::IDENTITY, &Colorspace::device_rgb(), 0., false))
            .collect::<Result<Vec<Pixmap>, _>>()?;
        Ok(Self { doc, page_pixmaps })
    }

    pub fn search(&self, text: &str) -> Result<Vec<Image>, PdfError> {
        let mut res = Vec::new();
        'main_loop: for (i, page) in self.doc.pages()?.enumerate() {
            let page = page?;
            let pixmap = &self.page_pixmaps[i];
            let image = Image {
                width: pixmap.width(),
                height: pixmap.height(),
                rgb: pixmap.samples().to_vec(),
            };
            let text_page = page.to_text_page(TextPageOptions::empty()).unwrap();
            for block in text_page.blocks() {
                for line in block.lines() {
                    let s: String = line.chars().filter_map(|c| c.char()).collect();
                    if s.contains(text) {
                        let line_bounds = line.bounds();
                        res.push(image);
                        // Limit results.
                        if res.len() >= 10 {
                            break 'main_loop;
                        }
                        continue 'main_loop;
                    }
                }
            }
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::PdfSearcher;

    #[test]
    fn basic_test() {
        let searcher = PdfSearcher::new("16_integrales_a_parametres.pdf").unwrap();
        assert!(!searcher.search("extension").unwrap().is_empty());
    }
}
