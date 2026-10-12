//! The sources offered on first run.
//!
//! Chosen for catalog size and for having passed a full test (popular list,
//! search, chapters, page list, real page image) against Suwayomi v2.4.2366
//! on 2026-10-11. Sources that need a FlareSolverr service (Manganato,
//! Mangakakalot) are left out because the app cannot make them work alone.

/// Keiyoushi's extension store, the largest Mihon-compatible one.
pub const KEIYOUSHI_INDEX: &str = "https://github.com/keiyoushi/extensions/raw/repo/index.pb";

/// Every Mihon extension package starts with this.
pub const EXTENSION_PREFIX: &str = "eu.kanade.tachiyomi.extension.";

/// Shown whenever the user adds a store or installs an extension by hand.
pub const EXTENSION_TRUST_WARNING: &str = "Extensions are programs written by third parties. \
Only add stores and install extensions from people you trust.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requires {
    Nothing,
    /// Works only with browser support turned on.
    Browser,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DefaultSource {
    /// Extension id without the common prefix, e.g. `en.mangapill`.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub requires: Requires,
    /// A title this source is known to carry, used by the test as an extra
    /// check that search returns sensible results.
    pub reference_title: Option<&'static str>,
    /// Date we last saw this source pass a full test.
    pub verified_on: &'static str,
}

impl DefaultSource {
    /// The full extension package name Suwayomi knows it by.
    pub fn pkg_name(&self) -> String {
        format!("{EXTENSION_PREFIX}{}", self.id)
    }
}

const VERIFIED: &str = "2026-10-11";

pub const DEFAULTS: &[DefaultSource] = &[
    DefaultSource {
        id: "all.mangadex",
        name: "MangaDex",
        description: "Huge community catalog. Licensed series often have only a few chapters.",
        requires: Requires::Nothing,
        reference_title: Some("One Piece"),
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "all.mangafire",
        name: "MangaFire",
        description: "Very large catalog with complete chapter lists for popular series.",
        requires: Requires::Nothing,
        reference_title: Some("One Piece"),
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "en.mangapill",
        name: "MangaPill",
        description: "Large catalog, fast to search and download.",
        requires: Requires::Nothing,
        reference_title: Some("One Piece"),
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "en.mangabuddy",
        name: "MangaK",
        description: "Large catalog (MangaBuddy). Fast.",
        requires: Requires::Nothing,
        reference_title: Some("One Piece"),
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "en.mangakatana",
        name: "MangaKatana",
        description: "Large catalog. Works, but downloads can be slow.",
        requires: Requires::Nothing,
        reference_title: Some("One Piece"),
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "en.asurascans",
        name: "Asura Scans",
        description: "Popular action and fantasy manhwa.",
        requires: Requires::Nothing,
        reference_title: None,
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "en.flamecomics",
        name: "Flame Comics",
        description: "Manhwa and webcomics.",
        requires: Requires::Nothing,
        reference_title: None,
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "all.mangaplus",
        name: "MANGA Plus by SHUEISHA",
        description: "Official Shueisha releases. Only the newest chapters of each series.",
        requires: Requires::Nothing,
        reference_title: Some("One Piece"),
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "all.webtoons",
        name: "Webtoons.com",
        description: "Official webcomics.",
        requires: Requires::Nothing,
        reference_title: None,
        verified_on: VERIFIED,
    },
    DefaultSource {
        id: "all.comikey",
        name: "Comikey",
        description:
            "Licensed series. Needs browser support (a one-time download of about 270 MB).",
        requires: Requires::Browser,
        reference_title: None,
        verified_on: "needs browser support; not verified",
    },
];

pub fn find(id: &str) -> Option<&'static DefaultSource> {
    DEFAULTS.iter().find(|d| d.id == id)
}

/// The default whose extension package is `pkg_name`.
pub fn find_by_pkg(pkg_name: &str) -> Option<&'static DefaultSource> {
    pkg_name.strip_prefix(EXTENSION_PREFIX).and_then(find)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_names_are_filled_in() {
        for (i, a) in DEFAULTS.iter().enumerate() {
            assert!(!a.name.is_empty() && !a.description.is_empty());
            for b in &DEFAULTS[i + 1..] {
                assert_ne!(a.id, b.id);
            }
        }
    }

    #[test]
    fn package_names_round_trip() {
        let d = find("en.mangapill").unwrap();
        assert_eq!(d.pkg_name(), "eu.kanade.tachiyomi.extension.en.mangapill");
        assert_eq!(find_by_pkg(&d.pkg_name()), Some(d));
        assert_eq!(find_by_pkg("com.other.extension"), None);
    }

    #[test]
    fn only_browser_sources_are_marked_as_needing_it() {
        let needing: Vec<_> = DEFAULTS
            .iter()
            .filter(|d| d.requires == Requires::Browser)
            .map(|d| d.id)
            .collect();
        assert_eq!(needing, vec!["all.comikey"]);
    }
}
