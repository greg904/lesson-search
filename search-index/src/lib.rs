use std::{
    collections::HashMap,
    io::{self, Read, Write},
};

pub struct SearchResult {
    pub image_index: u32,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

pub struct Match {
    pub result_index: u32,
    pub score: f32,
}

pub struct SearchIndex {
    pub image_ids: Vec<String>,
    pub results: Vec<SearchResult>,
    pub words: HashMap<String, Vec<Match>>,
}

impl SearchIndex {
    pub fn new() -> Self {
        Self {
            image_ids: Vec::new(),
            results: Vec::new(),
            words: HashMap::new(),
        }
    }

    pub fn deserialize<R: Read>(r: &mut R) -> io::Result<Self> {
        let mut buf = [0u8; 4];
        r.read_exact(&mut buf)?;
        let image_ids_len = u32::from_le_bytes(buf);
        let mut image_ids = Vec::with_capacity(image_ids_len as usize);
        for _ in 0..image_ids_len {
            r.read_exact(&mut buf)?;
            let s_len = u32::from_le_bytes(buf);
            let mut s = vec![0u8; s_len as usize];
            r.read_exact(&mut s)?;
            image_ids.push(
                std::str::from_utf8(&s)
                    .map_err(|_err| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))?
                    .to_owned(),
            );
        }
        r.read_exact(&mut buf)?;
        let results_len = u32::from_le_bytes(buf);
        let mut results = Vec::with_capacity(results_len as usize);
        for _ in 0..results_len {
            r.read_exact(&mut buf)?;
            let image_index = u32::from_le_bytes(buf);
            let mut buf = [0u8; 2];
            r.read_exact(&mut buf)?;
            let x = i16::from_le_bytes(buf);
            r.read_exact(&mut buf)?;
            let y = i16::from_le_bytes(buf);
            r.read_exact(&mut buf)?;
            let width = u16::from_le_bytes(buf);
            r.read_exact(&mut buf)?;
            let height = u16::from_le_bytes(buf);
            results.push(SearchResult { image_index, x, y, width, height });
        }
        r.read_exact(&mut buf)?;
        let words_len = u32::from_le_bytes(buf);
        let mut words = HashMap::with_capacity(words_len as usize);
        for _ in 0..words_len {
            r.read_exact(&mut buf)?;
            let word_len = u32::from_le_bytes(buf);
            let mut word = vec![0u8; word_len as usize];
            r.read_exact(&mut word)?;
            r.read_exact(&mut buf)?;
            let matches_len = u32::from_le_bytes(buf);
            let mut matches = Vec::with_capacity(matches_len as usize);
            for _ in 0..matches_len {
                r.read_exact(&mut buf)?;
                let result_index = u32::from_le_bytes(buf);
                r.read_exact(&mut buf)?;
                let score = f32::from_le_bytes(buf);
                matches.push(Match {
                    result_index,
                    score,
                });
            }
            words.insert(
                std::str::from_utf8(&word)
                    .map_err(|_err| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))?
                    .to_owned(),
                matches,
            );
        }
        Ok(Self {
            image_ids,
            results,
            words,
        })
    }

    pub fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&(self.image_ids.len() as u32).to_le_bytes())?;
        for id in self.image_ids.iter() {
            w.write_all(&(id.len() as u32).to_le_bytes())?;
            w.write_all(id.as_bytes())?;
        }
        w.write_all(&(self.results.len() as u32).to_le_bytes())?;
        for r in self.results.iter() {
            w.write_all(&r.image_index.to_le_bytes())?;
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
