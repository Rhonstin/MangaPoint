//! The operations the rest of the app needs from a manga catalog.
//!
//! [`SuwayomiClient`] is the real implementation. Everything above this
//! crate (sources, search, pipeline) talks to the trait, which keeps that
//! logic testable without a server and lets another backend be added later.

use crate::client::{Browse, BrowsePage, Chapter, Extension, MangaDetails, Source, SuwayomiClient};
use crate::error::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Catalog: Send + Sync {
    /// Refresh and list the extensions offered by the configured stores.
    async fn fetch_extensions(&self) -> Result<Vec<Extension>>;
    async fn install_extension(&self, pkg_name: &str) -> Result<()>;
    async fn uninstall_extension(&self, pkg_name: &str) -> Result<()>;
    async fn add_extension_store(&self, index_url: &str) -> Result<()>;
    /// Sources provided by the installed extensions.
    async fn list_sources(&self) -> Result<Vec<Source>>;
    async fn browse(&self, source_id: &str, kind: &Browse, page: u32) -> Result<BrowsePage>;
    async fn fetch_chapters(&self, manga_id: i64) -> Result<Vec<Chapter>>;
    /// The series a chapter belongs to, with its description and cover.
    async fn manga_of_chapter(&self, chapter_id: i64) -> Result<MangaDetails>;
    /// A series by its id, with its description and cover.
    async fn manga_details(&self, manga_id: i64) -> Result<MangaDetails>;
    /// Paths of the page images of a chapter, usable with [`Catalog::get_bytes`].
    async fn chapter_pages(&self, chapter_id: i64) -> Result<Vec<String>>;
    /// Fetch a page image or cover by the path the catalog gave out.
    async fn get_bytes(&self, path: &str) -> Result<Vec<u8>>;
}

#[async_trait]
impl Catalog for SuwayomiClient {
    async fn fetch_extensions(&self) -> Result<Vec<Extension>> {
        SuwayomiClient::fetch_extensions(self).await
    }
    async fn install_extension(&self, pkg_name: &str) -> Result<()> {
        SuwayomiClient::install_extension(self, pkg_name).await
    }
    async fn uninstall_extension(&self, pkg_name: &str) -> Result<()> {
        SuwayomiClient::uninstall_extension(self, pkg_name).await
    }
    async fn add_extension_store(&self, index_url: &str) -> Result<()> {
        SuwayomiClient::add_extension_store(self, index_url).await
    }
    async fn list_sources(&self) -> Result<Vec<Source>> {
        SuwayomiClient::list_sources(self).await
    }
    async fn browse(&self, source_id: &str, kind: &Browse, page: u32) -> Result<BrowsePage> {
        SuwayomiClient::browse(self, source_id, kind, page).await
    }
    async fn fetch_chapters(&self, manga_id: i64) -> Result<Vec<Chapter>> {
        SuwayomiClient::fetch_chapters(self, manga_id).await
    }
    async fn manga_of_chapter(&self, chapter_id: i64) -> Result<MangaDetails> {
        SuwayomiClient::manga_of_chapter(self, chapter_id).await
    }
    async fn manga_details(&self, manga_id: i64) -> Result<MangaDetails> {
        SuwayomiClient::manga_details(self, manga_id).await
    }
    async fn chapter_pages(&self, chapter_id: i64) -> Result<Vec<String>> {
        SuwayomiClient::chapter_pages(self, chapter_id).await
    }
    async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        SuwayomiClient::get_bytes(self, path).await
    }
}
