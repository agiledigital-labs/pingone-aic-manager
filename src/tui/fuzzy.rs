//! Small wrapper around `nucleo_matcher` for the app's synchronous list
//! filters. Feature modules still own row construction and sorting.

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

pub struct FuzzyMatcher {
    matcher: Matcher,
    pattern: Pattern,
    buf: Vec<char>,
    positions: Vec<u32>,
}

impl FuzzyMatcher {
    pub fn new(query: &str) -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            pattern: Pattern::new(
                query,
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
            ),
            buf: Vec::new(),
            positions: Vec::new(),
        }
    }

    pub fn match_indices(&mut self, haystack: &str) -> Option<(u32, Vec<u32>)> {
        self.positions.clear();
        let haystack = Utf32Str::new(haystack, &mut self.buf);
        let score = self
            .pattern
            .indices(haystack, &mut self.matcher, &mut self.positions)?;
        Some((score, self.positions.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_case_insensitive_positions() {
        let mut matcher = FuzzyMatcher::new("svc");

        let (score, positions) = matcher.match_indices("ServiceClient").unwrap();

        assert!(score > 0);
        assert_eq!(positions, [0, 3, 7]);
    }
}
