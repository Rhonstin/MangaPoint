//! A short speed test of a source: fetch a handful of pages at once and see
//! how long it takes. Cheap enough to run before a download, and it tells
//! sources apart far better than guessing.

use crate::fetch::{fetch_page, with_retry, RetryPolicy};
use crate::types::ChapterRef;
use catalog::Catalog;
use converter::decode::image_dimensions;
use futures_util::future::join_all;
use std::time::Instant;

/// Pages fetched at once in a probe.
pub const PROBE_PAGES: usize = 6;

/// Pages per second the source gave for the first few pages of this chapter,
/// including the time to list them. `None` if it gave no usable page.
pub async fn probe_source<C: Catalog>(
    catalog: &C,
    option: &ChapterRef,
    policy: &RetryPolicy,
) -> Option<f32> {
    let started = Instant::now();
    let paths = with_retry(policy, || catalog.chapter_pages(option.chapter_id))
        .await
        .ok()?;
    let wanted: Vec<&String> = paths.iter().take(PROBE_PAGES).collect();
    if wanted.is_empty() {
        return None;
    }
    let pages = join_all(wanted.iter().map(|path| fetch_page(catalog, path, policy))).await;
    let usable = pages
        .iter()
        .filter(|r| {
            r.as_ref()
                .is_ok_and(|bytes| image_dimensions(bytes).is_ok())
        })
        .count();
    if usable == 0 {
        return None;
    }
    Some(usable as f32 / started.elapsed().as_secs_f32().max(0.001))
}

/// Probe several sources at the same time. Each entry is a source id and a
/// chapter it can supply.
pub async fn probe_sources<C: Catalog>(
    catalog: &C,
    probes: &[(String, ChapterRef)],
    policy: &RetryPolicy,
) -> Vec<(String, Option<f32>)> {
    join_all(probes.iter().map(|(source, option)| async move {
        (source.clone(), probe_source(catalog, option, policy).await)
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::FakeCatalog;
    use image::{ImageEncoder, Rgba, RgbaImage};
    use std::time::Duration;

    fn png() -> Vec<u8> {
        let img = RgbaImage::from_fn(30, 40, |x, y| Rgba([(x + y) as u8, 0, 0, 255]));
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(img.as_raw(), 30, 40, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn quick() -> RetryPolicy {
        RetryPolicy {
            attempts: 1,
            base_delay: Duration::from_millis(1),
        }
    }

    fn add(c: &FakeCatalog, id: i64, pages: usize, latency: u64) {
        let paths: Vec<String> = (0..pages).map(|i| format!("/c{id}/p{i}")).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        c.set_pages(id, &refs);
        for p in &paths {
            c.set_bytes(p, png());
        }
        c.set_path_latency_ms(&format!("/c{id}/"), latency);
    }

    fn option(id: i64) -> ChapterRef {
        ChapterRef {
            source_id: format!("s{id}"),
            chapter_id: id,
        }
    }

    #[tokio::test]
    async fn a_quicker_source_scores_higher() {
        let c = FakeCatalog::new();
        add(&c, 1, 10, 150);
        add(&c, 2, 10, 15);
        let slow = probe_source(&c, &option(1), &quick()).await.unwrap();
        let fast = probe_source(&c, &option(2), &quick()).await.unwrap();
        assert!(fast > slow * 3.0, "fast {fast}, slow {slow}");
    }

    #[tokio::test]
    async fn only_the_first_few_pages_are_fetched() {
        let c = FakeCatalog::new();
        add(&c, 1, 40, 1);
        probe_source(&c, &option(1), &quick()).await.unwrap();
        let fetched = c.calls().iter().filter(|x| x.starts_with("get ")).count();
        assert_eq!(fetched, PROBE_PAGES);
    }

    #[tokio::test]
    async fn a_source_that_gives_nothing_usable_scores_nothing() {
        let c = FakeCatalog::new();
        c.set_pages(1, &["/missing/p0", "/missing/p1"]);
        assert_eq!(probe_source(&c, &option(1), &quick()).await, None);
        // A page that is not an image does not count either.
        c.set_pages(2, &["/html/p0"]);
        c.set_bytes("/html/p0", b"<html>blocked</html>".to_vec());
        assert_eq!(probe_source(&c, &option(2), &quick()).await, None);
        // No pages at all.
        c.set_pages(3, &[]);
        assert_eq!(probe_source(&c, &option(3), &quick()).await, None);
    }

    #[tokio::test]
    async fn several_sources_are_probed_at_once_and_reported_by_name() {
        let c = FakeCatalog::new();
        add(&c, 1, 6, 80);
        add(&c, 2, 6, 80);
        let started = Instant::now();
        let out = probe_sources(
            &c,
            &[("a".into(), option(1)), ("b".into(), option(2))],
            &quick(),
        )
        .await;
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "probed one after the other"
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "a");
        assert!(out.iter().all(|(_, s)| s.is_some()));
    }
}
