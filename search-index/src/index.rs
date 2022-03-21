use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
};

fn deserialize_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn deserialize_f32<R: Read>(r: &mut R) -> io::Result<f32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(f32::from_le_bytes(buf))
}

fn deserialize_u16<R: Read>(r: &mut R) -> io::Result<u16> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn deserialize_i16<R: Read>(r: &mut R) -> io::Result<i16> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(i16::from_le_bytes(buf))
}

fn deserialize_string<R: Read>(r: &mut R) -> io::Result<String> {
    let len = deserialize_u32(r)?;

    let mut s = vec![0u8; len as usize];
    r.read_exact(&mut s)?;

    String::from_utf8(s).map_err(|_err| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))
}

fn deserialize_vec_string<R: Read>(r: &mut R) -> io::Result<Vec<String>> {
    let count = deserialize_u32(r)?;

    let mut vec = Vec::with_capacity(count as usize);
    for _ in 0..count {
        vec.push(deserialize_string(r)?);
    }

    Ok(vec)
}

#[derive(Clone)]
pub struct Page {
    pub document_index: u16,
    pub page_nr: u16,
    pub rendered_avif: String,
    pub rendered_jpeg: String,
    pub width: u16,
    pub height: u16,
}

impl Page {
    fn deserialize<R: Read>(r: &mut R) -> io::Result<Self> {
        let document_index = deserialize_u16(r)?;
        let page_nr = deserialize_u16(r)?;
        let rendered_avif = deserialize_string(r)?;
        let rendered_jpeg = deserialize_string(r)?;
        let width = deserialize_u16(r)?;
        let height = deserialize_u16(r)?;

        Ok(Self {
            document_index,
            page_nr,
            rendered_avif,
            rendered_jpeg,
            width,
            height,
        })
    }
}

#[derive(Clone)]
pub struct SearchResult {
    pub page_index: u32,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl SearchResult {
    fn deserialize<R: Read>(r: &mut R) -> io::Result<SearchResult> {
        let page_index = deserialize_u32(r)?;
        let x = deserialize_i16(r)?;
        let y = deserialize_i16(r)?;
        let width = deserialize_u16(r)?;
        let height = deserialize_u16(r)?;

        Ok(SearchResult {
            page_index,
            x,
            y,
            width,
            height,
        })
    }
}

#[derive(Clone)]
pub struct Match {
    pub result_index: u32,
    pub score: f32,
}

impl Match {
    fn deserialize<R: Read>(r: &mut R) -> io::Result<Match> {
        let result_index = deserialize_u32(r)?;
        let score = deserialize_f32(r)?;

        Ok(Match {
            result_index,
            score,
        })
    }
}

#[derive(Clone, Default)]
pub struct SearchIndex {
    pub documents: Vec<String>,
    pub pages: Vec<Page>,
    pub results: Vec<SearchResult>,
    pub words: BTreeMap<String, Vec<Match>>,
}

impl SearchIndex {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn deserialize<R: Read>(r: &mut R) -> io::Result<Self> {
        let documents = deserialize_vec_string(r)?;

        let page_count = deserialize_u32(r)?;
        let mut pages = Vec::with_capacity(page_count as usize);
        for _ in 0..page_count {
            pages.push(Page::deserialize(r)?);
        }

        let result_count = deserialize_u32(r)?;
        let mut results = Vec::with_capacity(result_count as usize);
        for _ in 0..result_count {
            results.push(SearchResult::deserialize(r)?);
        }

        let word_count = deserialize_u32(r)?;
        let mut words = BTreeMap::new();
        for _ in 0..word_count {
            let word = deserialize_string(r)?;

            let match_count = deserialize_u32(r)?;
            let mut matches = Vec::with_capacity(match_count as usize);
            for _ in 0..match_count {
                matches.push(Match::deserialize(r)?);
            }

            words.insert(word, matches);
        }

        Ok(Self {
            documents,
            pages,
            results,
            words,
        })
    }

    pub fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&(self.documents.len() as u32).to_le_bytes())?;
        for doc in self.documents.iter() {
            w.write_all(&(doc.len() as u32).to_le_bytes())?;
            w.write_all(doc.as_bytes())?;
        }

        w.write_all(&(self.pages.len() as u32).to_le_bytes())?;
        for page in self.pages.iter() {
            w.write_all(&page.document_index.to_le_bytes())?;
            w.write_all(&page.page_nr.to_le_bytes())?;
            w.write_all(&(page.rendered_avif.len() as u32).to_le_bytes())?;
            w.write_all(page.rendered_avif.as_bytes())?;
            w.write_all(&(page.rendered_jpeg.len() as u32).to_le_bytes())?;
            w.write_all(page.rendered_jpeg.as_bytes())?;
            w.write_all(&page.width.to_le_bytes())?;
            w.write_all(&page.height.to_le_bytes())?;
        }

        w.write_all(&(self.results.len() as u32).to_le_bytes())?;
        for r in self.results.iter() {
            w.write_all(&r.page_index.to_le_bytes())?;
            w.write_all(&r.x.to_le_bytes())?;
            w.write_all(&r.y.to_le_bytes())?;
            w.write_all(&r.width.to_le_bytes())?;
            w.write_all(&r.height.to_le_bytes())?;
        }

        w.write_all(&(self.words.len() as u32).to_le_bytes())?;
        for (word, matches) in self.words.iter() {
            w.write_all(&(word.len() as u32).to_le_bytes())?;
            w.write_all(word.as_bytes())?;
            w.write_all(&(matches.len() as u32).to_le_bytes())?;
            for m in matches.iter() {
                w.write_all(&m.result_index.to_le_bytes())?;
                w.write_all(&m.score.to_le_bytes())?;
            }
        }

        Ok(())
    }
}
