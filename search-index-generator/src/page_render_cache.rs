//! A container to keep track of what page correspond to what image. This is so that we don't have
//! to encode the rendered pages over and over when building the search index: we can reuse
//! previously rendered pages' images.

use std::{collections::HashMap, io::{Read, self, Write}};

#[derive(Clone)]
pub(crate) struct CachedPage {
    pub image_id: String,
    pub width: u16,
    pub height: u16,
}

pub(crate) type PageRenderCache = HashMap<u16, CachedPage>;

pub(crate) struct DocumentRenderCache {
    pub by_path: HashMap<String, PageRenderCache>,
}

fn deserialize_u16<R: Read>(r: &mut R) -> io::Result<u16> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn deserialize_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn deserialize_string<R: Read>(r: &mut R) -> io::Result<String> {
    let len = deserialize_u32(r)?;

    let mut s = vec![0u8; len as usize];
    r.read_exact(&mut s)?;

    String::from_utf8(s).map_err(|_err| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))
}

impl DocumentRenderCache {
    pub(crate) fn new() -> Self {
        Self {
            by_path: HashMap::new(),
        }
    }

    pub(crate) fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&(self.by_path.len() as u16).to_le_bytes())?;
        for (path, pages) in self.by_path.iter() {
            w.write_all(&(path.len() as u32).to_le_bytes())?;
            w.write_all(path.as_bytes())?;
            w.write_all(&(pages.len() as u16).to_le_bytes())?;
            for (page_nr, page) in pages.iter() {
                w.write_all(&page_nr.to_le_bytes())?;
                w.write_all(&(page.image_id.len() as u32).to_le_bytes())?;
                w.write_all(page.image_id.as_bytes())?;
                w.write_all(&page.width.to_le_bytes())?;
                w.write_all(&page.height.to_le_bytes())?;
            }
        }
        Ok(())
    }

    pub(crate) fn deserialize<R: Read>(r: &mut R) -> io::Result<Self> {
        let doc_count = deserialize_u16(r)?;
        let mut by_path = HashMap::with_capacity(doc_count as usize);
        for _ in 0..doc_count {
            let path = deserialize_string(r)?;
            let page_count = deserialize_u16(r)?;
            let mut page_cache = HashMap::with_capacity(page_count as usize);
            for _ in 0..page_count {
                let page_nr = deserialize_u16(r)?;
                let image_id = deserialize_string(r)?;
                let width = deserialize_u16(r)?;
                let height = deserialize_u16(r)?;
                page_cache.insert(
                    page_nr,
                    CachedPage {
                        image_id,
                        width,
                        height,
                    },
                );
            }
            by_path.insert(path, page_cache);
        }
        Ok(Self { by_path })
    }
}
