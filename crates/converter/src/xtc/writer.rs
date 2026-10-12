//! Assembling pages into an XTC/XTCH container.

use super::encode::encode_page;
use super::{
    Format, ReadDirection, XtcError, CHAPTER_SIZE, HEADER_SIZE, METADATA_SIZE,
    PAGE_TABLE_ENTRY_SIZE,
};
use crate::gray::LevelImage;
use std::io::{self, Write};

/// Book-level information shown by the reader. Strings are truncated to fit
/// their fixed-size fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub author: String,
    pub publisher: String,
    /// BCP 47 tag such as "en-US" or "ja-JP".
    pub language: String,
    /// Seconds since the Unix epoch; 0 if unknown.
    pub create_time: u32,
    /// 0-based index of the cover page.
    pub cover_page: Option<u16>,
}

/// A chapter entry for the reader's chapter menu. Pages are 0-based and
/// `end_page` is inclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chapter {
    pub name: String,
    pub start_page: u16,
    pub end_page: u16,
}

struct EncodedPage {
    bytes: Vec<u8>,
    width: u16,
    height: u16,
}

/// Collects pages and writes them out as one file.
///
/// Pages are held in encoded form until [`XtcBuilder::build`], because the
/// page table precedes the data and needs every size. A 1000-page XTCH book
/// is about 96 MB in memory.
pub struct XtcBuilder {
    format: Format,
    direction: ReadDirection,
    metadata: Option<Metadata>,
    chapters: Vec<Chapter>,
    pages: Vec<EncodedPage>,
}

impl XtcBuilder {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            direction: ReadDirection::LeftToRight,
            metadata: None,
            chapters: Vec::new(),
            pages: Vec::new(),
        }
    }

    pub fn direction(mut self, direction: ReadDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn chapters(mut self, chapters: Vec<Chapter>) -> Self {
        self.chapters = chapters;
        self
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Encode and append a page. Its depth must match the container format.
    pub fn add_page(&mut self, page: &LevelImage) -> Result<(), XtcError> {
        if page.depth != self.format.depth() {
            return Err(XtcError::DepthMismatch {
                expected: self.format.depth(),
                found: page.depth,
            });
        }
        if self.pages.len() >= u16::MAX as usize {
            return Err(XtcError::TooManyPages(self.pages.len() + 1));
        }
        let bytes = encode_page(page)?;
        self.pages.push(EncodedPage {
            bytes,
            width: page.width as u16,
            height: page.height as u16,
        });
        Ok(())
    }

    /// Produce the finished file.
    pub fn build(self) -> Result<Vec<u8>, XtcError> {
        let mut out = Vec::new();
        self.write_to(&mut out).map_err(|e| match e {
            WriteError::Xtc(e) => e,
            WriteError::Io(_) => unreachable!("writing to a Vec cannot fail"),
        })?;
        Ok(out)
    }

    /// Write the finished file to any `Write`.
    pub fn write_to<W: Write>(self, w: &mut W) -> Result<(), WriteError> {
        let count = self.pages.len();
        if count == 0 {
            return Err(XtcError::NoPages.into());
        }
        for c in &self.chapters {
            if c.start_page > c.end_page || c.end_page as usize >= count {
                return Err(XtcError::BadChapter {
                    name: c.name.clone(),
                    start: c.start_page,
                    end: c.end_page,
                    pages: count,
                }
                .into());
            }
        }

        let has_meta = self.metadata.is_some();
        let has_chapters = !self.chapters.is_empty();
        let meta_offset = HEADER_SIZE;
        let chapter_offset = meta_offset + if has_meta { METADATA_SIZE } else { 0 };
        let table_offset = chapter_offset + self.chapters.len() * CHAPTER_SIZE;
        let data_offset = table_offset + count * PAGE_TABLE_ENTRY_SIZE;

        // Header (56 bytes).
        let mut head = Vec::with_capacity(data_offset);
        head.extend_from_slice(&self.format.magic().to_le_bytes());
        head.push(1); // version major
        head.push(0); // version minor
        head.extend_from_slice(&(count as u16).to_le_bytes());
        head.push(self.direction as u8);
        head.push(has_meta as u8);
        head.push(0); // hasThumbnails
        head.push(has_chapters as u8);
        head.extend_from_slice(&1u32.to_le_bytes()); // currentPage, 1-based
        head.extend_from_slice(&(if has_meta { meta_offset as u64 } else { 0 }).to_le_bytes());
        head.extend_from_slice(&(table_offset as u64).to_le_bytes());
        head.extend_from_slice(&(data_offset as u64).to_le_bytes());
        head.extend_from_slice(&0u64.to_le_bytes()); // thumbOffset
        head.extend_from_slice(
            &(if has_chapters {
                chapter_offset as u32
            } else {
                0
            })
            .to_le_bytes(),
        );
        head.extend_from_slice(&0u32.to_le_bytes()); // padding
        debug_assert_eq!(head.len(), HEADER_SIZE);

        // Metadata (256 bytes) right after the header.
        if let Some(m) = &self.metadata {
            put_str(&mut head, &m.title, 128);
            put_str(&mut head, &m.author, 64);
            put_str(&mut head, &m.publisher, 32);
            put_str(&mut head, &m.language, 16);
            head.extend_from_slice(&m.create_time.to_le_bytes());
            head.extend_from_slice(&m.cover_page.unwrap_or(0xFFFF).to_le_bytes());
            head.extend_from_slice(&(self.chapters.len() as u16).to_le_bytes());
            head.extend_from_slice(&0u64.to_le_bytes()); // reserved
            debug_assert_eq!(head.len(), HEADER_SIZE + METADATA_SIZE);
        }

        // Chapters (96 bytes each), packed directly before the page table.
        for c in &self.chapters {
            put_str(&mut head, &c.name, 80);
            head.extend_from_slice(&c.start_page.to_le_bytes());
            head.extend_from_slice(&c.end_page.to_le_bytes());
            head.extend_from_slice(&[0u8; 12]);
        }

        // Page table (16 bytes each).
        let mut offset = data_offset as u64;
        for p in &self.pages {
            head.extend_from_slice(&offset.to_le_bytes());
            head.extend_from_slice(&(p.bytes.len() as u32).to_le_bytes());
            head.extend_from_slice(&p.width.to_le_bytes());
            head.extend_from_slice(&p.height.to_le_bytes());
            offset += p.bytes.len() as u64;
        }
        debug_assert_eq!(head.len(), data_offset);

        w.write_all(&head)?;
        for p in &self.pages {
            w.write_all(&p.bytes)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(transparent)]
    Xtc(#[from] XtcError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Append `s` as a fixed-size UTF-8 field: truncated on a character boundary
/// to leave room for a terminating NUL, then zero-padded.
fn put_str(buf: &mut Vec<u8>, s: &str, len: usize) {
    let mut end = s.len().min(len - 1);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    buf.extend_from_slice(&s.as_bytes()[..end]);
    buf.resize(buf.len() + (len - end), 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gray::Depth;

    fn blank(depth: Depth) -> LevelImage {
        LevelImage {
            width: 8,
            height: 8,
            depth,
            levels: vec![depth.max_level(); 64],
        }
    }

    #[test]
    fn put_str_truncates_on_char_boundary_and_pads() {
        let mut b = Vec::new();
        put_str(&mut b, "héllo", 4); // 'é' is two bytes: "h" + 'é' would need 3, then NUL
        assert_eq!(b.len(), 4);
        assert_eq!(&b[..3], "hé".as_bytes());
        assert_eq!(b[3], 0);

        let mut b = Vec::new();
        put_str(&mut b, "日本語", 5); // each char is 3 bytes; only one fits in 4
        assert_eq!(b.len(), 5);
        assert_eq!(&b[..3], "日".as_bytes());
        assert_eq!(&b[3..], &[0, 0]);
    }

    #[test]
    fn rejects_empty_mismatched_and_bad_chapters() {
        let b = XtcBuilder::new(Format::Xtc);
        assert_eq!(b.build(), Err(XtcError::NoPages));

        let mut b = XtcBuilder::new(Format::Xtc);
        assert!(matches!(
            b.add_page(&blank(Depth::Two)),
            Err(XtcError::DepthMismatch { .. })
        ));

        let mut b = XtcBuilder::new(Format::Xtc).chapters(vec![Chapter {
            name: "x".into(),
            start_page: 0,
            end_page: 5,
        }]);
        b.add_page(&blank(Depth::One)).unwrap();
        assert!(matches!(b.build(), Err(XtcError::BadChapter { .. })));
    }

    #[test]
    fn header_fields_are_at_the_documented_offsets() {
        let mut b = XtcBuilder::new(Format::Xtch)
            .direction(ReadDirection::RightToLeft)
            .metadata(Metadata {
                title: "T".into(),
                author: "A".into(),
                ..Default::default()
            })
            .chapters(vec![Chapter {
                name: "C1".into(),
                start_page: 0,
                end_page: 1,
            }]);
        b.add_page(&blank(Depth::Two)).unwrap();
        b.add_page(&blank(Depth::Two)).unwrap();
        let f = b.build().unwrap();

        assert_eq!(&f[0..4], b"XTCH");
        assert_eq!(f[4], 1); // version major
        assert_eq!(f[5], 0); // version minor
        assert_eq!(u16::from_le_bytes([f[6], f[7]]), 2); // pageCount
        assert_eq!(f[8], 1); // readDirection: RTL
        assert_eq!((f[9], f[10], f[11]), (1, 0, 1)); // meta, thumbs, chapters
        let u64_at = |o: usize| u64::from_le_bytes(f[o..o + 8].try_into().unwrap());
        assert_eq!(u64_at(0x10), 56); // metadataOffset
        let table = 56 + 256 + 96;
        assert_eq!(u64_at(0x18), table as u64); // pageTableOffset
        assert_eq!(u64_at(0x20), (table + 32) as u64); // dataOffset
        assert_eq!(u64_at(0x28), 0); // thumbOffset
                                     // chapterOffset is read as u64 by CrossPoint; the padding must be zero.
        assert_eq!(u64_at(0x30), 56 + 256);
        // Title and author at the absolute offsets CrossPoint uses.
        assert_eq!(f[0x38], b'T');
        assert_eq!(f[0xB8], b'A');
    }
}
