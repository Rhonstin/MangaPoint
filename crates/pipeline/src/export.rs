//! Getting a finished file onto the reader.

use crate::error::{PipelineError, Result};
use converter::xtc::Format;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use sysinfo::Disks;

/// Where a finished file goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ExportTarget {
    /// A folder on this computer.
    Folder { path: PathBuf },
    /// A mounted reader or SD card; files go into `<mount>/<folder>/`.
    Device { mount: PathBuf, folder: String },
    /// The reader's WebDAV share (CrossPoint's Wi-Fi file transfer).
    /// Untested against real hardware.
    WebDav { base_url: String, folder: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedFile {
    /// A path or URL the user can be shown.
    pub location: String,
    pub bytes: u64,
}

impl ExportTarget {
    pub async fn write(&self, filename: &str, data: &[u8]) -> Result<ExportedFile> {
        match self {
            ExportTarget::Folder { path } => write_file(path, filename, data),
            ExportTarget::Device { mount, folder } => {
                check_space(mount, data.len() as u64)?;
                write_file(&mount.join(sanitize_component(folder)), filename, data)
            }
            ExportTarget::WebDav { base_url, folder } => {
                upload_webdav(base_url, folder, filename, data).await
            }
        }
    }
}

fn write_file(dir: &Path, filename: &str, data: &[u8]) -> Result<ExportedFile> {
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |e| PipelineError::Io(p, e)
    };
    fs::create_dir_all(dir).map_err(io(dir))?;
    let path = unique_path(dir, filename);
    let tmp = path.with_extension("part");
    fs::write(&tmp, data).map_err(io(&tmp))?;
    fs::rename(&tmp, &path).map_err(io(&path))?;
    Ok(ExportedFile {
        location: path.display().to_string(),
        bytes: data.len() as u64,
    })
}

/// `dir/name`, or `dir/name (2)` and so on if that file already exists, so
/// nothing is ever overwritten.
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (filename, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

// ---- Drives ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    pub mount: PathBuf,
    pub name: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// Removable drives currently mounted: SD cards, USB sticks, and a reader in
/// USB drive mode.
pub fn removable_drives() -> Vec<Drive> {
    Disks::new_with_refreshed_list()
        .iter()
        .filter(|d| d.is_removable())
        .map(|d| Drive {
            mount: d.mount_point().to_path_buf(),
            name: d.name().to_string_lossy().into_owned(),
            total_bytes: d.total_space(),
            available_bytes: d.available_space(),
        })
        .collect()
}

/// Fail early, with a clear message, if the card is too full. A mount we
/// cannot find in the drive list is let through rather than blocked.
fn check_space(mount: &Path, needed: u64) -> Result<()> {
    let disks = Disks::new_with_refreshed_list();
    let Some(disk) = disks.iter().find(|d| d.mount_point() == mount) else {
        return Ok(());
    };
    // Leave a little room for the filesystem.
    let needed_with_margin = needed + needed / 20;
    if disk.available_space() < needed_with_margin {
        return Err(PipelineError::NoSpace {
            mount: mount.to_path_buf(),
            needed: needed_with_margin,
            available: disk.available_space(),
        });
    }
    Ok(())
}

// ---- Names -----------------------------------------------------------------

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make `name` safe as one path component on Windows, macOS and Linux:
/// no separators or reserved characters, no trailing dots or spaces, not a
/// reserved device name, at most 100 characters.
pub fn sanitize_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(100).collect();
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if out.is_empty() {
        return "manga".into();
    }
    let stem = out.split('.').next().unwrap_or("").to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.push('_');
    }
    out
}

fn number(n: f64) -> String {
    if n.fract().abs() < 0.0005 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// `Series - Ch 1-12.xtch`, `Series - Ch 5.xtch`, or with a part number
/// `Series - Part 2 - Ch 13-25.xtch`.
pub fn output_file_name(
    series: &str,
    first: Option<f64>,
    last: Option<f64>,
    part: Option<usize>,
    format: Format,
) -> String {
    let mut name = sanitize_component(series);
    if let Some(p) = part {
        name.push_str(&format!(" - Part {p}"));
    }
    match (first, last) {
        (Some(a), Some(b)) if (a - b).abs() < 0.0005 => {
            name.push_str(&format!(" - Ch {}", number(a)))
        }
        (Some(a), Some(b)) => name.push_str(&format!(" - Ch {}-{}", number(a), number(b))),
        _ => {}
    }
    format!("{name}.{}", format.extension())
}

// ---- WebDAV ----------------------------------------------------------------

/// Percent-encode one URL path segment.
fn encode_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn upload_webdav(
    base_url: &str,
    folder: &str,
    filename: &str,
    data: &[u8],
) -> Result<ExportedFile> {
    let fail = |m: String| PipelineError::Upload(m);
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(fail(format!("'{base_url}' is not an http address")));
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| fail(e.to_string()))?;
    let dir_url = format!(
        "{}/{}/",
        base_url.trim_end_matches('/'),
        encode_segment(&sanitize_component(folder))
    );
    // Create the folder; a failure usually just means it already exists.
    let mkcol = reqwest::Method::from_bytes(b"MKCOL").expect("valid method");
    let _ = client.request(mkcol, &dir_url).send().await;

    let file_url = format!("{dir_url}{}", encode_segment(filename));
    let response = client
        .put(&file_url)
        .body(data.to_vec())
        .send()
        .await
        .map_err(|e| fail(e.to_string()))?;
    let status = response.status();
    if status.is_success() {
        Ok(ExportedFile {
            location: file_url,
            bytes: data.len() as u64,
        })
    } else {
        Err(fail(format!("the reader answered {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn temp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("mangapoint-export-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn names_are_safe_on_every_platform() {
        assert_eq!(
            sanitize_component("Re:Zero / Season 1?"),
            "Re_Zero _ Season 1_"
        );
        assert_eq!(sanitize_component("  spaced   out  "), "spaced out");
        assert_eq!(sanitize_component("trailing..."), "trailing");
        assert_eq!(sanitize_component("CON"), "CON_");
        assert_eq!(sanitize_component("nul.txt"), "nul.txt_");
        assert_eq!(sanitize_component(""), "manga");
        assert_eq!(sanitize_component("..."), "manga");
        assert_eq!(sanitize_component("進撃の巨人"), "進撃の巨人");
        assert_eq!(sanitize_component(&"x".repeat(300)).chars().count(), 100);
        assert!(!sanitize_component("../../etc/passwd").contains('/'));
    }

    #[test]
    fn file_names_describe_the_chapters() {
        let n = |a, b, p| output_file_name("Berserk", a, b, p, Format::Xtch);
        assert_eq!(n(Some(1.0), Some(12.0), None), "Berserk - Ch 1-12.xtch");
        assert_eq!(n(Some(5.0), Some(5.0), None), "Berserk - Ch 5.xtch");
        assert_eq!(
            n(Some(5.5), Some(6.0), Some(2)),
            "Berserk - Part 2 - Ch 5.5-6.xtch"
        );
        assert_eq!(n(None, None, None), "Berserk.xtch");
        assert_eq!(
            output_file_name("A/B", Some(1.0), Some(2.0), None, Format::Xtc),
            "A_B - Ch 1-2.xtc"
        );
    }

    #[tokio::test]
    async fn folder_export_writes_and_never_overwrites() {
        let dir = temp("folder");
        let target = ExportTarget::Folder {
            path: dir.join("out"),
        };
        let a = target.write("Book.xtch", b"one").await.unwrap();
        let b = target.write("Book.xtch", b"two").await.unwrap();
        let c = target.write("Book.xtch", b"three").await.unwrap();
        assert!(a.location.ends_with("Book.xtch"));
        assert!(b.location.ends_with("Book (2).xtch"), "{}", b.location);
        assert!(c.location.ends_with("Book (3).xtch"));
        assert_eq!(fs::read(&a.location).unwrap(), b"one");
        assert_eq!(fs::read(&b.location).unwrap(), b"two");
        assert!(fs::read_dir(dir.join("out"))
            .unwrap()
            .all(|e| { e.unwrap().path().extension().is_some_and(|x| x != "part") }));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn device_export_goes_into_a_subfolder_with_a_safe_name() {
        let dir = temp("device");
        fs::create_dir_all(&dir).unwrap();
        let target = ExportTarget::Device {
            mount: dir.clone(),
            folder: "Manga/../Evil".into(),
        };
        let f = target.write("Book.xtc", b"data").await.unwrap();
        let path = PathBuf::from(&f.location);
        assert!(path.starts_with(&dir), "stays inside the card: {path:?}");
        assert_eq!(path.parent().unwrap().parent().unwrap(), dir.as_path());
        assert_eq!(fs::read(&path).unwrap(), b"data");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unique_path_handles_names_without_extensions() {
        let dir = temp("unique");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plain"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "plain"), dir.join("plain (2)"));
        assert_eq!(unique_path(&dir, "fresh.xtc"), dir.join("fresh.xtc"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn listing_drives_does_not_fail() {
        // Whatever is mounted on this machine, it must not panic.
        for d in removable_drives() {
            assert!(d.available_bytes <= d.total_bytes);
        }
    }

    #[test]
    fn a_mount_we_cannot_find_is_not_blocked() {
        assert!(check_space(Path::new("/definitely/not/mounted"), u64::MAX / 4).is_ok());
    }

    #[test]
    fn url_segments_are_encoded() {
        assert_eq!(
            encode_segment("Berserk - Ch 1-12.xtch"),
            "Berserk%20-%20Ch%201-12.xtch"
        );
        assert_eq!(encode_segment("a/b"), "a%2Fb");
        assert_eq!(encode_segment("進"), "%E9%80%B2");
    }

    /// Each request the fake server saw: its first line and its body.
    type Seen = Vec<(String, Vec<u8>)>;

    /// A one-shot HTTP server that records each request and replies `status`.
    fn fake_server(
        status: &'static str,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<Seen>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head_end, content_length) = loop {
                    let n = stream.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (pos + 4, len);
                    }
                    if n == 0 {
                        break (buf.len(), 0);
                    }
                };
                while buf.len() < head_end + content_length {
                    let n = stream.read(&mut chunk).unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let request_line = String::from_utf8_lossy(&buf)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                seen.push((request_line, buf[head_end..].to_vec()));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
            seen
        });
        (url, handle)
    }

    #[tokio::test]
    async fn webdav_creates_the_folder_then_puts_the_file() {
        let (url, server) = fake_server("201 Created", 2);
        let target = ExportTarget::WebDav {
            base_url: url.clone(),
            folder: "Manga".into(),
        };
        let f = target.write("My Book.xtch", b"payload").await.unwrap();
        let seen = server.join().unwrap();
        assert!(seen[0].0.starts_with("MKCOL /Manga/ "), "{:?}", seen[0]);
        assert!(
            seen[1].0.starts_with("PUT /Manga/My%20Book.xtch "),
            "{:?}",
            seen[1]
        );
        assert_eq!(seen[1].1, b"payload");
        assert_eq!(f.location, format!("{url}/Manga/My%20Book.xtch"));
    }

    #[tokio::test]
    async fn webdav_errors_are_reported() {
        let (url, server) = fake_server("507 Insufficient Storage", 2);
        let target = ExportTarget::WebDav {
            base_url: url,
            folder: "Manga".into(),
        };
        let err = target.write("b.xtc", b"x").await.unwrap_err();
        server.join().unwrap();
        assert!(matches!(err, PipelineError::Upload(m) if m.contains("507")));

        let err = ExportTarget::WebDav {
            base_url: "ftp://reader".into(),
            folder: "m".into(),
        }
        .write("b.xtc", b"x")
        .await
        .unwrap_err();
        assert!(matches!(err, PipelineError::Upload(_)));
    }
}
