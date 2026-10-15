//! The app's own log lines, kept in a file: new lines are added to the end
//! (the file is not made again at every start), and when it reaches its limit
//! it is rolled over, keeping only a few older copies, so the logs can never
//! grow without end.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

/// How big the log gets before it is rolled over.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// How many older logs are kept (`mangapoint.log.1` is the newest of them).
const KEEP_OLD: usize = 3;
const NAME: &str = "mangapoint.log";

struct Inner {
    dir: PathBuf,
    file: Option<File>,
    written: u64,
    max: u64,
    keep: usize,
}

impl Inner {
    fn path(dir: &Path, n: usize) -> PathBuf {
        if n == 0 {
            dir.join(NAME)
        } else {
            dir.join(format!("{NAME}.{n}"))
        }
    }

    fn open(dir: &Path) -> io::Result<(File, u64)> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(Self::path(dir, 0))?;
        let written = file.metadata().map_or(0, |m| m.len());
        Ok((file, written))
    }

    /// The log becomes `.1`, `.1` becomes `.2`, and so on; the oldest goes.
    fn roll(&mut self) -> io::Result<()> {
        for n in (1..self.keep).rev() {
            let from = Self::path(&self.dir, n);
            if from.exists() {
                let _ = fs::rename(&from, Self::path(&self.dir, n + 1));
            }
        }
        // The file is still open, which Windows will not let be renamed.
        self.file = None;
        let _ = fs::rename(Self::path(&self.dir, 0), Self::path(&self.dir, 1));
        let (file, written) = Self::open(&self.dir)?;
        self.file = Some(file);
        self.written = written;
        Ok(())
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<()> {
        if self.written + buf.len() as u64 > self.max && self.written > 0 {
            self.roll()?;
        }
        if let Some(file) = &mut self.file {
            file.write_all(buf)?;
            self.written += buf.len() as u64;
        }
        Ok(())
    }
}

/// The log file, shared by everything that writes to it.
#[derive(Clone)]
pub struct LogFile(Arc<Mutex<Inner>>);

impl LogFile {
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        Self::open_with(dir, MAX_BYTES, KEEP_OLD)
    }

    fn open_with(dir: PathBuf, max: u64, keep: usize) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let (file, written) = Inner::open(&dir)?;
        Ok(Self(Arc::new(Mutex::new(Inner {
            dir,
            file: Some(file),
            written,
            max,
            keep: keep.max(1),
        }))))
    }
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(mut inner) = self.0.lock() {
            inner.write(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Ok(mut inner) = self.0.lock() {
            if let Some(file) = &mut inner.file {
                file.flush()?;
            }
        }
        Ok(())
    }
}

/// Where the log lines go: the file and, in a debug build, the terminal too.
#[derive(Clone)]
pub struct Sink(pub Option<LogFile>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(debug_assertions)]
        {
            let _ = io::stderr().write_all(buf);
        }
        if let Some(file) = &mut self.0 {
            let _ = file.write(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = &mut self.0 {
            let _ = file.flush();
        }
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Sink {
    type Writer = Sink;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mangapoint-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn lines_are_added_to_the_end_of_the_file_across_starts() {
        let dir = scratch("append");
        let mut first = LogFile::open(dir.clone()).unwrap();
        first.write_all(b"first run\n").unwrap();
        drop(first);
        let mut second = LogFile::open(dir.clone()).unwrap();
        second.write_all(b"second run\n").unwrap();
        let text = fs::read_to_string(dir.join(NAME)).unwrap();
        assert_eq!(text, "first run\nsecond run\n");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_log_is_rolled_over_at_its_limit_and_only_a_few_copies_stay() {
        let dir = scratch("roll");
        let mut log = LogFile::open_with(dir.clone(), 100, 2).unwrap();
        for n in 0..40 {
            log.write_all(format!("line number {n:03}\n").as_bytes()).unwrap();
        }
        let size = |name: &str| fs::metadata(dir.join(name)).map(|m| m.len()).ok();
        assert!(size(NAME).unwrap() <= 100, "the current log stays within the limit");
        assert!(size("mangapoint.log.1").is_some() && size("mangapoint.log.2").is_some());
        assert!(size("mangapoint.log.3").is_none(), "older copies are dropped");
        let newest = fs::read_to_string(dir.join(NAME)).unwrap();
        assert!(newest.contains("line number 039"));
        let _ = fs::remove_dir_all(dir);
    }
}
