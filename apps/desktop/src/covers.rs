//! Cover pictures, handed to the web view on demand.
//!
//! A cover is not read into the app and kept as text in memory. The page asks
//! for the picture by address, only for the covers in view (the images are
//! lazy), and the app answers from a small cache on disk, fetching it from the
//! sources the first time. When the same series is on several sources, they
//! are all asked at once and the first picture to arrive is the one used.

use catalog::SuwayomiClient;
use dioxus::desktop::wry::http::{Request, Response};
use dioxus::desktop::RequestAsyncResponder;
use futures_util::stream::{FuturesUnordered, StreamExt};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// The name the web view knows this by.
pub const SCHEME: &str = "cover";

/// The catalog server's client, for fetching covers. Kept here because the
/// answers are given from outside the page's own code.
static CLIENT: RwLock<Option<Arc<SuwayomiClient>>> = RwLock::new(None);

/// How many covers are fetched from the sources at once.
static FETCHING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(6);

/// How big the cache of fetched covers may get, and how long a cover stays in it.
const CACHE_LIMIT: u64 = 300 * 1024 * 1024;
const CACHE_KEEP: Duration = Duration::from_secs(30 * 24 * 3600);

/// Use this client (or none, when the server stops) for fetching covers.
pub fn set_client(client: Option<Arc<SuwayomiClient>>) {
    if let Ok(mut slot) = CLIENT.write() {
        *slot = client;
    }
}

fn cache_dir() -> PathBuf {
    crate::state::paths().root().join("cache").join("covers")
}

/// Where the web view finds covers.
fn base() -> &'static str {
    if cfg!(target_os = "windows") {
        "http://cover.localhost"
    } else {
        "cover://localhost"
    }
}

/// Percent-encode text for a query value.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The text a percent-encoded query value stands for.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The address of a cover that the sources hold, under these paths (the
/// catalog's own paths, best first). With several, the first to answer wins.
pub fn source_url(paths: &[String]) -> String {
    let list: Vec<String> = paths.iter().map(|p| format!("p={}", encode(p))).collect();
    format!("{}/s?{}", base(), list.join("&"))
}

/// The address of the cover saved with a series in the library. `version`
/// changes when the saved picture does, so the page asks again.
pub fn saved_url(series_id: &str, version: u64) -> String {
    format!("{}/v?id={}&t={version}", base(), encode(series_id))
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn mime_of(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', ..] => "image/png",
        [b'R', b'I', ..] => "image/webp",
        [b'G', b'I', ..] => "image/gif",
        _ => "image/jpeg",
    }
}

fn reply(status: u16, mime: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Cache-Control", "private, max-age=86400")
        .header("Access-Control-Allow-Origin", "*")
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

fn missing() -> Response<Cow<'static, [u8]>> {
    reply(404, "text/plain", Vec::new())
}

/// Answer the web view's request for a cover.
pub fn serve(request: Request<Vec<u8>>, responder: RequestAsyncResponder) {
    let path = request.uri().path().to_string();
    let query = parse_query(request.uri().query().unwrap_or(""));
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        responder.respond(reply(503, "text/plain", Vec::new()));
        return;
    };
    runtime.spawn(async move {
        let response = match path.as_str() {
            "/v" => saved(&query).await,
            "/s" => from_sources(&query).await,
            _ => None,
        };
        responder.respond(response.unwrap_or_else(missing));
    });
}

/// The cover saved with a series.
async fn saved(query: &[(String, String)]) -> Option<Response<Cow<'static, [u8]>>> {
    let id = &query.iter().find(|(k, _)| k == "id")?.1;
    let path = crate::state::library_files().cover_path(id);
    let mut bytes = tokio::fs::read(&path).await.ok()?;
    // A cover saved before they were kept small is shrunk once, on disk.
    if bytes.len() > crate::logic::COVER_KEEP_BYTES {
        let big = bytes.clone();
        let small = tokio::task::spawn_blocking(move || crate::logic::small_cover(&big))
            .await
            .ok()
            .flatten();
        if let Some(small) = small.filter(|s| s.len() < bytes.len()) {
            let _ = tokio::fs::write(&path, &small).await;
            bytes = small;
        }
    }
    let mime = mime_of(&bytes);
    Some(reply(200, mime, bytes))
}

/// A cover from the sources: from the cache if it was fetched before, else
/// from whichever source answers first.
async fn from_sources(query: &[(String, String)]) -> Option<Response<Cow<'static, [u8]>>> {
    let paths: Vec<String> = query
        .iter()
        .filter(|(k, _)| k == "p")
        .map(|(_, v)| v.clone())
        .collect();
    if paths.is_empty() {
        return None;
    }
    let mut hash = Sha256::new();
    for p in &paths {
        hash.update(p.as_bytes());
        hash.update([0]);
    }
    let name: String = hash.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect();
    let file = cache_dir().join(format!("{name}.img"));
    if let Ok(bytes) = tokio::fs::read(&file).await {
        let mime = mime_of(&bytes);
        return Some(reply(200, mime, bytes));
    }
    let _turn = FETCHING.acquire().await.ok()?;
    // Another request for the same cover may have finished while this waited.
    if let Ok(bytes) = tokio::fs::read(&file).await {
        let mime = mime_of(&bytes);
        return Some(reply(200, mime, bytes));
    }
    let client = CLIENT.read().ok()?.clone()?;
    let winner = first_picture(&client, paths).await?;
    let original = winner.clone();
    // Shown at poster size, so what the source sent is shrunk as long as that
    // makes it smaller.
    let small = tokio::task::spawn_blocking(move || crate::logic::small_cover(&original))
        .await
        .ok()
        .flatten()
        .filter(|s| s.len() < winner.len());
    let bytes = small.unwrap_or(winner);
    if let Some(dir) = file.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    let _ = tokio::fs::write(&file, &bytes).await;
    let mime = mime_of(&bytes);
    Some(reply(200, mime, bytes))
}

/// Ask for all these pictures at once and take the first that is one.
pub async fn first_picture(client: &Arc<SuwayomiClient>, paths: Vec<String>) -> Option<Vec<u8>> {
    let mut asking = FuturesUnordered::new();
    for path in paths {
        let client = client.clone();
        asking.push(async move {
            client
                .get_bytes(&path)
                .await
                .ok()
                .filter(|b| sources::check::image_kind(b).is_some())
        });
    }
    let wait = async {
        while let Some(found) = asking.next().await {
            if found.is_some() {
                return found;
            }
        }
        None
    };
    tokio::time::timeout(Duration::from_secs(25), wait).await.ok().flatten()
}

/// Tidy the cache of fetched covers: old ones go, and the oldest of the rest
/// until it is under its limit. Run once at start, off the main thread.
pub fn tidy_cache() {
    std::thread::spawn(|| {
        let Ok(entries) = std::fs::read_dir(cache_dir()) else { return };
        let now = SystemTime::now();
        let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let at = meta.modified().unwrap_or(now);
            if now.duration_since(at).is_ok_and(|age| age > CACHE_KEEP) {
                let _ = std::fs::remove_file(entry.path());
                continue;
            }
            files.push((at, meta.len(), entry.path()));
        }
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort_by_key(|f| f.0);
        for (_, size, path) in files {
            if total <= CACHE_LIMIT {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_survive_the_address() {
        let paths = vec!["/api/v1/manga/12/thumbnail?a=b&c=d e".to_string(), "/x/ü".to_string()];
        let url = source_url(&paths);
        let query = url.split_once('?').unwrap().1;
        let back: Vec<String> = parse_query(query).into_iter().map(|(_, v)| v).collect();
        assert_eq!(back, paths);
    }

    #[test]
    fn a_short_or_broken_escape_is_left_alone() {
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("a%zzb"), "a%zzb");
        assert_eq!(decode("a%41"), "aA");
    }
}
