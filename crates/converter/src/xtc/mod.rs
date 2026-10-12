//! The XTC/XTCH container and its XTG/XTH page formats (Xteink X-series).
//!
//! Layout decisions here follow CrossPoint Reader's parser
//! (`lib/Xtc/Xtc/XtcParser.cpp`), which is the reference we must satisfy:
//!
//! ```text
//! 0x00  header (56 bytes)
//! 0x38  metadata (256 bytes, optional; CrossPoint reads title at 0x38 and
//!       author at 0xB8 regardless of metadataOffset)
//!       chapters (96 bytes each, optional; count is derived from the gap
//!       between chapterOffset and pageTableOffset, so no padding allowed)
//!       page table (16 bytes per page)
//!       page data (22-byte XTG/XTH header + bitmap, per page)
//! ```
//!
//! All integers are little-endian.

mod encode;
mod parser;
mod writer;

use crate::gray::Depth;
use thiserror::Error;

pub use encode::{decode_page, encode_page, xtg_data_size, xth_data_size, DecodedPage};
pub use parser::{ChapterInfo, Header, PageInfo, XtcFile};
pub use writer::{Chapter, Metadata, XtcBuilder};

pub const XTC_MAGIC: u32 = 0x0043_5458; // "XTC\0"
pub const XTCH_MAGIC: u32 = 0x4843_5458; // "XTCH"
pub const XTG_MAGIC: u32 = 0x0047_5458; // "XTG\0"
pub const XTH_MAGIC: u32 = 0x0048_5458; // "XTH\0"

pub const HEADER_SIZE: usize = 56;
pub const PAGE_HEADER_SIZE: usize = 22;
pub const PAGE_TABLE_ENTRY_SIZE: usize = 16;
pub const METADATA_SIZE: usize = 256;
pub const CHAPTER_SIZE: usize = 96;

/// Offsets inside the metadata block (CrossPoint reads these absolutely).
pub const TITLE_OFFSET: usize = 0x38;
pub const AUTHOR_OFFSET: usize = 0xB8;
pub const TITLE_LEN: usize = 128;
pub const AUTHOR_LEN: usize = 64;

/// Mapping between brightness level (0 = black .. 3 = white) and the 2-bit
/// value stored in an XTH page: `stored = GRAY_MAP[level]`.
///
/// The community spec says stored 0 = white, 1 = dark gray, 2 = light gray,
/// 3 = black. The table is its own inverse, so it also decodes. This is the
/// single place to change if a hardware test shows a different mapping.
pub const GRAY_MAP: [u8; 4] = [3, 1, 2, 0];

/// Which flavour of container to write.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// `.xtc`: 1-bit pages.
    Xtc,
    /// `.xtch`: 2-bit pages with four gray levels.
    Xtch,
}

impl Format {
    pub fn depth(self) -> Depth {
        match self {
            Format::Xtc => Depth::One,
            Format::Xtch => Depth::Two,
        }
    }

    pub fn magic(self) -> u32 {
        match self {
            Format::Xtc => XTC_MAGIC,
            Format::Xtch => XTCH_MAGIC,
        }
    }

    pub fn page_magic(self) -> u32 {
        match self {
            Format::Xtc => XTG_MAGIC,
            Format::Xtch => XTH_MAGIC,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Xtc => "xtc",
            Format::Xtch => "xtch",
        }
    }
}

/// Value of the header's `readDirection` byte.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadDirection {
    LeftToRight = 0,
    /// Manga.
    RightToLeft = 1,
    TopToBottom = 2,
}

impl ReadDirection {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::LeftToRight),
            1 => Some(Self::RightToLeft),
            2 => Some(Self::TopToBottom),
            _ => None,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum XtcError {
    #[error("page dimensions {width}x{height} are outside 1..=65535")]
    BadDimensions { width: u32, height: u32 },
    #[error("page has {found} levels but {width}x{height} needs {expected}")]
    BadPixelCount {
        width: u32,
        height: u32,
        expected: usize,
        found: usize,
    },
    #[error("page depth {found:?} does not match container format ({expected:?})")]
    DepthMismatch { expected: Depth, found: Depth },
    #[error("an XTC file holds at most 65535 pages (got {0})")]
    TooManyPages(usize),
    #[error("could not use the temporary page file: {0}")]
    Spool(String),
    #[error("a file needs at least one page")]
    NoPages,
    #[error("chapter '{name}' covers pages {start}..={end} but the file has {pages} pages")]
    BadChapter {
        name: String,
        start: u16,
        end: u16,
        pages: usize,
    },
    #[error("file is truncated: {0}")]
    Truncated(&'static str),
    #[error("invalid magic 0x{0:08X}")]
    InvalidMagic(u32),
    #[error("unsupported version {major}.{minor}")]
    InvalidVersion { major: u8, minor: u8 },
    #[error("corrupted header: {0}")]
    CorruptedHeader(&'static str),
    #[error("page {index} out of range (file has {count})")]
    PageOutOfRange { index: usize, count: usize },
    #[error("invalid page {index}: {reason}")]
    InvalidPage { index: usize, reason: &'static str },
}
