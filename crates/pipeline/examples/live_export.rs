//! Live end-to-end check against real sources (needs network and a
//! provisioned Suwayomi, for example the one the desktop app installed).
//!
//!   cargo run -p pipeline --example live_export -- <suwayomi-parent-dir> <work-dir> "<query>" [chapters]
//!
//! Searches the query on a few sources, ranks the results, merges chapter
//! lists, downloads the first few chapters, converts them, writes an XTCH
//! file into `<work-dir>/out`, then re-reads the file with the parser that
//! mirrors the reader's.

use catalog::provision::locate_install;
use catalog::release::VERSION_TAG;
use catalog::{RunningServer, ServerConfig};
use converter::xtc::XtcFile;
use futures_util::StreamExt;
use library::{Choice, HealthDb, Settings, SourceState};
use pipeline::{
    requests_from_merged, run_job, CancelFlag, ExportTarget, JobEvent, JobSpec, PageCache,
    RetryPolicy,
};
use search::{
    enrich, merge_chapters, missing_numbers, rank_series, search_stream, RankContext, SearchState,
};
use sources::SourceManager;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const STARTERS: &[&str] = &[
    "en.mangapill",
    "all.mangafire",
    "en.mangabuddy",
    "en.mangakatana",
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let suwayomi_parent: PathBuf = args
        .next()
        .ok_or("usage: live_export <suwayomi-dir> <work-dir> <query> [chapters]")?
        .into();
    let work: PathBuf = args.next().ok_or("missing work dir")?.into();
    let query = args.next().ok_or("missing query")?;
    let want_chapters: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(2);

    let install = locate_install(&suwayomi_parent.join(format!("suwayomi-{VERSION_TAG}")))
        .or_else(|_| locate_install(&suwayomi_parent))?;
    std::fs::create_dir_all(&work)?;
    let config = ServerConfig {
        install,
        data_dir: work.join("server-data"),
        browser_support: false,
    };
    println!("== starting the catalog server");
    let server = RunningServer::start(&config).await?;
    let client = &server.client;

    println!("== installing starter sources");
    let manager = SourceManager::new(client);
    let mut state = SourceState::default();
    manager.ensure_default_store(&mut state).await?;
    for id in STARTERS {
        match manager.answer(&mut state, id, Choice::Install).await {
            Ok(()) => println!("   installed {id}"),
            Err(e) => println!("   could not install {id}: {e}"),
        }
    }
    let installed = manager.installed("en").await?;
    let sources: Vec<_> = installed.into_iter().map(|i| i.source).collect();
    println!("   {} sources ready", sources.len());

    println!("== searching for '{query}' on all of them at once");
    let started = Instant::now();
    let mut search = SearchState::new(query.clone(), sources.len());
    let mut health = HealthDb::default();
    let mut stream = Box::pin(search_stream(
        client,
        sources,
        query.clone(),
        Duration::from_secs(25),
        8,
    ));
    while let Some(ev) = stream.next().await {
        match &ev {
            search::SearchEvent::Done { source, mangas, ms } => println!(
                "   {:<14} {:>3} results in {ms} ms",
                source.name,
                mangas.len()
            ),
            search::SearchEvent::Failed {
                source, message, ..
            } => println!("   {:<14} failed: {message}", source.name),
        }
        search::fanout::record_health(&mut health, &ev);
        search.apply(&ev);
    }
    drop(stream);
    println!("   all answered in {:.1}s", started.elapsed().as_secs_f32());

    let clusters = search.clusters();
    println!(
        "== {} results became {} distinct series",
        search.candidates().len(),
        clusters.len()
    );
    let mut enrichment = HashMap::new();
    let top: Vec<_> = clusters
        .iter()
        .take(5)
        .flat_map(|c| c.members.clone())
        .collect();
    enrichment.extend(enrich(client, &top, Duration::from_secs(40), 4).await);
    let ctx = RankContext {
        health: &health,
        language: "en",
        preferred_sources: &[],
        now_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as i64,
        enrichment: &enrichment,
    };
    let ranked = rank_series(clusters, &query, &ctx);
    for (i, s) in ranked.iter().take(5).enumerate() {
        let best = s.best();
        println!(
            "   #{} {:<40} relevance {:.2}, {} source(s), best: {} (score {:.2}, {} ch.)",
            i + 1,
            s.title,
            s.relevance,
            s.members.len(),
            best.candidate.source_name,
            best.score,
            best.enrichment
                .as_ref()
                .and_then(|e| e.chapter_count)
                .unwrap_or(0),
        );
    }
    let series = ranked.first().ok_or("no results")?;

    println!(
        "== merging chapter lists of '{}' across {} source(s)",
        series.title,
        series.members.len().min(4)
    );
    let members: Vec<_> = series.members.iter().take(4).collect();
    let mut lists = Vec::new();
    for m in &members {
        lists.push(
            client
                .fetch_chapters(m.candidate.manga_id)
                .await
                .unwrap_or_default(),
        );
    }
    let merged = merge_chapters(&lists);
    let missing = missing_numbers(&merged);
    println!(
        "   {} chapters in total, {} whole-number gap(s)",
        merged.len(),
        missing.len()
    );
    let multi = merged.iter().filter(|m| m.options.len() > 1).count();
    println!("   {multi} chapters are available from more than one source (automatic fallback)");

    // First chapters in reading order.
    let pick: Vec<usize> = (0..merged.len())
        .filter(|&i| merged[i].number.is_some())
        .take(want_chapters)
        .collect();
    let member_sources: Vec<String> = members
        .iter()
        .map(|m| m.candidate.source_id.clone())
        .collect();
    let chosen: Vec<_> = pick.iter().map(|&i| merged[i].clone()).collect();
    let requests = requests_from_merged(&chosen, &member_sources);

    let out_dir = work.join("out");
    let spec = JobSpec::new(
        series.title.clone(),
        requests,
        Settings::default(),
        ExportTarget::Folder {
            path: out_dir.clone(),
        },
    );
    println!("== exporting {} chapter(s) as XTCH", spec.chapters.len());
    let cache = PageCache::new(work.join("cache"));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let policy = RetryPolicy::default();
    let cancel = CancelFlag::new();
    let t = Instant::now();
    let run = run_job(client, &spec, &cache, &policy, Some(tx), &cancel);
    let watch = async {
        let mut last = 0;
        while let Some(ev) = rx.recv().await {
            match ev {
                JobEvent::ChapterStarted { index, total, name } => {
                    println!("   chapter {}/{}: {name}", index + 1, total)
                }
                JobEvent::PageDone { done, total, .. } if done == total || done >= last + 10 => {
                    last = done;
                    println!("      {done}/{total} pages");
                }
                JobEvent::UsedFallback { from, to, .. } => {
                    println!("      fell back from {from} to {to}")
                }
                JobEvent::ChapterSkipped { name, reason, .. } => {
                    println!("      skipped {name}: {reason}")
                }
                JobEvent::Writing { file } => println!("   writing {file}"),
                _ => {}
            }
        }
    };
    let (outcome, ()) = futures_util::future::join(run, watch).await;
    let outcome = outcome?;
    println!(
        "   done in {:.1}s: {} pages, {} chapter(s)",
        t.elapsed().as_secs_f32(),
        outcome.pages,
        outcome.chapters_done
    );

    println!("== checking the file the way the reader would");
    for f in &outcome.files {
        let bytes = std::fs::read(&f.location)?;
        let file = XtcFile::parse(&bytes)
            .map_err(|e| format!("the reader would reject this file: {e}"))?;
        for i in 0..file.page_count() {
            let page = file.decode_page(i)?;
            assert_eq!((page.image.width, page.image.height), (480, 800));
        }
        println!(
            "   {}\n   {:?}, {} pages all 480x800, title {:?}, direction {:?}, {:.1} MB",
            f.location,
            file.format,
            file.page_count(),
            file.title,
            file.read_direction(),
            bytes.len() as f64 / 1e6
        );
        for c in &file.chapters {
            println!(
                "      chapter menu: {:?} pages {}..={}",
                c.name,
                c.start_page + 1,
                c.end_page + 1
            );
        }
    }
    server.stop().await?;
    println!("\nLIVE EXPORT OK");
    Ok(())
}
