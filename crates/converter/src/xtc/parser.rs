//! Reading XTC/XTCH files, mirroring CrossPoint Reader's `XtcParser`.
//!
//! The point of this module is to be a stand-in for the device: anything we
//! write must be accepted here, and the checks below deliberately follow the
//! firmware's (including its quirks) rather than being more lenient.

use super::encode::{decode_page, DecodedPage};
use super::{
    Format, ReadDirection, XtcError, AUTHOR_LEN, AUTHOR_OFFSET, CHAPTER_SIZE, HEADER_SIZE,
    PAGE_TABLE_ENTRY_SIZE, TITLE_LEN, TITLE_OFFSET, XTCH_MAGIC, XTC_MAGIC,
};

/// Older files started the page table right after a 48-byte header.
const LEGACY_HEADER_SIZE: u64 = 0x30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub magic: u32,
    pub version_major: u8,
    pub version_minor: u8,
    pub page_count: u16,
    pub read_direction: u8,
    pub has_metadata: bool,
    pub has_thumbnails: bool,
    pub has_chapters: bool,
    pub current_page: u32,
    pub metadata_offset: u64,
    pub page_table_offset: u64,
    pub data_offset: u64,
    pub thumb_offset: u64,
    pub chapter_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageInfo {
    pub offset: u64,
    pub size: u32,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterInfo {
    pub name: String,
    pub start_page: u16,
    pub end_page: u16,
}

/// A parsed file borrowing its bytes.
pub struct XtcFile<'a> {
    data: &'a [u8],
    pub header: Header,
    pub format: Format,
    pub title: String,
    pub author: String,
    pub chapters: Vec<ChapterInfo>,
}

fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(d[o..o + 2].try_into().unwrap())
}
fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
fn u64_at(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(d[o..o + 8].try_into().unwrap())
}

fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

impl<'a> XtcFile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, XtcError> {
        if data.len() < HEADER_SIZE {
            return Err(XtcError::Truncated("shorter than the 56-byte header"));
        }
        let magic = u32_at(data, 0);
        let format = match magic {
            XTC_MAGIC => Format::Xtc,
            XTCH_MAGIC => Format::Xtch,
            other => return Err(XtcError::InvalidMagic(other)),
        };
        let (major, minor) = (data[4], data[5]);
        // The firmware accepts 1.0 and the byte-swapped 0.1.
        if !((major == 1 && minor == 0) || (major == 0 && minor == 1)) {
            return Err(XtcError::InvalidVersion { major, minor });
        }
        let page_count = u16_at(data, 6);
        if page_count == 0 {
            return Err(XtcError::CorruptedHeader("page count is zero"));
        }
        let header = Header {
            magic,
            version_major: major,
            version_minor: minor,
            page_count,
            read_direction: data[8],
            has_metadata: data[9] == 1,
            has_thumbnails: data[10] == 1,
            has_chapters: data[11] == 1,
            current_page: u32_at(data, 0x0C),
            metadata_offset: u64_at(data, 0x10),
            page_table_offset: u64_at(data, 0x18),
            data_offset: u64_at(data, 0x20),
            thumb_offset: u64_at(data, 0x28),
            // Read as 8 bytes, like readChapters(); the 4 padding bytes must be 0.
            chapter_offset: u64_at(data, 0x30),
        };

        let file_len = data.len() as u64;
        let table_bytes = page_count as u64 * PAGE_TABLE_ENTRY_SIZE as u64;
        if header.page_table_offset == 0 {
            return Err(XtcError::CorruptedHeader("page table offset is 0"));
        }
        if header.page_table_offset < LEGACY_HEADER_SIZE
            || header.page_table_offset > file_len
            || table_bytes > file_len - header.page_table_offset
        {
            return Err(XtcError::CorruptedHeader("page table exceeds file bounds"));
        }

        // The firmware reads these at fixed offsets whenever hasMetadata is set.
        let (title, author) = if header.has_metadata {
            if data.len() < AUTHOR_OFFSET + AUTHOR_LEN {
                return Err(XtcError::Truncated("metadata block"));
            }
            (
                c_string(&data[TITLE_OFFSET..TITLE_OFFSET + TITLE_LEN - 1]),
                c_string(&data[AUTHOR_OFFSET..AUTHOR_OFFSET + AUTHOR_LEN - 1]),
            )
        } else {
            (String::new(), String::new())
        };

        let mut file = XtcFile {
            data,
            header,
            format,
            title,
            author,
            chapters: Vec::new(),
        };
        file.chapters = file.read_chapters();
        Ok(file)
    }

    pub fn page_count(&self) -> usize {
        self.header.page_count as usize
    }

    pub fn read_direction(&self) -> Option<ReadDirection> {
        ReadDirection::from_byte(self.header.read_direction)
    }

    pub fn page_info(&self, index: usize) -> Result<PageInfo, XtcError> {
        if index >= self.page_count() {
            return Err(XtcError::PageOutOfRange {
                index,
                count: self.page_count(),
            });
        }
        let at = self.header.page_table_offset as usize + index * PAGE_TABLE_ENTRY_SIZE;
        Ok(PageInfo {
            offset: u64_at(self.data, at),
            size: u32_at(self.data, at + 8),
            width: u16_at(self.data, at + 12),
            height: u16_at(self.data, at + 14),
        })
    }

    /// Raw page bytes (22-byte page header plus bitmap).
    pub fn page_bytes(&self, index: usize) -> Result<&'a [u8], XtcError> {
        let info = self.page_info(index)?;
        let start = info.offset as usize;
        let end = start
            .checked_add(info.size as usize)
            .filter(|&e| e <= self.data.len())
            .ok_or(XtcError::InvalidPage {
                index,
                reason: "page lies outside the file",
            })?;
        Ok(&self.data[start..end])
    }

    pub fn decode_page(&self, index: usize) -> Result<DecodedPage, XtcError> {
        let decoded = decode_page(self.page_bytes(index)?, index)?;
        // The firmware expects XTG pages in an XTC file and XTH in an XTCH file.
        if decoded.image.depth != self.format.depth() {
            return Err(XtcError::InvalidPage {
                index,
                reason: "page type does not match the container type",
            });
        }
        Ok(decoded)
    }

    /// Chapter parsing as done by `XtcParser::readChapters`: the count comes
    /// from the space between the chapter offset and the page table.
    fn read_chapters(&self) -> Vec<ChapterInfo> {
        let h = &self.header;
        let file_len = self.data.len() as u64;
        // Legacy files have no chapterOffset field.
        if !h.has_chapters
            || h.page_table_offset < HEADER_SIZE as u64
            || h.chapter_offset == 0
            || h.chapter_offset < HEADER_SIZE as u64
            || h.chapter_offset >= file_len
            || h.chapter_offset + CHAPTER_SIZE as u64 > file_len
        {
            return Vec::new();
        }
        let max_offset =
            if h.page_table_offset > h.chapter_offset && h.page_table_offset <= file_len {
                h.page_table_offset
            } else if h.data_offset > h.chapter_offset && h.data_offset <= file_len {
                h.data_offset
            } else {
                file_len
            };
        if max_offset <= h.chapter_offset {
            return Vec::new();
        }
        let count = ((max_offset - h.chapter_offset) / CHAPTER_SIZE as u64) as usize;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let at = h.chapter_offset as usize + i * CHAPTER_SIZE;
            let entry = &self.data[at..at + CHAPTER_SIZE];
            let start = u16_at(entry, 0x50);
            let mut end = u16_at(entry, 0x52);
            if start >= h.page_count {
                continue;
            }
            if end >= h.page_count {
                end = h.page_count - 1;
            }
            out.push(ChapterInfo {
                name: c_string(&entry[..80]),
                start_page: start,
                end_page: end,
            });
        }
        out
    }
}
