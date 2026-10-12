# MangaPoint — Infrastructure Plan (draft v2.0)

Native desktop (Rust) + Next.js website · Xteink X4 Pro · Fully open source
Status: **draft for editing**. Items marked `[VERIFY]` are unconfirmed assumptions; `[DECISION]` needs your call.

## 1. Goal and UX principles

Search a huge catalog easily, pick chapters, convert to the e-reader format, export. Nothing else for the user to do.

- Three steps: **Search -> Pick chapters -> Export**.
- No setup: user never installs Java, Suwayomi, extensions, or touches ports.
- Plain-language errors; advanced settings hidden behind "Customize".

## 2. Architecture

```
                 crates/converter  (pure Rust, no I/O, no UI)
                 crop · grayscale · dither · XTC/XTCH encode
                 /                                  \
   native build                                      wasm32 build (wasm-bindgen)
        |                                                   |
MangaPoint Desktop (Rust, Win/Linux/macOS)        MangaPoint Web (Next.js, static)
  UI · library · search/ranking                     Import CBZ/PDF -> Preview -> Export
  crates/catalog (Suwayomi adapter)                 Everything runs in the browser
        |
Suwayomi sidecar (bundled JRE, 127.0.0.1)
        |
Mihon-compatible extensions -> manga sources
```

- The website and desktop app share the **converter crate**: native for desktop, compiled to WASM for the website. One implementation, identical output.
- The website has no catalog or download features (no Suwayomi in a browser). Its logic is baked into the Next.js app (WASM + Web Worker), no backend.
- No servers operated by us. Everything runs on the user's device.

## 3. Technology choices

| Component | Technology | Notes |
|---|---|---|
| Workspace | Cargo workspace (+ pnpm for `apps/web`) | |
| Converter | Rust crate -> native + WASM | `image`, `imageproc`, `fast_image_resize`; `[VERIFY]` WASM perf |
| Desktop UI | **Dioxus desktop** (decided) | Rust UI, HTML/CSS-style design freedom, cover-grid friendly; modern, minimal look. Fallback if it disappoints: Slint |
| Async / HTTP | tokio + reqwest | Talk to Suwayomi |
| Suwayomi API | `graphql_client` (typed from pinned schema) | |
| Local DB | SQLite via `rusqlite`/`sqlx` | Library, jobs, source health, prefs |
| Archives | `zip` crate | CBZ in/out (native + WASM) |
| PDF | Desktop: `pdfium-render`; Web: pdf.js in JS feeding pixels to WASM `[VERIFY]` | |
| Fuzzy matching | `strsim`, `unicode-normalization`, `image_hasher` | Search dedup (section 7) |
| Web | Next.js (static export) + TypeScript + Tailwind | Hosted on GitHub Pages |
| Packaging | `cargo-packager` (msi/nsis, dmg, AppImage/deb) `[VERIFY]` | Bundles Suwayomi + JRE |
| CI/CD | GitHub Actions | Matrix build on 3 OSes |
| Releases | GitHub Releases | |

No Docker, cloud DB, object storage, or paid APIs.

## 4. Desktop runtime

Rust service manager (`crates/catalog` + desktop shell):
1. On launch, check for a running local Suwayomi; otherwise start the bundled one (pinned version + minimal `jlink` JRE).
2. Poll health check until ready; show a friendly splash meanwhile.
3. Connect via local GraphQL.
4. Search, download, then pass downloaded images to the converter.
5. On exit, shut Suwayomi down cleanly (also handle crashes/orphan processes).

Dev flag: connect to an existing Suwayomi instead of spawning one.

First run: one-click "Enable sources" screen that pre-configures an extension repository and a starter set of sources (decided: Keiyoushi repo, most popular large-catalog sources).

### Starter extension repo and sources
- **Repo:** Keiyoushi (`github.com/keiyoushi/extensions`), the largest Mihon-compatible repo.
- **Extension store setup (verified live on v2.4.2366; this supersedes the Suwayomi wiki's "repo URL" advice):** Suwayomi ships no default store, so MangaPoint adds one on first run with the GraphQL mutation `addExtensionStore(input:{indexUrl:"https://github.com/keiyoushi/extensions/raw/repo/index.pb"})`. The older `extensionRepos` setting is deprecated and must not be used: it rewrites GitHub URLs to the two-entry stub `.../repo/index.min.json`, which loads nothing useful. The new `extensionStores` list is not readable through GraphQL settings; check that a store loaded by counting `fetchExtensions` results (1,432 for Keiyoushi).
- **Tested 2026-10-11 against Suwayomi v2.4.2366 (Windows zip, bundled Zulu JRE 25, SHA-256 verified against GitHub's digest and the project's `Checksums.sha256`):**
  - **An earlier note here was wrong:** I first recorded `https://raw.githubusercontent.com/keiyoushi/extensions/repo` as working. It only appeared to work because the extension list was cached from a previous attempt. On a fresh data folder every `extensionRepos` URL returned 0 extensions. Lesson: always verify setup on a fresh data directory, and treat an empty extension list as an error (the smoke test asserts > 100).
  - Server flags (all verified): `-Dsuwayomi.tachidesk.config.server.{rootDir,ip=127.0.0.1,port,webUIEnabled=false,systemTrayEnabled=false,initialOpenInBrowserEnabled=false,kcefEnabled=<bool>,authMode=BASIC_AUTH,authUsername,authPassword}`. With `BASIC_AUTH` and a random per-launch password, a request without credentials gets HTTP 401. Introspection is blocked, so the typed client is written against names confirmed on a live server; the server's validation errors reveal missing field names (that is how `indexUrl` was found).
  - `kcefEnabled` defaults to **on** (except macOS, where Suwayomi does not support the embedded Chromium at all). MangaPoint sets it **off** by default; with it off, no Chromium is downloaded and the server starts in about 3 seconds.
  - Flow works end to end through GraphQL: `fetchExtensions` -> `updateExtension(patch:{install:true})` -> `fetchSourceManga(type:SEARCH)` -> `fetchChapters` -> `enqueueChapterDownload` + `startDownloader`.
  - Search "one piece" (all search calls took 0.1-1.1s):

    | Source | Result |
    |---|---|
    | MangaFire | works: 50 results, 2441 chapters, download OK |
    | MangaPill | works: 50 results, 1211 chapters, download OK (first chapter 404'd, stale link; latest chapter OK) |
    | MangaK (MangaBuddy ext.) | works: 24 results, 1310 chapters, download OK |
    | MangaKatana | works but slow: 5 results, 1201 chapters; a 13-page chapter took >60s, a 54-page chapter finished in ~2 min (retested) |
    | MangaDex | works: 20 results, but "One Piece" only has 1 chapter (official licensed chapters are not hosted) |
    | MangaPlus | works but only 1 result (official, latest chapters only) |
    | Webtoons | returned unrelated titles for this query (webcomics catalog) |
    | Asura Scans | works (retested with its own popular titles): 20 popular, search OK, 119-chapter series, 24-page chapter downloaded in ~25s |
    | Flame Comics | works (retested): 154 popular titles, search OK, 15-page chapter downloaded in ~15s |
    | Comikey | search + 376 chapters OK, but page fetch fails: "Open chapter in WebView, then try again (token not found)". Needs interactive WebView, so exclude from starter set |
    | Manganato, Mangakakalot | **blocked: Cloudflare bypass disabled**; needs FlareSolverr (external service) |

  - **Surprises to design for:** (1) Suwayomi downloads a ~268 MB Chromium (CEF) component on first start; ship it in the installer or show a progress screen. (2) Some sources need FlareSolverr for Cloudflare, so treat them as "unavailable" by default and hide them from starter sources. (3) Popular series are often missing or incomplete on official sources (MangaDex, MangaPlus), which makes the multi-source fallback and chapter-merge in section 7 essential. (4) Per-chapter links can be stale (404), so downloads need retry-with-fallback.
  - **Clean re-run with the Rust code (`cargo run -p catalog --example smoke`), fresh data folder, browser support OFF:** 9 of 12 sources pass the full chain (popular -> search -> chapters -> page list -> real page image fetched): MangaDex, MangaFire, MangaPlus, Webtoons, MangaPill, MangaBuddy, MangaKatana, Asura Scans, Flame Comics, each in 1.5-9 s. Comikey fails with "CEF is disabled" (needs browser support); Manganato and Mangakakalot fail with the Cloudflare/FlareSolverr message. So **only Comikey-type sources need the Chromium opt-in**; the nine above work without it. Page images can be fetched directly through the server, so conversion does not need a separate download step.
  - **Starter set based on these results (all verified through a full download):** MangaFire, MangaPill, MangaBuddy (MangaK), MangaKatana (slow, rank lower in best-source scoring), Asura Scans, Flame Comics, plus MangaDex and MangaPlus (official, partial catalogs). Excluded: Comikey (needs WebView), Manganato and Mangakakalot (need FlareSolverr). Webtoons: search works but untested for download.
  - **Lesson for search (section 7):** a "0 results" can just mean the title isn't in that catalog, so never score a source as unhealthy from an empty search alone; use error rate and download success. Source speed varies 10x (15s to 120s per chapter), so best-source scoring should include measured download time.
- **Starter sources.** Confirmed to exist as source directories in `keiyoushi/extensions-source` (existence only; working status is not yet tested):
  - Multi-language (`src/all`): **MangaDex**, **MangaPlus**, **Webtoons**, **Comikey**, **MangaFire**, **MangaToon**.
  - English (`src/en`) large catalogs: **Mangakakalot**, **Manganelo**, **MangaBuddy**, **Mangapill**, **MangaKatana**, **MangaFox**, **MangaHere**, **MangaFreak**, **Mangago**, **MangaDemon**, **Asura Scans**, **Flame Comics**, **Atsumaru**, **Comix**, plus official ones (**K Manga**, **Kodansha**).
  - Not located in the truncated listings: Comick, Bato.to, MangaPark (they may sit under another path or have a different name).
  - Initial enabled set = the ones that pass the CI health check (below), prioritizing the largest catalogs (MangaDex, MangaFire, Mangakakalot/Manganelo, MangaBuddy, Mangapill, Asura, Flame, Webtoons, MangaPlus).
- **Make it self-maintaining:** a scheduled CI job installs the candidate list into the pinned Suwayomi and runs a search + one-chapter download per source; the app ships a `starter-sources.toml` regenerated from passing sources, and the runtime health stats (section 7.4) demote sources that break.
- Language default `en`; user can add more languages in Settings.

Security: bind to `127.0.0.1` only (random port + auth if Suwayomi supports it `[VERIFY]`); validate imported archives and source URLs; extensions are third-party code, so keep sources managed through MangaPoint's own settings.

## 5. Repository layout

```
mangapoint/
├── Cargo.toml                 (workspace)
├── crates/
│   ├── converter/             pure conversion engine (native + wasm)
│   ├── device-profiles/       X4 Pro resolution, bit depth, format params
│   ├── catalog/               CatalogProvider trait + Suwayomi adapter
│   ├── search/                normalization, clustering, ranking (section 7)
│   ├── library/               SQLite models, conversion_jobs
│   └── wasm/                  wasm-bindgen bindings over converter
├── apps/
│   ├── desktop/               Dioxus app, Suwayomi process manager
│   └── web/                   Next.js app, imports crates/wasm build
├── tests/                     fixtures, golden images, integration
├── .github/workflows/         desktop.yml, web.yml, tests.yml
└── README.md
```

Rule: `converter` never depends on UI, network, or Suwayomi. Pixels in, encoded bytes out.

## 6. Converter API (Rust)

```rust
pub struct MangaProfile {
    pub device: Device,            // XteinkX4Pro
    pub layout: Layout,            // Manga | Webtoon | Auto
    pub crop: Crop,                // Smart | Overlapping | Full
    pub direction: Direction,      // Rtl | Ltr | Vertical
    pub contrast: u8,              // 0-100
    pub overlap_percent: u8,
    pub dithering: Dither,         // FloydSteinberg | Atkinson | Ordered
    pub format: Format,            // Xtc | Xtch
}

pub trait PageSource { fn next_page(&mut self) -> Option<RawPage>; }   // streaming, low memory

pub fn convert(
    pages: impl PageSource,
    profile: &MangaProfile,
    progress: impl FnMut(usize, Option<usize>),
) -> Result<ConversionResult, ConvertError>;
```

Default profile: X4 Pro, manga, smart crop, RTL, contrast 55, overlap 8%, Floyd-Steinberg, XTC.
Smart cropping: deterministic gutter/projection-profile detection first; optional ONNX detector (`ort`) later. No OpenCV dependency. `[VERIFY]` XTC/XTCH spec and X4 Pro screen specs before building the encoder.

## 6b. Target devices and file format (researched 2026-10-11)

**Devices** (all 480x800 portrait, so one page size for all):
- Xteink X4 (ESP32-C3), X3, and X4 Pro (4.3", 219 PPI, touchscreen, frontlight, 16 GB, launched July 2026; MCU reportedly ESP32-S3, unconfirmed by the vendor).
- **CrossPoint Reader** (open-source, MIT, `crosspoint-reader/crosspoint-reader`): first-priority target. Its README lists X3, X4, **X4 Pro**, X4 Classic, reTerminal Sticky, M5PaperMono. Reads `.epub`, `.xtc`/`.xtch`, `.txt`, `.bmp`; **no CBZ/PDF**, so conversion to XTC/XTCH is required. Files go on the SD card, or via Wi-Fi upload web UI, WebDAV, OPDS, or USB drive mode (X4 Pro). CNX Software (July 23) said X4 Pro support was "not yet" with Xteink an official partner; the repo now lists it. `[VERIFY on hardware]`.
- Stock firmware: XTC/XTCH is its native format (CrossPoint's parser is based on the community EPUB2XTC converter). Same files should work on stock and CrossPoint. `[VERIFY on hardware]`.

**Format (verified against CrossPoint's parser, `lib/Xtc/Xtc/XtcTypes.h`, not just the AI-generated community spec):** little-endian throughout.
- **XTC** = container with 1-bit pages (XTG); **XTCH** = same container, magic `XTCH`, 2-bit pages (XTH, 4 gray levels).
- File header, 56 bytes: magic u32 (`XTC\0`=0x00435458, `XTCH`=0x48435458), version major u8=1 / minor u8=0 (readers also accept swapped), pageCount u16, readDirection u8 (0=LTR, 1=RTL, 2=top-to-bottom), hasMetadata/hasThumbnails/hasChapters u8 each, currentPage u32 (1-based), metadataOffset u64, pageTableOffset u64, dataOffset u64, thumbOffset u64, chapterOffset u32, padding u32. Legacy files used a 48-byte header; always write 56.
- Page table: 16 bytes per page: dataOffset u64 (absolute), dataSize u32, width u16, height u16.
- Page header, 22 bytes (XTG and XTH): magic (`XTG\0`=0x00475458, `XTH\0`=0x00485458), width u16, height u16, colorMode u8=0, compression u8=0, dataSize u32, md5 u64 (first 8 bytes, optional), then pixel data.
- **XTG (1-bit):** row-major, 8 px/byte, MSB = leftmost, `dataSize = ((w+7)/8)*h`, 0=black, 1=white.
- **XTH (2-bit):** two bit planes (all Bit1, then all Bit2), column-major starting from the **rightmost** column, 8 vertical px/byte MSB=top, each column padded to a byte, `dataSize = w*((h+7)/8)*2`, `value=(bit1<<1)|bit2`. Gray mapping per community spec is non-linear (0=white, 1=dark gray, 2=light gray, 3=black). `[VERIFY on device]` with a test pattern.
- Optional metadata block (256 B: title 128, author 64, publisher 32, language 16, createTime, coverPage, chapterCount) and chapter entries (96 B: name 80, startPage u16, endPage u16, reserved).
- Page numbers: `currentPage` is 1-based; page index, chapter start/end, and cover are 0-based.

**Stock firmware on X4 Pro (checked 2026-10-11): unconfirmed for XTC.** Xteink's official X4 Pro FAQ/spec lists only EPUB and TXT (documents), JPG/BMP (images), BIN/XTF (fonts); XTC/CBZ are not listed. Community guides contradict each other (one says PDF/MOBI also work). XTC is widely described as the X4's native format and many CBZ->XTC tools target stock X4, but nobody documents it for the X4 Pro stock firmware. Same 480x800 panel as the X4.
- **Export targets:** (1) XTC/XTCH, confirmed for CrossPoint (lists X4 Pro); (2) **fallback "EPUB (image pages)"**, one full-page image per page, for stock firmware if XTC turns out unsupported. Build (1) first; add (2) after a hardware test, or sooner if a tester reports XTC failing on stock.

**Product implications**
- Output default: **XTCH (2-bit)** for better manga grayscale vs XTC (1-bit) for smaller/faster; offer both. `[VERIFY]` page-turn speed and file size on device.
- Target resolution is always 480x800; the converter is device-agnostic beyond that. Keep `device-profiles` as data (resolution, formats, direction), since X4 Pro may differ in future firmware.
- Add chapter entries from the Suwayomi chapter list so the reader's chapter menu works.
- Write the encoder against CrossPoint's parser as the reference and add golden-file tests; ideally also a reader in the test suite (a Rust XTC parser mirroring `XtcParser.cpp`).

## 7. Search, de-duplication and relevance ranking

Problem: the same series appears in many sources, with different titles, chapter counts, and quality. We show **one result per series**, with the best source preselected.

### 7.1 Fan-out
- Query all enabled sources in parallel via Suwayomi (`[VERIFY]` GraphQL search query shape).
- Per-source timeout (~4s), stream results to the UI as they arrive, drop failing sources silently.
- Cache results per (query, source) for a short TTL; cache covers on disk.

### 7.2 Normalize titles
Unicode NFKC, lowercase, strip punctuation and bracket tags (`[Official]`, `(Colored)`, language tags), collapse whitespace, drop leading articles, keep alt titles when sources provide them.

### 7.3 Cluster into series
Merge two results when any of:
1. Normalized titles equal.
2. Fuzzy title similarity above threshold (Jaro-Winkler / token-set ratio) **and** a corroborating signal: matching author, or cover perceptual-hash distance below threshold.
3. An alt title of one equals a title of the other.

Be conservative: a false merge (two different series) is worse than a missed merge, since a missed merge only shows a duplicate. Make thresholds configurable and cover them with a labelled test set of known title variants.

### 7.4 Pick the best source within a cluster
Weighted score (weights tunable):

| Signal | Why |
|---|---|
| Chapter count vs. cluster max | Completeness |
| Latest chapter freshness | Actively updated |
| Source health (rolling success rate, latency) | Reliable downloads |
| Metadata completeness (cover, author, description) | Better UI |
| Language match with user setting | Avoid wrong language |
| User preference / pinned source | Respect choices |
| Past failed downloads from that source | Learn from failures |

Best source is the default; "Other sources (N)" is an expandable list on the series page.

### 7.5 Rank clusters against the query
Exact normalized match > prefix match > all tokens match > fuzzy match; tie-break by number of sources carrying the series (popularity proxy) and best-source score.

### 7.6 Chapter-level merge and fallback
On the series page, union chapters across the cluster by chapter number. If a chapter is missing or fails on the best source, silently retry from the next-best source. Flag gaps clearly ("Ch. 57 unavailable").

### 7.7 Learning
Store source health, user overrides, and failures in SQLite (`library` crate) so ranking improves with use. No data leaves the device.

## 8. Catalog integration

```rust
#[async_trait]
pub trait CatalogProvider {
    async fn list_sources(&self) -> Result<Vec<Source>>;
    async fn search(&self, source: &SourceId, query: &str) -> Result<Vec<MangaSummary>>;
    async fn chapters(&self, manga: &MangaId) -> Result<Vec<Chapter>>;
    async fn download(&self, chapters: &[ChapterId]) -> Result<DownloadHandle>;
    async fn pages(&self, chapter: &ChapterId) -> Result<PageStream>;
}
```

Suwayomi is the only implementation initially; types generated from the pinned version's GraphQL schema. Multi-source fan-out and clustering live in `crates/search`, above this trait.

## 9. Storage

```
MangaPoint/
├── library.db            (series, jobs, source health, prefs)
├── config.toml
├── downloads/<manga-id>/<chapter-id>/
├── exports/<Title>_Vol1.xtc
├── cache/{covers,search}/
└── suwayomi/{data,extensions}/
```

`conversion_jobs` table lets an interrupted conversion resume without re-downloading. Browser build: presets in IndexedDB; stream large files instead of loading them whole.

## 10. Website (Next.js)

- Static export (`output: 'export'`) hosted on GitHub Pages; no server.
- Converter runs as WASM in a Web Worker; pages are decoded with the browser (`createImageBitmap`) and passed to WASM as raw pixels.
- Flow: drop CBZ/PDF -> preview -> choose preset -> export XTC/XTCH.
- Same default preset as desktop; shares `device-profiles`.
- Mobile/iOS Safari: stream pages, avoid holding the full archive in memory.

## 11. Hosting, releases, cost

~$0/month on free tiers (GitHub Pages, Releases, Actions; public repo). Domain and code signing may add cost.
Distribution: Windows `.msi/.exe`, macOS `.dmg` (ARM + Intel), Linux AppImage/`.deb`. CI runs unit tests, golden-image regression tests, 3-OS builds, smoke tests. No auto-update in MVP.

## 12. Phasing

1. `converter` crate + `device-profiles` + golden-image tests (native CLI harness).
2. `wasm` bindings + Next.js site (CBZ/PDF -> XTC). First public release.
3. Desktop shell (Dioxus) with local file import using the same converter.
4. `catalog` crate: Suwayomi sidecar lifecycle (bundled JRE), first-run source setup.
5. `search` crate: fan-out, clustering, ranking, with labelled test set.
6. Series page, chapter selection, download -> convert -> export pipeline, job retry.
7. Installers for all 3 OSes, smoke tests, polish.
8. Optional: ONNX panel detector, auto-update, extra providers.

## 13. Open questions

- Decided: Dioxus for desktop UI; Keiyoushi repo with popular large-catalog starter sources (list to be validated, see section 4).
- Web has **no catalog/download feature** (needs Suwayomi running locally; browsers can't run it or bypass source CORS). It only converts files the user provides.
- `[VERIFY]` XTC/XTCH format spec; X4 Pro resolution and grayscale depth.
- `[VERIFY]` Suwayomi GraphQL schema for search, downloads, page retrieval; whether it supports auth on localhost.
- Page retrieval: GraphQL pages vs. Suwayomi's CBZ download then reuse the importer.
- Is MangaPoint's own SQLite needed beyond jobs, source health and prefs?
- Bundled JRE size and per-OS packaging.
- Dedup thresholds and weights: need a real test set from actual source results.
- WASM performance of dithering/resizing on large series, especially iOS Safari.
- PDF import on web: pdf.js bridge vs. a Rust/WASM PDF renderer.
