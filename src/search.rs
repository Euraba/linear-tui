//! Text matching for the `/` find and `f` filter features.
//!
//! Substring matching with *smart-case*: case-insensitive unless the query
//! contains an uppercase letter (the same rule ripgrep uses). The
//! case-insensitive path folds with [`str::to_ascii_lowercase`], which
//! preserves byte length so the returned ranges stay valid for slicing the
//! original (un-folded) haystack when highlighting.

/// Byte ranges `(start, end)` of every non-overlapping match of `query` in
/// `haystack`. Empty when `query` is empty or there are no matches.
pub fn match_ranges(haystack: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    // Smart-case: any uppercase letter in the query → case-sensitive.
    let case_sensitive = query.chars().any(|c| c.is_ascii_uppercase());
    let (hay, needle) = if case_sensitive {
        (haystack.to_string(), query.to_string())
    } else {
        (haystack.to_ascii_lowercase(), query.to_ascii_lowercase())
    };

    let mut ranges = Vec::new();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(&needle) {
        let s = start + pos;
        let e = s + needle.len();
        ranges.push((s, e));
        start = e.max(s + 1);
    }
    ranges
}

/// Whether `query` occurs in `haystack` (smart-case substring).
pub fn is_match(haystack: &str, query: &str) -> bool {
    !match_ranges(haystack, query).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_case_insensitive_for_lowercase_query() {
        assert!(is_match("Fix Redis timeout", "redis"));
        assert!(is_match("REDIS", "redis"));
        assert!(is_match("redis", "redis"));
    }

    #[test]
    fn smart_case_sensitive_when_query_has_uppercase() {
        assert!(is_match("Fix Redis timeout", "Redis"));
        assert!(!is_match("fix redis timeout", "Redis"));
    }

    #[test]
    fn empty_query_never_matches() {
        assert!(!is_match("anything", ""));
        assert!(match_ranges("anything", "").is_empty());
    }

    #[test]
    fn ranges_are_valid_byte_offsets_into_original() {
        let hay = "Redis and redis again";
        let ranges = match_ranges(hay, "redis");
        // Two matches (case-insensitive), and each range slices back to a
        // case-insensitive equal of the needle.
        assert_eq!(ranges.len(), 2);
        for (s, e) in ranges {
            assert!(hay[s..e].eq_ignore_ascii_case("redis"));
        }
    }

    #[test]
    fn handles_unicode_haystack_without_panicking() {
        // Non-ASCII bytes must not cause a panic when slicing by the ranges.
        let hay = "café — redis ☕";
        let ranges = match_ranges(hay, "redis");
        assert_eq!(ranges.len(), 1);
        let (s, e) = ranges[0];
        assert_eq!(&hay[s..e], "redis");
    }
}
