//! Knowing whether a newer MangaPoint exists, and getting it.
//!
//! The screen for this is in Settings, under About. Nothing is connected behind
//! it yet: [`check`] says there is nothing newer and [`install`] says it cannot.
//! Both are the two places to fill in.

/// The version of this build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the check for a newer version has got to.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    /// Not looked yet.
    Idle,
    /// Looking.
    Checking,
    /// This is the newest.
    UpToDate,
    /// A newer one is there.
    Available { version: String },
    /// Getting it.
    Installing,
    /// It did not work; this says why.
    Failed(String),
}

/// Look for a newer version: `Some(version)` if there is one.
pub async fn check() -> Result<Option<String>, String> {
    // To do: ask where releases are published and compare with VERSION.
    Ok(None)
}

/// Download and put in place the newer `version`.
pub async fn install(_version: &str) -> Result<(), String> {
    // To do: download the release, check it, and restart into it.
    Err("Updating from inside the app is not available yet.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_this_builds() {
        assert!(VERSION.split('.').count() >= 2);
    }

    #[test]
    fn until_it_is_connected_nothing_is_newer_and_nothing_installs() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        assert_eq!(rt.block_on(check()), Ok(None));
        assert!(rt.block_on(install("9.9.9")).is_err());
    }
}
