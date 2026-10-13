# Page Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or superpowers:subagent-driven-development to implement task-by-task. Checkboxes track progress.

**Goal:** Implement every item in the user-approved attached page editor design, including persisted crops, matching export pixels, visual settings, responsive rendering, and downloads polish.

**Architecture:** Keep the existing Rust/Dioxus desktop app and shared conversion pipeline. Persist normalized manual crop overrides on the downloaded series with page > chapter > series precedence. Identify pages by their source image/part or stable strip-unit section; crop the prepared source before fitting, tone adjustment and dithering. The focused editor and export call the same renderer.

**Tech Stack:** Rust, Dioxus 0.7, CSS, existing grayscale/XTC conversion and JSON persistence.

**Spec:** `C:/Users/msleo/.codex/attachments/ecaa1b54-8564-46a3-aef5-bc825fb44330/pasted-text-1.txt`

## Global Constraints

- Reader output remains device-sized, including 480 × 800 for X4 Pro, without distorted aspect ratios.
- Original downloaded images remain unchanged; old JSON loads with no overrides.
- Manual crop scope is explicit; overrides persist and invalidate the affected chapter cache.
- Preserve prior export performance optimizations and shared preview/export pixels.
- Commit locally after verification; do not push this new work without a new request.
- Stop any dev/test server started for verification before finishing.

## Review Focus

- Very tall strips use a source section with its own scroll/overview, never control output page size.
- Split spread halves remain selectable individually and keep the correct source/crop identity.
- Page selection survives redraw; obsolete generations never replace current settings.
- Reopening persisted page/chapter/series crops yields the same exported pixels, with narrower scopes taking precedence.
- Small windows and 100%/200% zoom scroll within panes without stretching or changing the layout.

## Tasks

- [x] **1. Persisted crop rendering** — Add `CropRect`, `CropScope`, `CropOverrides` in library; add `DownloadedSeries.crops` with serde default. Add converter source-section and cropped rendering APIs; pipeline preview helper and `crop_page_key`. Include effective chapter crop settings in cache identity. Tests: sanitization, scope precedence, old JSON, spread/strip behavior, preview/export equality, cache invalidation.
- [x] **2. Focused stable editor** — Replace preview row sizing with a viewport-height canvas and separate source/result panes. Add thumbnails, selected page, overview, chapter/page navigation, keyboard shortcuts, reader/preset inspector, zoom controls. Tests: selection bounds, viewport styles, responsive portrait/spread/strip QA.
- [x] **3. Direct crop interaction** — Add edges/corners and crop shield, immediate draft interaction, precise numeric percentages, explicit Page/Chapter/Series scope, apply/reset. Store overrides from task 1 and show effective/overridden scope. Verify actual drag and reload/export pixels.
- [x] **4. Visual settings** — Illustrated layout/spread/crop/direction choices, shading samples from artwork, contrast neutral/reset, relevant advanced sections; preserve existing settings callbacks and preset differences.
- [x] **5. Responsive rendering** — Render the selected page first and visible/nearby pages next; background thumbnails only as needed. Keep old images during redraw and generation guards. Navigation to an unrendered page prioritizes it immediately; handles remain independent from render completion.
- [x] **6. Downloads polish** — Consistent row heights/alignment/status labels/surfaces; distinct active/downloaded/exported sections and Open in editor actions.
- [x] **7. Verification and build** — Run full workspace tests and release build. Use isolated test data and app webview diagnostics to verify portrait, spread, short page, long strip, keyboard selection, scope persistence, crop dragging, and source-independent sizing at multiple viewport sizes. Capture screenshots, review final changes, stop test processes, commit locally.

## Progress

- Plan recorded from the approved attached design. Existing branch: crop-performance-rewrite; initial HEAD ff13936; worktree clean.

- Implemented all seven tasks. Crop overrides persist with Page > Chapter > Series precedence and share the export renderer; affected-chapter cache invalidation and old-library compatibility are covered by tests.
- Full release workspace tests: 473 passed, 0 failed, 3 existing performance benchmarks ignored. Formatting and whitespace checks passed.
- Live QA used an isolated library with portrait, split spread, short page, and joined long-strip fixtures. Every reader image was 480 × 800 and displayed at 226 × 378 in the 1180 × 800 viewport, with no result-pane overflow. A 1000 × 700 viewport used Source/Reader tabs and fit the full reader page. Also captured a 1600 × 1000 viewport.
- Verified edge/corner dragging, immediate shield and numeric updates, saved crop after app restart, Cancel restoration, same-page draft retention, arrow navigation, Escape, uniform overview, last-strip-section priority, artwork shading samples, downloads rows, and Open in editor.
- Independent final code review found no remaining important issues. Full results/source PNGs are generated for the selected page; visible/background navigator images are 160px thumbnails.
- Release artifact: `.work/editor-build/MangaPoint.exe`. QA app and its catalog server were shut down; owned PIDs 64748/18892 and ports 60873/50864/9338 are no longer active. No pushes performed.