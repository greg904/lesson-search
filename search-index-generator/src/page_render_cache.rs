//! A module to kep track of documents' pages' renders so that we don't have to regenerate every
//! time if the document hasn't changed.

use std::{
    collections::HashMap,
    io::{self, Read, Write},
};

pub(crate) type Digest = String;

/// An image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Image {
    pub digest: Digest,
    pub width: u16,
    pub height: u16,
}

/// A page number (counting starts from 0).
pub(crate) type PageNumber = u16;

/// Maps a page number to an image of the page.
pub(crate) struct PageImageMap(pub HashMap<PageNumber, Image>);

impl PageImageMap {
    pub(crate) fn new() -> Self {
        Self(HashMap::new())
    }
}

/// Maps a document's digest to a `PageImageMap`.
pub(crate) struct DocumentMap(pub HashMap<Digest, PageImageMap>);

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

impl DocumentMap {
    pub(crate) fn new() -> Self {
        Self(HashMap::new())
    }

    pub(crate) fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&(self.0.len() as u16).to_le_bytes())?;
        for (digest, pages) in self.0.iter() {
            w.write_all(&(digest.len() as u32).to_le_bytes())?;
            w.write_all(digest.as_bytes())?;
            w.write_all(&(pages.0.len() as u16).to_le_bytes())?;
            for (page_nr, image) in pages.0.iter() {
                w.write_all(&page_nr.to_le_bytes())?;
                w.write_all(&(image.digest.len() as u32).to_le_bytes())?;
                w.write_all(image.digest.as_bytes())?;
                w.write_all(&image.width.to_le_bytes())?;
                w.write_all(&image.height.to_le_bytes())?;
            }
        }
        Ok(())
    }

    pub(crate) fn deserialize<R: Read>(r: &mut R) -> io::Result<Self> {
        let doc_count = deserialize_u16(r)?;
        let mut docs = HashMap::with_capacity(doc_count as usize);
        for _ in 0..doc_count {
            let doc_digest = deserialize_string(r)?;
            let page_count = deserialize_u16(r)?;
            let mut pages = PageImageMap(HashMap::with_capacity(page_count as usize));
            for _ in 0..page_count {
                let page_nr = deserialize_u16(r)?;
                let image_digest = deserialize_string(r)?;
                let width = deserialize_u16(r)?;
                let height = deserialize_u16(r)?;
                pages.0.insert(
                    page_nr,
                    Image {
                        digest: image_digest,
                        width,
                        height,
                    },
                );
            }
            docs.insert(doc_digest, pages);
        }
        Ok(Self(docs))
    }
}
