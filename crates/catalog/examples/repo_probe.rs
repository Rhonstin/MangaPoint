//! Try extension repository URLs against a real server and report which
//! ones actually load extensions.
//!
//!   cargo run -p catalog --example repo_probe -- <work-dir> <url> [<url>...]
//!
//! `<work-dir>` must already hold a provisioned Suwayomi (run `smoke` once).

use catalog::provision::locate_install;
use catalog::{RunningServer, ServerConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let work: PathBuf = args
        .next()
        .ok_or("usage: repo_probe <work-dir> <url>...")?
        .into();
    let urls: Vec<String> = args.collect();

    let install = locate_install(&work.join("suwayomi")).or_else(|_| {
        locate_install(
            &work
                .join("suwayomi")
                .join(format!("suwayomi-{}", catalog::release::VERSION_TAG)),
        )
    })?;
    let config = ServerConfig {
        install,
        // PROBE_DATA lets you start from a brand-new data folder each time.
        data_dir: work.join(std::env::var("PROBE_DATA").unwrap_or_else(|_| "probe-data".into())),
        browser_support: false,
    };
    let server = RunningServer::start(&config).await?;
    for url in &urls {
        let before = server.log_tail(1 << 20).len();
        server.client.add_extension_store(url).await?;
        // The server may sync the repo in the background, so poll for a while.
        let mut count = 0;
        let mut waited = 0;
        while waited <= 20 {
            count = server.client.fetch_extensions().await?.len();
            if count > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            waited += 2;
        }
        println!("{count:>5} extensions after {waited}s  {url}");
        let log = server.log_tail(1 << 20);
        let new = &log[before.min(log.len())..];
        for line in new
            .lines()
            .filter(|l| l.contains("ExtensionStoreService") || l.contains("HttpException"))
        {
            println!(
                "        log: {}",
                line.chars().take(200).collect::<String>()
            );
        }
    }
    server.stop().await?;
    Ok(())
}
