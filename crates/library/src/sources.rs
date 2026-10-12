//! What the user decided about sources.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The answer to "install this source?" during first-run setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Choice {
    Install,
    /// "Skip for now": not installed, can be revisited in Sources.
    Skip,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceState {
    /// True once the first-run source questions were answered.
    pub onboarding_done: bool,
    /// True once the default extension store was added to the server.
    pub default_store_added: bool,
    /// Answers for the built-in list, keyed by the default's id (`en.mangapill`).
    pub choices: BTreeMap<String, Choice>,
    /// Extension stores the user added themselves.
    pub custom_stores: Vec<String>,
    /// Extensions the user installed by hand (package names).
    pub custom_extensions: BTreeSet<String>,
}

impl SourceState {
    /// Default ids that still have no answer.
    pub fn unanswered<'a>(&self, ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        ids.into_iter()
            .filter(|id| !self.choices.contains_key(*id))
            .map(String::from)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unanswered_lists_only_missing_ids() {
        let mut s = SourceState::default();
        s.choices.insert("a".into(), Choice::Install);
        s.choices.insert("b".into(), Choice::Skip);
        assert_eq!(s.unanswered(["a", "b", "c", "d"]), vec!["c", "d"]);
    }

    #[test]
    fn round_trips() {
        let mut s = SourceState::default();
        s.onboarding_done = true;
        s.custom_stores.push("https://example.org/index.pb".into());
        s.custom_extensions.insert("pkg.one".into());
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<SourceState>(&json).unwrap(), s);
    }
}
