# Changelog

All notable changes to MangaPoint are written down here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-10-15

The first release.

### Features

- Search many sources at once; results for the same series are grouped and the best source is chosen from its chapter list. Covers, descriptions and details come from whichever source answers first.
- Download chapters through a queue with pause, resume, retry and reordering, with a library of what you have.
- Convert pages for the reader's own screen: automatic and manual crop, double-page spreads, long strips cut into slices, contrast and dithering, with a live preview and presets.
- Export as XTC, XTCH or a fixed-layout EPUB 3 (chapters as the table of contents, reading direction, cover), to a folder, a USB drive or SD card, or over Wi-Fi.
- A catalog of 29 readers (Xteink, Amazon, Kobo, Boox, PocketBook, reMarkable) with their sizes, densities and file formats; add your own. Each reader has its own folder, export method, firmware and formats, and the Export screen only offers the formats the reader opens.
- A first-run setup (engine, sources, reader), notifications when a download or an export finishes, keyboard shortcuts, and settings that warn before unsaved changes are lost.

### Performance

- Covers are served on demand from a small disk cache instead of being held in memory, and the chapter table draws only the rows in view, so long series stay light.
- Search shows its results as soon as every source has answered.
- Exports run on their own threads, and the screens no longer redraw for every downloaded page.

### Build

- Continuous integration on Windows, Linux and macOS, and automatic packaging and publishing of releases.
