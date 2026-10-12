//! Live check of the series-details query.
//!
//!   cargo run -p pipeline --example live_details -- <suwayomi-dir> <data-dir> <source name> "<title>"
//!
//! Finds the series on the source, then asks for its details through one of
//! its chapters, the way the Library does for a series downloaded long ago.

use catalog::provision::locate_install;
use catalog::release::VERSION_TAG;
use catalog::{Browse, RunningServer, ServerConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        return Err("usage: live_details <suwayomi-dir> <data-dir> <source> <title>".into());
    }
    let parent = PathBuf::from(&args[0]);
    let install = locate_install(&parent.join(format!("suwayomi-{VERSION_TAG}")))
        .or_else(|_| locate_install(&parent))?;
    let server = RunningServer::start(&ServerConfig {
        install,
        data_dir: PathBuf::from(&args[1]),
        browser_support: false,
    })
    .await?;
    let client = &server.client;
    let sources = client.list_sources().await?;
    let source = sources
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(&args[2]))
        .ok_or("source not installed")?;
    let found = client
        .browse(&source.id, &Browse::Search(args[3].clone()), 1)
        .await?;
    let manga = found.mangas.first().ok_or("no results")?;
    let chapters = client.fetch_chapters(manga.id).await?;
    let first = chapters.first().ok_or("no chapters")?;
    let details = client.manga_of_chapter(first.id).await?;
    println!("title:       {}", details.title);
    println!("status:      {:?}", details.status);
    println!("author:      {:?}", details.author);
    println!("artist:      {:?}", details.artist);
    println!("genres:      {:?}", details.genre);
    println!(
        "description: {} characters",
        details.description.as_deref().map_or(0, str::len)
    );
    println!("cover path:  {:?}", details.thumbnail_url);
    if let Some(path) = &details.thumbnail_url {
        let bytes = client.get_bytes(path).await?;
        println!(
            "cover bytes: {} ({:?})",
            bytes.len(),
            converter::decode::image_dimensions(&bytes)
        );
    }
    let _ = server.stop().await;
    Ok(())
}
