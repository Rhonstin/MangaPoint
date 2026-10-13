use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Fractions removed from each edge of the prepared source section.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CropRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}
impl CropRect {
    pub fn normalized(self) -> Self {
        let clean = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 0.99)
            } else {
                0.0
            }
        };
        let pair = |a: f32, b: f32| {
            let (a, b) = (clean(a), clean(b));
            let scale = if a + b > 0.99 { 0.99 / (a + b) } else { 1.0 };
            (a * scale, b * scale)
        };
        let (left, right) = pair(self.left, self.right);
        let (top, bottom) = pair(self.top, self.bottom);
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
    pub fn as_array(self) -> [f32; 4] {
        let c = self.normalized();
        [c.left, c.top, c.right, c.bottom]
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CropScope {
    #[default]
    Page,
    Chapter,
    Series,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CropOverrides {
    pub series: Option<CropRect>,
    pub chapters: BTreeMap<String, CropRect>,
    pub pages: BTreeMap<String, BTreeMap<String, CropRect>>,
}
impl CropOverrides {
    pub fn scope_for(&self, chapter: &str, page_key: &str) -> Option<CropScope> {
        if self
            .pages
            .get(chapter)
            .is_some_and(|p| p.contains_key(page_key))
        {
            Some(CropScope::Page)
        } else if self.chapters.contains_key(chapter) {
            Some(CropScope::Chapter)
        } else if self.series.is_some() {
            Some(CropScope::Series)
        } else {
            None
        }
    }
    pub fn effective(&self, chapter: &str, page_key: &str) -> Option<CropRect> {
        self.pages
            .get(chapter)
            .and_then(|p| p.get(page_key))
            .copied()
            .or_else(|| self.chapters.get(chapter).copied())
            .or(self.series)
            .map(CropRect::normalized)
    }
    pub fn set(&mut self, scope: CropScope, chapter: &str, page_key: &str, crop: Option<CropRect>) {
        let crop = crop.map(CropRect::normalized);
        match scope {
            CropScope::Series => self.series = crop,
            CropScope::Chapter => match crop {
                Some(c) => {
                    self.chapters.insert(chapter.into(), c);
                }
                None => {
                    self.chapters.remove(chapter);
                }
            },
            CropScope::Page => match crop {
                Some(c) => {
                    self.pages
                        .entry(chapter.into())
                        .or_default()
                        .insert(page_key.into(), c);
                }
                None => {
                    if let Some(pages) = self.pages.get_mut(chapter) {
                        pages.remove(page_key);
                        if pages.is_empty() {
                            self.pages.remove(chapter);
                        }
                    }
                }
            },
        }
    }
    /// Canonical effective configuration for a single chapter's cache.
    pub fn chapter_configuration(&self, chapter: &str) -> Vec<u8> {
        let base = self
            .chapters
            .get(chapter)
            .copied()
            .or(self.series)
            .map(CropRect::normalized);
        let pages: BTreeMap<_, _> = self
            .pages
            .get(chapter)
            .into_iter()
            .flat_map(|p| p.iter())
            .map(|(k, c)| (k, c.normalized()))
            .collect();
        if base.is_none() && pages.is_empty() {
            Vec::new()
        } else {
            serde_json::to_vec(&(base, pages)).expect("normalized crops serialize")
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitized_crop_keeps_at_least_one_percent_and_finite_edges() {
        let c = CropRect {
            left: f32::NAN,
            top: -1.0,
            right: 2.0,
            bottom: 0.9,
        }
        .normalized();
        assert_eq!(c.left, 0.0);
        assert_eq!(c.top, 0.0);
        assert!(c.left + c.right <= 0.99);
        assert!(c.top + c.bottom <= 0.99);
    }
    #[test]
    fn scope_reset_reveals_broader_crop_and_old_json_defaults() {
        let mut c = CropOverrides::default();
        let series = CropRect {
            left: 0.1,
            ..Default::default()
        };
        let chapter = CropRect {
            top: 0.2,
            ..Default::default()
        };
        let page = CropRect {
            right: 0.3,
            ..Default::default()
        };
        c.set(CropScope::Series, "c1", "image:0:whole", Some(series));
        c.set(CropScope::Chapter, "c1", "image:0:whole", Some(chapter));
        c.set(CropScope::Page, "c1", "image:0:whole", Some(page));
        assert_eq!(c.scope_for("c1", "image:0:whole"), Some(CropScope::Page));
        assert_eq!(c.scope_for("c1", "other"), Some(CropScope::Chapter));
        assert_eq!(c.scope_for("c2", "other"), Some(CropScope::Series));
        assert_eq!(c.effective("c1", "image:0:whole"), Some(page));
        assert_eq!(c.effective("c1", "other"), Some(chapter));
        assert_eq!(c.effective("c2", "other"), Some(series));
        c.set(CropScope::Page, "c1", "image:0:whole", None);
        assert_eq!(c.effective("c1", "image:0:whole"), Some(chapter));
        let loaded: crate::DownloadedSeries =
            serde_json::from_str(r#"{"id":"old","title":"Old"}"#).unwrap();
        assert_eq!(loaded.crops.effective("c1", "image:0:whole"), None);
        let restored: CropOverrides =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(restored, c);
    }
}
