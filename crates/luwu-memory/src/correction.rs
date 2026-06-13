//! Correction Detection — detect user corrections and save them immediately.
//!
//! Inspired by pi-hermes-memory's correction detection system.
//! When a user corrects the agent ("no, use X instead" / "不对" / "错了"),
//! that's the most valuable memory — it marks a specific mistake.
//! We detect it immediately rather than waiting for the next checkpoint cycle.

/// Type of correction pattern matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectionPattern {
    /// Explicit correction — must trigger save.
    Strong,
    /// Possible correction — save but mark as low-confidence.
    Weak,
}

/// Result of a correction detection check.
#[derive(Debug, Clone)]
pub struct CorrectionResult {
    /// Which pattern type was matched.
    pub pattern_type: CorrectionPattern,
    /// The specific phrase that triggered the match.
    pub matched_text: String,
    /// The full user message for context.
    pub full_message: String,
}

/// Detects user corrections in messages.
///
/// Uses three tiers of pattern matching:
/// - **Negative** patterns cancel detection ("no worries", "没关系")
/// - **Strong** patterns are explicit corrections ("no,", "不对", "错了")
/// - **Weak** patterns are possible corrections ("wait,", "等等", "应该是")
pub struct CorrectionDetector {
    /// Current turn number (incremented by caller).
    current_turn: usize,
    /// Turn number of the last correction that triggered a save.
    last_save_turn: Option<usize>,
    /// Minimum turns between correction saves (rate limiter).
    min_turn_gap: usize,
}

impl Default for CorrectionDetector {
    fn default() -> Self {
        Self {
            current_turn: 0,
            last_save_turn: None,
            min_turn_gap: 3,
        }
    }
}

impl CorrectionDetector {
    /// Create a new detector with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance the internal turn counter. Call once per user message.
    pub fn advance_turn(&mut self) {
        self.current_turn += 1;
    }

    /// Check if a user message is a correction.
    ///
    /// Returns `Some(CorrectionResult)` if a correction is detected
    /// and the rate limiter allows it. Returns `None` otherwise.
    pub fn detect(&mut self, message: &str) -> Option<CorrectionResult> {
        let lower = message.to_lowercase();

        // 1. Check negative patterns first — these are NOT corrections.
        if Self::matches_negative(&lower) {
            return None;
        }

        // 2. Check rate limiter.
        if let Some(last) = self.last_save_turn {
            if self.current_turn - last < self.min_turn_gap {
                return None;
            }
        }

        // 3. Check strong patterns.
        if let Some(matched) = Self::matches_strong(&lower) {
            self.last_save_turn = Some(self.current_turn);
            return Some(CorrectionResult {
                pattern_type: CorrectionPattern::Strong,
                matched_text: matched.into(),
                full_message: message.into(),
            });
        }

        // 4. Check weak patterns.
        if let Some(matched) = Self::matches_weak(&lower) {
            self.last_save_turn = Some(self.current_turn);
            return Some(CorrectionResult {
                pattern_type: CorrectionPattern::Weak,
                matched_text: matched.into(),
                full_message: message.into(),
            });
        }

        None
    }

    /// Check if message matches negative (non-correction) patterns.
    fn matches_negative(lower: &str) -> bool {
        const NEGATIVE: &[&str] = &[
            "no worries",
            "no problem",
            "no way",
            "actually looks great",
            "actually fine",
            "actually good",
            "actually perfect",
            "没关系",
            "没事",
            "不用了",
            "不需要",
            "挺好的",
            "没问题",
        ];
        NEGATIVE.iter().any(|p| lower.contains(p))
    }

    /// Check if message matches strong correction patterns.
    fn matches_strong(lower: &str) -> Option<&'static str> {
        const STRONG: &[&str] = &[
            "no,",
            "wrong",
            "actually,",
            "don't",
            "do not",
            "not like that",
            "i said",
            "stop doing",
            "that's wrong",
            "you're wrong",
            "incorrect",
            "should be",
            "supposed to be",
            "不对",
            "错了",
            "不是这样",
            "别这样",
            "不要",
            "不是",
            "搞错了",
            "反了",
            "我说的是",
            "不要这样",
            "不能用",
            "别用",
        ];
        STRONG
            .iter()
            .find(|p| lower.contains(*p))
            .copied()
    }

    /// Check if message matches weak correction patterns.
    fn matches_weak(lower: &str) -> Option<&'static str> {
        const WEAK: &[&str] = &[
            "wait,",
            "hmm,",
            "instead",
            "actually",
            "perhaps",
            "等等",
            "应该是",
            "其实",
            "不对吧",
            "好像不对",
            "改一下",
            "换成",
        ];
        WEAK
            .iter()
            .find(|p| lower.contains(*p))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_strong_english() {
        let mut d = CorrectionDetector::new();
        d.advance_turn();
        let r = d.detect("No, use pnpm instead of npm").unwrap();
        assert_eq!(r.pattern_type, CorrectionPattern::Strong);
        assert_eq!(r.matched_text, "no,");
    }

    #[test]
    fn detect_strong_chinese() {
        let mut d = CorrectionDetector::new();
        d.advance_turn();
        let r = d.detect("不对，这里应该用 async").unwrap();
        assert_eq!(r.pattern_type, CorrectionPattern::Strong);
        assert_eq!(r.matched_text, "不对");
    }

    #[test]
    fn detect_negative_skipped() {
        let mut d = CorrectionDetector::new();
        d.advance_turn();
        assert!(d.detect("no worries, it's fine").is_none());
        assert!(d.detect("没关系，下次注意就好").is_none());
    }

    #[test]
    fn detect_rate_limit() {
        let mut d = CorrectionDetector::new();
        d.advance_turn();
        assert!(d.detect("no, that's wrong").is_some()); // turn 1 — triggers

        d.advance_turn(); // turn 2
        assert!(d.detect("actually, fix the test").is_none()); // only 1 turn gap — blocked

        d.advance_turn(); // turn 3
        d.advance_turn(); // turn 4 — 3 turns gap from turn 1
        assert!(d.detect("wrong approach").is_some()); // now allowed
    }

    #[test]
    fn detect_weak_pattern() {
        let mut d = CorrectionDetector::new();
        d.advance_turn();
        let r = d.detect("wait, I think we should use a different approach").unwrap();
        assert_eq!(r.pattern_type, CorrectionPattern::Weak);
    }
}
