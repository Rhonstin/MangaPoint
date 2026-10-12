//! MangaPoint conversion engine: pixels in, XTC/XTCH bytes out.
//!
//! This crate does no I/O, networking or UI, so the same code runs in the
//! desktop app and (compiled to WASM) in the website.

#[cfg(feature = "decode")]
pub mod decode;
pub mod dither;
pub mod gray;
pub mod process;
pub mod testpattern;
pub mod xtc;

pub use dither::{quantize, Dither};
pub use gray::{Depth, GrayImage, ImageError, LevelImage};
pub use process::{process_page, CropMode, Layout, ProcessOptions, SpreadMode};
