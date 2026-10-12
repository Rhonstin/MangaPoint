//! Making titles comparable.

use strsim::jaro_winkler;
use unicode_normalization::UnicodeNormalization;

/// Words that, alone inside parentheses, only describe the edition and say
/// nothing about which series it is: `Berserk (Colored)`.
const EDITION_TAGS: &[&str] = &[
    "official",
    "colored",
    "coloured",
    "color",
    "colour",
    "digital",
    "uncensored",
    "english",
    "en",
    "raw",
    "webtoon",
    "webtoons",
    "manga",
    "manhwa",
    "manhua",
    "fan",
    "translated",
    "remastered",
    "new",
    "full",
    "scan",
    "scans",
];

/// Lowercase, strip accents and punctuation, drop edition tags such as
/// `[Official]` or `(Colored)`, collapse spaces and ignore a leading "the".
///
/// ```text
/// "The Greatest Estate Developer [Official]" -> "greatest estate developer"
/// "Sōlo-Leveling (Colored)"                  -> "solo leveling"
/// ```
pub fn normalize_title(title: &str) -> String {
    // Decompose, drop Latin accents (but not kana voicing marks), recompose.
    let folded: String = title
        .nfkd()
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
        .nfc()
        .collect();
    let without_brackets = strip_square_brackets(&folded);
    let without_tags = strip_edition_parens(&without_brackets);
    let spaced: String = without_tags
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut words: Vec<&str> = spaced.split_whitespace().collect();
    if words.len() > 1 && words[0] == "the" {
        words.remove(0);
    }
    words.join(" ")
}

fn strip_square_brackets(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Remove `( ... )` groups made only of edition words.
fn strip_edition_parens(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')').map(|i| open + i) else {
            break;
        };
        let inner = &rest[open + 1..close];
        let words: Vec<String> = inner
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect();
        out.push_str(&rest[..open]);
        let is_tag = !words.is_empty() && words.iter().all(|w| EDITION_TAGS.contains(&w.as_str()));
        if !is_tag {
            out.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

fn sorted_tokens(norm: &str) -> Vec<&str> {
    let mut t: Vec<&str> = norm.split(' ').filter(|w| !w.is_empty()).collect();
    t.sort_unstable();
    t
}

/// Number of differing bits between two 64-bit image hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Are two normalized titles (and optionally cover hashes) the same series?
///
/// Deliberately conservative: wrongly merging two series is worse than
/// showing one series twice. Series that merely share a prefix ("Naruto" and
/// "Naruto Shippuden", "Solo Leveling" and "Solo Leveling: Ragnarok") stay
/// separate.
pub fn same_series(a: &str, b: &str, cover_a: Option<u64>, cover_b: Option<u64>) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b || sorted_tokens(a) == sorted_tokens(b) {
        return true;
    }
    let sim = jaro_winkler(a, b);
    let (la, lb) = (a.chars().count(), b.chars().count());
    // Near-identical spellings: a typo, a missing letter.
    if sim >= 0.965 && la.abs_diff(lb) <= 2 && la.min(lb) >= 8 {
        return true;
    }
    // Similar title and a matching cover.
    if let (Some(x), Some(y)) = (cover_a, cover_b) {
        if sim >= 0.90 && hamming(x, y) <= 8 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_punctuation_accents_and_articles() {
        assert_eq!(
            normalize_title("The Greatest Estate Developer"),
            "greatest estate developer"
        );
        assert_eq!(normalize_title("Sōlo-Leveling"), "solo leveling");
        assert_eq!(normalize_title("  ONE   PIECE!! "), "one piece");
        assert_eq!(
            normalize_title("Ｒｅ：Ｚｅｒｏ"),
            "re zero",
            "full-width letters fold to ASCII"
        );
        assert_eq!(normalize_title("The"), "the", "a lone 'the' is kept");
    }

    #[test]
    fn strips_edition_tags_but_keeps_meaningful_parentheses() {
        assert_eq!(normalize_title("One Piece [Official]"), "one piece");
        assert_eq!(normalize_title("Berserk (Colored)"), "berserk");
        assert_eq!(normalize_title("Berserk (Digital Colored)"), "berserk");
        assert_eq!(
            normalize_title("Re:Zero (Light Novel)"),
            "re zero light novel"
        );
        assert_eq!(normalize_title("Title (2020) [Raw]"), "title 2020");
        assert_eq!(normalize_title("Broken (paren"), "broken paren");
    }

    #[test]
    fn keeps_japanese_text_intact() {
        // Voicing marks must survive accent folding.
        assert_eq!(normalize_title("ガンダム"), "ガンダム");
        assert_eq!(normalize_title("進撃の巨人"), "進撃の巨人");
    }

    #[test]
    fn spelling_variants_of_one_series_match() {
        let n = normalize_title;
        for (a, b) in [
            ("One Piece", "ONE PIECE"),
            ("One Piece", "One Piece [Official]"),
            ("Solo Leveling", "Solo-Leveling"),
            ("Solo Leveling", "Sōlo Leveling"),
            ("Piece One", "One Piece"),
            ("Tower of God", "Tower Of God (Webtoon)"),
            ("Shingeki no Kyojin", "Shingeki no Kyoujin"),
        ] {
            assert!(same_series(&n(a), &n(b), None, None), "{a} / {b}");
        }
    }

    #[test]
    fn different_series_never_match() {
        let n = normalize_title;
        for (a, b) in [
            ("Naruto", "Naruto Shippuden"),
            ("Solo Leveling", "Solo Leveling: Ragnarok"),
            ("One Piece", "One Punch Man"),
            ("Dragon Ball", "Dragon Ball Super"),
            ("Bleach", "Bleach: Thousand Year Blood War"),
            ("Attack on Titan", "Attack on Titan: Before the Fall"),
            ("A", "B"),
        ] {
            assert!(!same_series(&n(a), &n(b), None, None), "{a} / {b}");
        }
        assert!(!same_series("", "", None, None));
    }

    #[test]
    fn a_matching_cover_allows_a_looser_title_match_only() {
        let (a, b) = ("kimetsu no yaiba", "kimetsu no yaiba demon slayer");
        assert!(!same_series(a, b, None, None));
        assert!(same_series(
            a,
            b,
            Some(0xFFFF_0000_FFFF_0000),
            Some(0xFFFF_0000_FFFF_0003)
        ));
        // A different cover keeps them apart.
        assert!(!same_series(a, b, Some(0), Some(u64::MAX)));
        // A matching cover does not rescue unrelated titles.
        assert!(!same_series("naruto", "bleach", Some(1), Some(1)));
    }

    #[test]
    fn hamming_counts_bits() {
        assert_eq!(hamming(0, 0), 0);
        assert_eq!(hamming(0b1011, 0b0001), 2);
        assert_eq!(hamming(0, u64::MAX), 64);
    }
}
