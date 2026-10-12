//! Small command-line helper for the XTC format.
//!
//!   xtc-tool testpattern <out-dir>   write calibration files for a device
//!   xtc-tool inspect <file>          validate a file the way the firmware would

use converter::dither::{quantize, Dither};
use converter::testpattern;
use converter::xtc::{Chapter, Format, Metadata, ReadDirection, XtcBuilder, XtcFile};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [cmd, dir] if cmd == "testpattern" => write_test_patterns(Path::new(dir)),
        [cmd, file] if cmd == "inspect" => inspect(Path::new(file)),
        _ => {
            eprintln!("usage:\n  xtc-tool testpattern <out-dir>\n  xtc-tool inspect <file>");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn write_test_patterns(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let patterns = testpattern::all();
    for format in [Format::Xtc, Format::Xtch] {
        let mut builder = XtcBuilder::new(format)
            .direction(ReadDirection::LeftToRight)
            .metadata(Metadata {
                title: format!("MangaPoint test pattern ({})", format.extension()),
                author: "MangaPoint".into(),
                language: "en-US".into(),
                ..Default::default()
            })
            .chapters(
                patterns
                    .iter()
                    .enumerate()
                    .map(|(i, p)| Chapter {
                        name: p.name.to_string(),
                        start_page: i as u16,
                        end_page: i as u16,
                    })
                    .collect(),
            );
        for p in &patterns {
            // Plain threshold on purpose: the bands and orientation page use
            // exact levels, so dithering would only add noise to a diagnosis.
            let page = quantize(&p.image, format.depth(), Dither::Threshold);
            builder.add_page(&page).map_err(|e| e.to_string())?;
        }
        let bytes = builder.build().map_err(|e| e.to_string())?;
        let path = dir.join(format!("mangapoint-testpattern.{}", format.extension()));
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
    println!("\nCopy a file to the device and open it. See the doc comments in");
    println!("crates/converter/src/testpattern.rs for what each page should show.");
    Ok(())
}

fn inspect(path: &Path) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let file =
        XtcFile::parse(&data).map_err(|e| format!("firmware would reject this file: {e}"))?;
    let h = &file.header;
    println!("format:        {:?}", file.format);
    println!("version:       {}.{}", h.version_major, h.version_minor);
    println!("pages:         {}", h.page_count);
    println!("direction:     {:?}", file.read_direction());
    println!("title:         {:?}", file.title);
    println!("author:        {:?}", file.author);
    println!(
        "page table at: {}   data at: {}",
        h.page_table_offset, h.data_offset
    );
    for c in &file.chapters {
        println!(
            "chapter:       {:?} pages {}..={}",
            c.name, c.start_page, c.end_page
        );
    }
    for i in 0..file.page_count() {
        let info = file.page_info(i).map_err(|e| e.to_string())?;
        file.decode_page(i).map_err(|e| e.to_string())?;
        if i < 5 || i + 1 == file.page_count() {
            println!(
                "page {i:>4}: {}x{} {} bytes ok",
                info.width, info.height, info.size
            );
        } else if i == 5 {
            println!("   ...");
        }
    }
    println!("all {} pages decode correctly", file.page_count());
    Ok(())
}
