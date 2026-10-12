//! End-to-end checks: build whole files and read them back with the parser
//! that mirrors CrossPoint's.

use converter::dither::{quantize, Dither};
use converter::gray::GrayImage;
use converter::testpattern;
use converter::xtc::XtcBuilder;
use converter::xtc::{
    Chapter, Format, Metadata, ReadDirection, XtcError, XtcFile, HEADER_SIZE, METADATA_SIZE,
    PAGE_TABLE_ENTRY_SIZE,
};

fn build(format: Format, pages: usize, chapters: Vec<Chapter>, with_meta: bool) -> Vec<u8> {
    let mut b = XtcBuilder::new(format).direction(ReadDirection::RightToLeft);
    if with_meta {
        b = b.metadata(Metadata {
            title: "Test Title".into(),
            author: "Test Author".into(),
            language: "en-US".into(),
            create_time: 1_700_000_000,
            cover_page: Some(0),
            ..Default::default()
        });
    }
    b = b.chapters(chapters);
    let patterns = testpattern::all();
    for i in 0..pages {
        let img = &patterns[i % patterns.len()].image;
        b.add_page(&quantize(img, format.depth(), Dither::FloydSteinberg))
            .unwrap();
    }
    b.build().unwrap()
}

#[test]
fn xtc_and_xtch_files_round_trip_every_page() {
    for format in [Format::Xtc, Format::Xtch] {
        let patterns = testpattern::all();
        let bytes = build(format, patterns.len(), vec![], true);
        let file = XtcFile::parse(&bytes).unwrap();
        assert_eq!(file.format, format);
        assert_eq!(file.page_count(), patterns.len());
        for (i, p) in patterns.iter().enumerate() {
            let expected = quantize(&p.image, format.depth(), Dither::FloydSteinberg);
            let page = file.decode_page(i).unwrap();
            assert_eq!(page.image, expected, "{format:?} page {i}");
            let info = file.page_info(i).unwrap();
            assert_eq!((info.width, info.height), (480, 800));
        }
    }
}

#[test]
fn metadata_and_direction_survive() {
    let bytes = build(Format::Xtch, 2, vec![], true);
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.title, "Test Title");
    assert_eq!(file.author, "Test Author");
    assert_eq!(file.read_direction(), Some(ReadDirection::RightToLeft));
}

#[test]
fn chapters_are_found_by_the_firmware_algorithm() {
    // Several chapters and metadata: the firmware derives the chapter count
    // from the gap before the page table, so any padding would show up here
    // as an extra bogus chapter.
    let chapters = vec![
        Chapter {
            name: "Chapter 1".into(),
            start_page: 0,
            end_page: 1,
        },
        Chapter {
            name: "第二章".into(),
            start_page: 2,
            end_page: 2,
        },
        Chapter {
            name: "Chapter 3: a very long name ".repeat(5),
            start_page: 3,
            end_page: 3,
        },
    ];
    let bytes = build(Format::Xtc, 4, chapters.clone(), true);
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.chapters.len(), 3);
    assert_eq!(file.chapters[0].name, "Chapter 1");
    assert_eq!(file.chapters[1].name, "第二章");
    assert_eq!(
        (file.chapters[1].start_page, file.chapters[1].end_page),
        (2, 2)
    );
    // Over-long names are truncated to 79 bytes, never rejected.
    assert!(file.chapters[2].name.len() <= 79);
    assert!(file.chapters[2].name.starts_with("Chapter 3: a very long"));
}

#[test]
fn chapters_without_metadata_still_parse() {
    let chapters = vec![Chapter {
        name: "Only".into(),
        start_page: 0,
        end_page: 0,
    }];
    let bytes = build(Format::Xtc, 1, chapters, false);
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.chapters.len(), 1);
    assert_eq!(file.title, "");
}

#[test]
fn file_size_matches_layout_arithmetic() {
    let bytes = build(Format::Xtch, 3, vec![], true);
    let expected = HEADER_SIZE + METADATA_SIZE + 3 * PAGE_TABLE_ENTRY_SIZE + 3 * (22 + 96_000);
    assert_eq!(bytes.len(), expected);

    let bytes = build(Format::Xtc, 3, vec![], false);
    let expected = HEADER_SIZE + 3 * PAGE_TABLE_ENTRY_SIZE + 3 * (22 + 48_000);
    assert_eq!(bytes.len(), expected);
}

#[test]
fn parser_rejects_what_the_firmware_rejects() {
    let good = build(Format::Xtc, 2, vec![], true);

    assert_eq!(
        XtcFile::parse(&good[..30]).err(),
        Some(XtcError::Truncated("shorter than the 56-byte header"))
    );

    let mut bad = good.clone();
    bad[0] = b'Z';
    assert!(matches!(
        XtcFile::parse(&bad),
        Err(XtcError::InvalidMagic(_))
    ));

    let mut bad = good.clone();
    bad[4] = 9;
    assert!(matches!(
        XtcFile::parse(&bad),
        Err(XtcError::InvalidVersion { .. })
    ));

    let mut bad = good.clone();
    bad[6] = 0;
    bad[7] = 0;
    assert!(matches!(
        XtcFile::parse(&bad),
        Err(XtcError::CorruptedHeader(_))
    ));

    // Page table pointing past the end of the file.
    let mut bad = good.clone();
    bad[0x18..0x20].copy_from_slice(&(good.len() as u64 + 10).to_le_bytes());
    assert!(matches!(
        XtcFile::parse(&bad),
        Err(XtcError::CorruptedHeader(_))
    ));

    // Page data cut short.
    let cut = &good[..good.len() - 1000];
    let file = XtcFile::parse(cut).unwrap();
    assert!(file.decode_page(1).is_err());
    assert!(file.decode_page(5).is_err());
}

#[test]
fn xtch_page_in_xtc_container_is_rejected() {
    let mut b = XtcBuilder::new(Format::Xtch);
    let page = quantize(
        &GrayImage::new(16, 16, 255).unwrap(),
        Format::Xtch.depth(),
        Dither::Threshold,
    );
    b.add_page(&page).unwrap();
    let mut bytes = b.build().unwrap();
    // Relabel the container as XTC (1-bit) without touching the page.
    bytes[0..4].copy_from_slice(&converter::xtc::XTC_MAGIC.to_le_bytes());
    let file = XtcFile::parse(&bytes).unwrap();
    assert!(file.decode_page(0).is_err());
}

#[test]
fn a_full_size_book_builds_and_reads_back() {
    // 300 device-size pages, 2-bit.
    let img = quantize(
        &testpattern::all()[3].image,
        Format::Xtch.depth(),
        Dither::Atkinson,
    );
    let mut b = XtcBuilder::new(Format::Xtch);
    for _ in 0..300 {
        b.add_page(&img).unwrap();
    }
    let bytes = b.build().unwrap();
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.page_count(), 300);
    assert_eq!(file.decode_page(299).unwrap().image, img);
}

/// Whatever the device's screen, a planned chapter encodes to a file that
/// reads back page for page at exactly that size, in both formats.
#[test]
fn planned_chapters_round_trip_for_every_device_size() {
    use converter::plan::convert_chapter;
    use converter::{Layout, PageEdits, ProcessOptions};

    let images: Vec<GrayImage> = (0..4)
        .map(|i| {
            GrayImage::from_fn(700, 1400, |x, y| ((x / 7 + y / 9 + i * 40) % 256) as u8).unwrap()
        })
        .collect();
    for (w, h) in [(480, 800), (528, 792), (758, 1024), (300, 200)] {
        for format in [Format::Xtc, Format::Xtch] {
            let mut opts = ProcessOptions::manga_default(w, h, format.depth());
            opts.layout = Layout::Webtoon;
            let pages = convert_chapter(&images, &opts, &PageEdits::default());
            assert!(pages.len() >= 2, "{w}x{h}");
            let mut b = XtcBuilder::new(format);
            for page in &pages {
                b.add_page(page).unwrap();
            }
            let bytes = b.build().unwrap();
            let file = XtcFile::parse(&bytes).unwrap();
            assert_eq!(file.page_count(), pages.len());
            for (i, expected) in pages.iter().enumerate() {
                let info = file.page_info(i).unwrap();
                assert_eq!((info.width as u32, info.height as u32), (w, h));
                assert_eq!(
                    &file.decode_page(i).unwrap().image,
                    expected,
                    "{w}x{h} {format:?} page {i}"
                );
            }
        }
    }
}
