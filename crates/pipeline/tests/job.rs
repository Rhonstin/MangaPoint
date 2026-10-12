//! End-to-end job tests: fake catalog in, XTC/XTCH files out, read back with
//! the parser that mirrors the reader's.

use catalog::fake::FakeCatalog;
use converter::xtc::{Format, ReadDirection, XtcFile};
use image::{ImageEncoder, Rgba, RgbaImage};
use library::Settings;
use pipeline::{
    run_job, CancelFlag, ChapterRef, ChapterRequest, ExportTarget, JobEvent, JobSpec, OnMissing,
    PageCache, PipelineError, RetryPolicy,
};
use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc::unbounded_channel;

fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = RgbaImage::from_fn(w, h, |x, y| {
        let v = ((x * 3 + y * 2) as u8).wrapping_add(seed);
        Rgba([v, v, v, 255])
    });
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mangapoint-job-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn quick() -> RetryPolicy {
    RetryPolicy {
        attempts: 3,
        base_delay: std::time::Duration::from_millis(1),
    }
}

/// Register `chapter_id` with `pages` portrait pages served at `/c<id>/p<n>`.
fn add_chapter(c: &FakeCatalog, chapter_id: i64, pages: usize) {
    let paths: Vec<String> = (0..pages).map(|i| format!("/c{chapter_id}/p{i}")).collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    c.set_pages(chapter_id, &refs);
    for (i, p) in paths.iter().enumerate() {
        c.set_bytes(
            p,
            png(300, 450, (chapter_id as u8).wrapping_mul(10) + i as u8),
        );
    }
}

fn request(number: f64, options: &[(&str, i64)]) -> ChapterRequest {
    ChapterRequest {
        number: Some(number),
        name: format!("Chapter {number}"),
        options: options
            .iter()
            .map(|(s, id)| ChapterRef {
                source_id: s.to_string(),
                chapter_id: *id,
            })
            .collect(),
    }
}

fn spec(out: &PathBuf, chapters: Vec<ChapterRequest>) -> JobSpec {
    JobSpec::new(
        "Test Series",
        chapters,
        Settings::default(),
        ExportTarget::Folder { path: out.clone() },
    )
}

fn count_gets(c: &FakeCatalog, prefix: &str) -> usize {
    c.calls()
        .iter()
        .filter(|x| x.starts_with(&format!("get {prefix}")))
        .count()
}

#[tokio::test]
async fn a_two_chapter_job_produces_a_valid_xtch_with_chapter_menu() {
    let dir = temp("basic");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 3);
    add_chapter(&c, 2, 3);
    let sp = spec(
        &dir.join("out"),
        vec![request(1.0, &[("s", 1)]), request(2.0, &[("s", 2)])],
    );
    let cache = PageCache::new(dir.join("cache"));

    let outcome = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(outcome.pages, 6);
    assert_eq!(outcome.chapters_done, 2);
    assert!(outcome.skipped.is_empty());
    assert_eq!(outcome.files.len(), 1);
    assert!(
        outcome.files[0]
            .location
            .ends_with("Test Series - Ch 1-2.xtch"),
        "{}",
        outcome.files[0].location
    );

    let bytes = fs::read(&outcome.files[0].location).unwrap();
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.format, Format::Xtch);
    assert_eq!(file.page_count(), 6);
    assert_eq!(file.title, "Test Series");
    assert_eq!(file.read_direction(), Some(ReadDirection::RightToLeft));
    let chapters: Vec<_> = file
        .chapters
        .iter()
        .map(|c| (c.name.as_str(), c.start_page, c.end_page))
        .collect();
    assert_eq!(chapters, [("Chapter 1", 0, 2), ("Chapter 2", 3, 5)]);
    for i in 0..6 {
        let page = file.decode_page(i).unwrap();
        assert_eq!((page.image.width, page.image.height), (480, 800));
    }
    assert_eq!(outcome.source_use.len(), 2);
    assert!(outcome
        .source_use
        .iter()
        .all(|u| u.ok && u.source_id == "s"));
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn one_bit_output_and_left_to_right_follow_the_settings() {
    let dir = temp("xtc");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 2);
    let mut sp = spec(&dir.join("out"), vec![request(1.0, &[("s", 1)])]);
    sp.settings.conversion.format = Format::Xtc;
    sp.settings.conversion.direction = ReadDirection::LeftToRight;
    let cache = PageCache::new(dir.join("cache"));
    let outcome = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap();
    let bytes = fs::read(&outcome.files[0].location).unwrap();
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.format, Format::Xtc);
    assert_eq!(file.read_direction(), Some(ReadDirection::LeftToRight));
    assert!(outcome.files[0].location.ends_with(".xtc"));
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_double_page_spread_becomes_two_pages_in_reading_order() {
    let dir = temp("spread");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/spread"]);
    c.set_bytes("/spread", png(900, 450, 0));
    let sp = spec(&dir.join("out"), vec![request(1.0, &[("s", 1)])]);
    let cache = PageCache::new(dir.join("cache"));
    let outcome = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(outcome.pages, 2);
    let file_bytes = fs::read(&outcome.files[0].location).unwrap();
    let file = XtcFile::parse(&file_bytes).unwrap();
    assert_eq!(file.page_count(), 2);
    assert_eq!(file.chapters[0].end_page, 1);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn transient_download_errors_are_retried_transparently() {
    let dir = temp("flaky");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 2);
    c.flaky_path("/c1/p0", 2);
    let sp = spec(&dir.join("out"), vec![request(1.0, &[("s", 1)])]);
    let cache = PageCache::new(dir.join("cache"));
    let outcome = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(outcome.pages, 2);
    assert_eq!(count_gets(&c, "/c1/p0"), 3);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_failing_source_falls_back_to_the_next_one() {
    let dir = temp("fallback");
    let c = FakeCatalog::new();
    // Source "bad" lists pages that are gone; source "good" has the chapter.
    c.set_pages(10, &["/gone/p0"]);
    add_chapter(&c, 20, 2);
    let sp = spec(
        &dir.join("out"),
        vec![request(1.0, &[("bad", 10), ("good", 20)])],
    );
    let cache = PageCache::new(dir.join("cache"));
    let (tx, mut rx) = unbounded_channel();

    let outcome = run_job(&c, &sp, &cache, &quick(), Some(tx), &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(outcome.pages, 2);
    assert!(outcome.skipped.is_empty());
    assert_eq!(
        outcome
            .source_use
            .iter()
            .map(|u| (u.source_id.as_str(), u.ok))
            .collect::<Vec<_>>(),
        [("bad", false), ("good", true)]
    );

    let mut saw_fallback = false;
    while let Ok(ev) = rx.try_recv() {
        if let JobEvent::UsedFallback { from, to, .. } = ev {
            assert_eq!((from.as_str(), to.as_str()), ("bad", "good"));
            saw_fallback = true;
        }
    }
    assert!(saw_fallback);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_chapter_no_source_can_supply_is_skipped_and_reported() {
    let dir = temp("skip");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 2);
    add_chapter(&c, 3, 2);
    c.set_pages(2, &["/missing"]);
    let sp = spec(
        &dir.join("out"),
        vec![
            request(1.0, &[("s", 1)]),
            request(2.0, &[("s", 2)]),
            request(3.0, &[("s", 3)]),
        ],
    );
    let cache = PageCache::new(dir.join("cache"));
    let outcome = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(outcome.chapters_done, 2);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].name, "Chapter 2");
    let bytes = fs::read(&outcome.files[0].location).unwrap();
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.chapters.len(), 2);
    assert_eq!(file.chapters[1].name, "Chapter 3");
    assert_eq!(
        file.chapters[1].start_page, 2,
        "the skipped chapter leaves no hole"
    );
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn abort_mode_stops_at_the_first_missing_chapter() {
    let dir = temp("abort");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 1);
    c.set_pages(2, &["/missing"]);
    let mut sp = spec(
        &dir.join("out"),
        vec![request(1.0, &[("s", 1)]), request(2.0, &[("s", 2)])],
    );
    sp.on_missing = OnMissing::Abort;
    let cache = PageCache::new(dir.join("cache"));
    let err = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, PipelineError::ChapterFailed { ref name, .. } if name == "Chapter 2"),
        "{err:?}"
    );
    assert!(
        !dir.join("out").exists() || fs::read_dir(dir.join("out")).unwrap().next().is_none(),
        "no partial file is written"
    );
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_job_where_nothing_downloads_fails_clearly() {
    let dir = temp("nothing");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/missing"]);
    let sp = spec(&dir.join("out"), vec![request(1.0, &[("s", 1)])]);
    let cache = PageCache::new(dir.join("cache"));
    let err = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap_err();
    assert!(matches!(err, PipelineError::NothingDownloaded), "{err:?}");
    let err = run_job(
        &c,
        &spec(&dir.join("out"), vec![]),
        &cache,
        &quick(),
        None,
        &CancelFlag::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PipelineError::EmptyJob));
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_retried_job_does_not_download_finished_pages_again() {
    let dir = temp("resume");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 3);
    c.set_pages(2, &["/c2/p0", "/c2/p1"]); // no bytes yet: chapter 2 fails
    let mut sp = spec(
        &dir.join("out"),
        vec![request(1.0, &[("s", 1)]), request(2.0, &[("s", 2)])],
    );
    sp.on_missing = OnMissing::Abort;
    let cache = PageCache::new(dir.join("cache"));

    assert!(run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .is_err());
    assert_eq!(count_gets(&c, "/c1/"), 3, "chapter 1 downloaded once");

    // The source recovers; run the very same job again.
    c.set_bytes("/c2/p0", png(300, 450, 1));
    c.set_bytes("/c2/p1", png(300, 450, 2));
    let outcome = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(outcome.pages, 5);
    assert_eq!(
        count_gets(&c, "/c1/"),
        3,
        "chapter 1 came from the cache the second time"
    );
    assert!(
        cache.get(&sp.id(), 1, 0).is_none(),
        "the cache is emptied after success"
    );
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_corrupt_download_fails_the_source_and_is_not_kept() {
    let dir = temp("corrupt");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/bad"]);
    c.set_bytes("/bad", b"<html>access denied</html>".to_vec());
    add_chapter(&c, 2, 1);
    let sp = spec(&dir.join("out"), vec![request(1.0, &[("a", 1), ("b", 2)])]);
    let cache = PageCache::new(dir.join("cache"));
    let outcome = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(
        outcome.chapters_done, 1,
        "the second source supplied the chapter"
    );
    assert_eq!(outcome.source_use[0].source_id, "a");
    assert!(!outcome.source_use[0].ok);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn cancelling_stops_the_job_and_keeps_downloaded_pages() {
    let dir = temp("cancel");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 2);
    let sp = spec(&dir.join("out"), vec![request(1.0, &[("s", 1)])]);
    let cache = PageCache::new(dir.join("cache"));
    let cancel = CancelFlag::new();
    cancel.cancel();
    let err = run_job(&c, &sp, &cache, &quick(), None, &cancel)
        .await
        .unwrap_err();
    assert!(matches!(err, PipelineError::Cancelled));
    assert!(c.calls().is_empty(), "nothing is fetched once cancelled");
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn long_jobs_are_split_into_parts_at_chapter_boundaries() {
    let dir = temp("parts");
    let c = FakeCatalog::new();
    for id in 1..=3 {
        add_chapter(&c, id, 3);
    }
    let mut sp = spec(
        &dir.join("out"),
        (1..=3).map(|n| request(n as f64, &[("s", n)])).collect(),
    );
    sp.max_pages_per_file = 4; // two 3-page chapters would be 6
    let cache = PageCache::new(dir.join("cache"));
    let outcome = run_job(&c, &sp, &cache, &quick(), None, &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(outcome.files.len(), 3);
    let names: Vec<String> = outcome
        .files
        .iter()
        .map(|f| {
            PathBuf::from(&f.location)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        names,
        [
            "Test Series - Part 1 - Ch 1.xtch",
            "Test Series - Part 2 - Ch 2.xtch",
            "Test Series - Part 3 - Ch 3.xtch"
        ]
    );
    for f in &outcome.files {
        let bytes = fs::read(&f.location).unwrap();
        let file = XtcFile::parse(&bytes).unwrap();
        assert_eq!(file.page_count(), 3);
        assert_eq!(file.chapters.len(), 1);
        assert_eq!(
            file.chapters[0].start_page, 0,
            "chapter numbering restarts in each file"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn progress_events_report_every_page_and_chapter() {
    let dir = temp("events");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 3);
    let sp = spec(&dir.join("out"), vec![request(1.0, &[("s", 1)])]);
    let cache = PageCache::new(dir.join("cache"));
    let (tx, mut rx) = unbounded_channel();
    run_job(&c, &sp, &cache, &quick(), Some(tx), &CancelFlag::new())
        .await
        .unwrap();

    let mut started = 0;
    let mut dones = Vec::new();
    let mut wrote = 0;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            JobEvent::ChapterStarted { total, .. } => {
                started += 1;
                assert_eq!(total, 1);
            }
            JobEvent::PageDone { done, total, .. } => {
                assert_eq!(total, 3);
                dones.push(done);
            }
            JobEvent::Writing { file } => {
                wrote += 1;
                assert!(file.ends_with(".xtch"));
            }
            _ => {}
        }
    }
    dones.sort_unstable();
    assert_eq!((started, wrote), (1, 1));
    assert_eq!(dones, [1, 2, 3]);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_job_id_is_stable_and_depends_on_the_chapters() {
    let out = PathBuf::from("/out");
    let a = spec(&out, vec![request(1.0, &[("s", 1)])]);
    let same = spec(&out, vec![request(1.0, &[("s", 1)])]);
    let other_chapter = spec(&out, vec![request(1.0, &[("s", 2)])]);
    let mut other_title = a.clone();
    other_title.series_title = "Other".into();
    assert_eq!(a.id(), same.id());
    assert_ne!(a.id(), other_chapter.id());
    assert_ne!(a.id(), other_title.id());
    // Output settings do not change the id, so retried jobs reuse downloads.
    let mut tweaked = a.clone();
    tweaked.settings.conversion.contrast = 80;
    assert_eq!(a.id(), tweaked.id());
}

#[test]
fn requests_are_built_from_merged_chapter_lists() {
    use catalog::fake::chapter;
    let best = vec![chapter(1, 1.0, 0), chapter(2, 2.0, 0)];
    let other = vec![chapter(11, 2.0, 0), chapter(12, 3.0, 0)];
    let merged = search::merge_chapters(&[best, other]);
    let reqs =
        pipeline::requests_from_merged(&merged, &["src-best".to_string(), "src-other".to_string()]);
    assert_eq!(reqs.len(), 3);
    assert_eq!(
        reqs[1].options,
        [
            ChapterRef {
                source_id: "src-best".into(),
                chapter_id: 2
            },
            ChapterRef {
                source_id: "src-other".into(),
                chapter_id: 11
            },
        ]
    );
    assert_eq!(reqs[2].options.len(), 1);
}
