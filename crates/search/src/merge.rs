//! Combining chapter lists from several sources into one.
//!
//! The merged list has each chapter number once, and for each chapter an
//! ordered list of sources that can supply it. Downloading tries them in
//! order, so a missing or failing chapter falls back to the next source.

use catalog::Chapter;

/// Chapter numbers closer than this are the same chapter.
const SAME_NUMBER: f64 = 0.0005;

/// One way to get a chapter.
#[derive(Clone, Debug, PartialEq)]
pub struct ChapterOption {
    /// Index into the member list passed to [`merge_chapters`] (best source first).
    pub member: usize,
    pub chapter_id: i64,
    pub name: String,
    pub upload_date: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MergedChapter {
    /// `None` for chapters the source did not number (extras, notices).
    pub number: Option<f64>,
    /// Display name from the best source offering it.
    pub name: String,
    /// Sources able to supply it, best first.
    pub options: Vec<ChapterOption>,
}

/// Merge per-source chapter lists. `members` must be ordered best source
/// first; that order is kept within each chapter's options. The result is in
/// ascending chapter order, with unnumbered chapters after the numbered ones.
pub fn merge_chapters(members: &[Vec<Chapter>]) -> Vec<MergedChapter> {
    let mut numbered: Vec<MergedChapter> = Vec::new();
    let mut unnumbered: Vec<MergedChapter> = Vec::new();

    for (member, chapters) in members.iter().enumerate() {
        for ch in chapters {
            let option = ChapterOption {
                member,
                chapter_id: ch.id,
                name: ch.name.clone(),
                upload_date: ch.upload_date,
            };
            // Suwayomi uses a negative number for "unknown".
            if ch.chapter_number < 0.0 {
                unnumbered.push(MergedChapter {
                    number: None,
                    name: ch.name.clone(),
                    options: vec![option],
                });
                continue;
            }
            match numbered.iter_mut().find(|m| {
                m.number
                    .is_some_and(|n| (n - ch.chapter_number).abs() < SAME_NUMBER)
            }) {
                Some(existing) => {
                    // A source lists a number once; ignore a repeat from the same source.
                    if !existing.options.iter().any(|o| o.member == member) {
                        existing.options.push(option);
                    }
                }
                None => numbered.push(MergedChapter {
                    number: Some(ch.chapter_number),
                    name: ch.name.clone(),
                    options: vec![option],
                }),
            }
        }
    }
    numbered.sort_by(|a, b| a.number.unwrap_or(0.0).total_cmp(&b.number.unwrap_or(0.0)));
    for m in &mut numbered {
        m.options.sort_by_key(|o| o.member);
        // The best source's name is the display name.
        m.name = m.options[0].name.clone();
    }
    numbered.extend(unnumbered);
    numbered
}

/// Whole chapter numbers missing from the run of chapters, e.g. `[57]` when
/// 56 and 58 exist. Fractional chapters (56.5) are ignored, since they are
/// extras rather than gaps.
pub fn missing_numbers(merged: &[MergedChapter]) -> Vec<u32> {
    let mut have: Vec<u32> = merged
        .iter()
        .filter_map(|m| m.number)
        .filter(|n| n.fract().abs() < SAME_NUMBER && *n >= 1.0)
        .map(|n| n.round() as u32)
        .collect();
    have.sort_unstable();
    have.dedup();
    let (Some(&first), Some(&last)) = (have.first(), have.last()) else {
        return Vec::new();
    };
    (first..=last)
        .filter(|n| have.binary_search(n).is_err())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::chapter;

    fn ch(id: i64, number: f64) -> Chapter {
        chapter(id, number, 0)
    }

    #[test]
    fn same_numbers_merge_and_keep_source_order() {
        let best = vec![ch(1, 1.0), ch(2, 2.0)];
        let other = vec![ch(11, 2.0), ch(12, 3.0)];
        let merged = merge_chapters(&[best, other]);
        let numbers: Vec<_> = merged.iter().map(|m| m.number).collect();
        assert_eq!(numbers, [Some(1.0), Some(2.0), Some(3.0)]);
        // Chapter 2 is available from both, best source first.
        let two = &merged[1];
        assert_eq!(
            two.options
                .iter()
                .map(|o| (o.member, o.chapter_id))
                .collect::<Vec<_>>(),
            [(0, 2), (1, 11)]
        );
        // Chapter 3 only from the second source.
        assert_eq!(merged[2].options.len(), 1);
        assert_eq!(merged[2].options[0].member, 1);
    }

    #[test]
    fn output_is_sorted_even_if_sources_list_newest_first() {
        let desc = vec![ch(3, 3.0), ch(2, 2.0), ch(1, 1.0)];
        let merged = merge_chapters(&[desc]);
        let numbers: Vec<_> = merged.iter().filter_map(|m| m.number).collect();
        assert_eq!(numbers, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn fractional_chapters_stay_distinct_and_float_noise_is_tolerated() {
        let a = vec![ch(1, 56.0), ch(2, 56.5), ch(3, 57.0)];
        let b = vec![ch(11, 56.500_000_1)];
        let merged = merge_chapters(&[a, b]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[1].options.len(), 2, "56.5 from both sources");
    }

    #[test]
    fn unnumbered_chapters_go_last_and_are_never_merged() {
        let a = vec![ch(1, 1.0), ch(2, -1.0)];
        let b = vec![ch(11, -1.0)];
        let merged = merge_chapters(&[a, b]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].number, Some(1.0));
        assert!(merged[1..]
            .iter()
            .all(|m| m.number.is_none() && m.options.len() == 1));
    }

    #[test]
    fn a_source_repeating_a_number_does_not_duplicate_options() {
        let a = vec![ch(1, 5.0), ch(2, 5.0)];
        let merged = merge_chapters(&[a]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].options.len(), 1);
    }

    #[test]
    fn display_name_comes_from_the_best_source() {
        let mut a = ch(1, 1.0);
        a.name = "Chapter 1: Romance Dawn".into();
        let mut b = ch(2, 1.0);
        b.name = "Ch. 1".into();
        let merged = merge_chapters(&[vec![a], vec![b]]);
        assert_eq!(merged[0].name, "Chapter 1: Romance Dawn");
    }

    #[test]
    fn gaps_are_found_but_extras_and_edges_are_not_gaps() {
        // 3 and 6 are missing; 5.5 is an extra next to the real chapter 5.
        let list = vec![
            ch(1, 1.0),
            ch(2, 2.0),
            ch(4, 4.0),
            ch(5, 5.0),
            ch(6, 5.5),
            ch(7, 7.0),
        ];
        let merged = merge_chapters(&[list]);
        assert_eq!(missing_numbers(&merged), [3, 6]);
        // A half chapter alone does not count as having the whole chapter.
        let only_half = vec![ch(1, 1.0), ch(2, 2.5), ch(3, 3.0)];
        assert_eq!(missing_numbers(&merge_chapters(&[only_half])), [2]);
        assert!(missing_numbers(&[]).is_empty());
        // Starting at chapter 10 is not "chapters 1-9 missing".
        assert!(missing_numbers(&merge_chapters(&[vec![ch(1, 10.0), ch(2, 11.0)]])).is_empty());
    }

    #[test]
    fn merging_fills_a_gap_from_another_source() {
        let a = vec![ch(1, 1.0), ch(3, 3.0)];
        let b = vec![ch(12, 2.0)];
        let merged = merge_chapters(&[a, b]);
        assert!(missing_numbers(&merged).is_empty());
    }
}
