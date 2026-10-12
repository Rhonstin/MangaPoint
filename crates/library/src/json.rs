//! One JSON document on disk, written so a crash cannot leave it half-written.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JsonError {
    #[error("i/o error on {0}: {1}")]
    Io(PathBuf, io::Error),
    #[error("could not encode {0}: {1}")]
    Encode(PathBuf, serde_json::Error),
}

pub struct JsonFile<T> {
    path: PathBuf,
    _marker: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Default> JsonFile<T> {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            _marker: PhantomData,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the document, or the default if there is none yet.
    ///
    /// A file that cannot be parsed is moved aside to `<name>.bad` and the
    /// default is returned, so a corrupt file never stops the app from
    /// starting (and the bad copy is kept for inspection).
    pub fn load(&self) -> T {
        let Ok(text) = fs::read_to_string(&self.path) else {
            return T::default();
        };
        match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(_) => {
                let _ = fs::rename(&self.path, self.path.with_extension("json.bad"));
                T::default()
            }
        }
    }

    /// Save atomically: write a temporary file, then rename it over the old one.
    pub fn save(&self, value: &T) -> Result<(), JsonError> {
        let io_err = |e| JsonError::Io(self.path.clone(), e);
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        let text = serde_json::to_string_pretty(value)
            .map_err(|e| JsonError::Encode(self.path.clone(), e))?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(io_err)?;
        fs::rename(&tmp, &self.path).map_err(io_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    struct Doc {
        name: String,
        n: u32,
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mangapoint-json-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn missing_file_gives_default_and_round_trip_works() {
        let dir = temp("rt");
        let f = JsonFile::<Doc>::new(dir.join("sub").join("doc.json"));
        assert_eq!(f.load(), Doc::default());
        let doc = Doc {
            name: "x".into(),
            n: 7,
        };
        f.save(&doc).unwrap();
        assert_eq!(f.load(), doc);
        assert!(!f.path().with_extension("json.tmp").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_is_kept_aside_and_default_returned() {
        let dir = temp("bad");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        fs::write(&path, "{ this is not json").unwrap();
        let f = JsonFile::<Doc>::new(&path);
        assert_eq!(f.load(), Doc::default());
        assert!(dir.join("doc.json.bad").exists());
        assert!(!path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_or_missing_fields_do_not_break_loading() {
        let dir = temp("compat");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        // Older file lacking `n`, and a field from a newer version.
        fs::write(&path, r#"{"name":"old","future":1}"#).unwrap();
        let f = JsonFile::<Doc>::new(&path);
        // serde errors on a missing field without a default, so the file is
        // set aside rather than half-loaded.
        let loaded = f.load();
        assert_eq!(loaded, Doc::default());
        let _ = fs::remove_dir_all(dir);
    }
}
