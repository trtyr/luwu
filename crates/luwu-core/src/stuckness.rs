//! Stuckness detection for the agent loop.
//!
//! Replaces the old `max_iterations` hard cap with a smarter safety valve
//! that watches for actual lack of progress. A complex long-running task
//! that keeps calling different tools with different arguments will never
//! trigger this, no matter how many iterations it takes.
//!
//! Two detection strategies:
//!
//! 1. **Repeat detection** — `repeat_threshold` consecutive identical
//!    `(tool_name, args_hash)` pairs means the LLM is stuck calling the
//!    same tool with the same arguments.
//!
//! 2. **Cycle detection** — when the *unordered* fingerprint of the
//!    most recent `cycle_window_size` calls repeats across two
//!    consecutive sliding windows, the LLM is cycling through a fixed
//!    pattern (e.g. read→write→read→write with no other progress).
//!
//! Neither strategy caps the iteration count. A legitimate long task
//! that explores many different tools in sequence runs unrestricted.
//!
//! # Example
//!
//! ```
//! use luwu_core::stuckness::StucknessGuard;
//! use serde_json::json;
//!
//! let mut guard = StucknessGuard::default();
//! // 3 identical calls → triggers repeat detection
//! for _ in 0..3 {
//!     let r = guard.record("read", &json!({"path": "/etc/hosts"}));
//!     if r.is_stuck() {
//!         println!("stuck: {:?}", r);
//!     }
//! }
//! ```

use std::collections::{VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use serde_json::Value;

/// Why the agent loop is considered stuck, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stuckness {
    /// No stuckness detected yet.
    NotStuck,
    /// `repeat_threshold` consecutive calls to `tool` with identical
    /// arguments. The agent is calling the same tool with the same
    /// arguments over and over.
    Repeated { tool: String, count: usize },
    /// Two consecutive sliding windows have identical *unordered*
    /// fingerprints, meaning the agent is cycling through a fixed
    /// pattern (e.g. A→B→A→B with no other progress).
    Cycling { tool: String, count: usize },
}

impl Stuckness {
    /// Returns `true` if this represents a stuck condition.
    pub fn is_stuck(&self) -> bool {
        !matches!(self, Stuckness::NotStuck)
    }
}

/// Sliding-window stuckness detector.
///
/// Cheap to construct and `record` is O(1) amortized. Designed to be
/// instantiated once per turn and updated on every tool call.
pub struct StucknessGuard {
    /// Recent `(tool_name, args_hash)` pairs.
    window: VecDeque<(String, u64)>,
    /// Repeat detection: number of consecutive identical calls to
    /// declare "stuck". Default 3.
    repeat_threshold: usize,
    /// Cycle detection: size of the sliding window for fingerprint
    /// comparison. Smaller values are more sensitive. Default 2.
    cycle_window_size: usize,
    /// Cycle detection: number of consecutive identical fingerprints
    /// to declare "cycling". Default 2.
    cycle_match_threshold: usize,
    /// Most recent cycle-window fingerprint.
    last_cycle_fingerprint: Option<u64>,
    /// How many consecutive identical cycle-fingerprints we've seen.
    cycle_match_count: usize,
}

impl Default for StucknessGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl StucknessGuard {
    /// Default thresholds:
    /// - `repeat_threshold = 3` (3 identical calls = stuck)
    /// - `cycle_window_size = 2` (2-call sliding window)
    /// - `cycle_match_threshold = 2` (2 identical fingerprints = cycling)
    ///
    /// These work well together: repeat catches the most common pattern
    /// (same tool same args) quickly, and cycle catches A→B→A→B patterns
    /// after the cycle has been established.
    pub fn new() -> Self {
        Self {
            window: VecDeque::with_capacity(8),
            repeat_threshold: 3,
            cycle_window_size: 2,
            cycle_match_threshold: 2,
            last_cycle_fingerprint: None,
            cycle_match_count: 0,
        }
    }

    /// Custom thresholds. Use this if the defaults don't fit your
    /// workload (e.g. very chatty tool calling might want a higher
    /// repeat threshold).
    pub fn with_thresholds(
        repeat_threshold: usize,
        cycle_window_size: usize,
        cycle_match_threshold: usize,
    ) -> Self {
        Self {
            window: VecDeque::with_capacity(repeat_threshold.max(cycle_window_size + 1)),
            repeat_threshold,
            cycle_window_size,
            cycle_match_threshold,
            last_cycle_fingerprint: None,
            cycle_match_count: 0,
        }
    }

    /// Record a tool call and check for stuckness.
    ///
    /// Returns [`Stuckness::NotStuck`] if the agent is making progress,
    /// or one of the stuck variants if it appears to be in a loop.
    pub fn record(&mut self, tool_name: &str, args: &Value) -> Stuckness {
        let args_hash = hash_args(args);
        self.window.push_back((tool_name.to_string(), args_hash));
        // Trim to repeat_threshold so the basic check has a bounded window.
        // The cycle fingerprint is computed on a separate sliding window of
        // cycle_window_size which is always the most recent N entries.
        if self.window.len() > self.repeat_threshold {
            self.window.pop_front();
        }

        // 1. Repeat detection: all entries in the window are identical.
        if self.window.len() >= self.repeat_threshold {
            let first = &self.window[0];
            if self.window.iter().all(|e| e == first) {
                return Stuckness::Repeated {
                    tool: first.0.clone(),
                    count: self.repeat_threshold,
                };
            }
        }

        // 2. Cycle detection: hash the most recent `cycle_window_size`
        //    entries (unordered) and compare to the previous fingerprint.
        if self.window.len() >= self.cycle_window_size {
            // If the most recent `cycle_window_size` calls are all
            // identical, this is a repeat pattern (not a cycle). The
            // basic repeat check above will catch it as soon as the
            // window fills to `repeat_threshold`. Skip the cycle check
            // so it doesn't fire prematurely.
            let n = self.cycle_window_size;
            let last_n_first = &self.window[self.window.len() - n];
            let all_identical =
                (0..n).all(|i| &self.window[self.window.len() - n + i] == last_n_first);
            if !all_identical {
                let new_fp = self.fingerprint_last_n(self.cycle_window_size);
                if let Some(prev_fp) = self.last_cycle_fingerprint {
                    if new_fp == prev_fp {
                        self.cycle_match_count += 1;
                        if self.cycle_match_count >= self.cycle_match_threshold {
                            let tool = self
                                .window
                                .back()
                                .map(|(t, _)| t.clone())
                                .unwrap_or_default();
                            return Stuckness::Cycling {
                                tool,
                                count: self.cycle_match_count,
                            };
                        }
                    } else {
                        self.cycle_match_count = 0;
                    }
                }
                self.last_cycle_fingerprint = Some(new_fp);
            }
        }

        Stuckness::NotStuck
    }

    /// Reset the guard (e.g. between turns or when the user takes
    /// manual action that should clear stuckness history).
    pub fn reset(&mut self) {
        self.window.clear();
        self.last_cycle_fingerprint = None;
        self.cycle_match_count = 0;
    }

    /// Current repeat threshold (for logging / debugging).
    pub fn repeat_threshold(&self) -> usize {
        self.repeat_threshold
    }

    /// Number of recent calls in the window.
    pub fn window_size(&self) -> usize {
        self.window.len()
    }

    /// Hash the most recent `n` window entries as an *unordered* set
    /// (sorted by `(tool, hash)`) so that A→B and B→A produce the same
    /// fingerprint. This is what makes cycle detection catch A→B→A→B.
    fn fingerprint_last_n(&self, n: usize) -> u64 {
        let n = n.min(self.window.len());
        let start = self.window.len() - n;
        let mut entries: Vec<&(String, u64)> = self.window.iter().skip(start).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut h = DefaultHasher::new();
        for entry in entries {
            entry.0.hash(&mut h);
            entry.1.hash(&mut h);
        }
        h.finish()
    }
}

/// Hash a JSON value for window comparison. Uses `Value::to_string()`
/// for canonicalization — fast, deterministic, and good enough for
/// detecting "same arguments". Cryptographic-strength collision
/// resistance is not needed here.
fn hash_args(args: &Value) -> u64 {
    let mut h = DefaultHasher::new();
    args.to_string().hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn not_stuck_on_diverse_calls() {
        let mut g = StucknessGuard::new();
        // Different tools, different args → never stuck
        assert_eq!(
            g.record("read", &json!({"path": "/a"})),
            Stuckness::NotStuck
        );
        assert_eq!(
            g.record("write", &json!({"path": "/b", "content": "x"})),
            Stuckness::NotStuck
        );
        assert_eq!(g.record("bash", &json!({"cmd": "ls"})), Stuckness::NotStuck);
        assert_eq!(
            g.record("grep", &json!({"pattern": "foo"})),
            Stuckness::NotStuck
        );
    }

    #[test]
    fn not_stuck_on_same_tool_different_args() {
        let mut g = StucknessGuard::new();
        for i in 0..10 {
            let r = g.record("read", &json!({"path": format!("/file/{i}")}));
            assert_eq!(r, Stuckness::NotStuck, "iteration {i}");
        }
    }

    #[test]
    fn repeat_detected_on_identical_calls() {
        let mut g = StucknessGuard::new();
        assert_eq!(
            g.record("read", &json!({"path": "/a"})),
            Stuckness::NotStuck
        );
        assert_eq!(
            g.record("read", &json!({"path": "/a"})),
            Stuckness::NotStuck
        );
        // 3rd identical call → stuck
        let r = g.record("read", &json!({"path": "/a"}));
        assert_eq!(
            r,
            Stuckness::Repeated {
                tool: "read".to_string(),
                count: 3,
            }
        );
    }

    #[test]
    fn cycle_detected_on_ab_pattern() {
        let mut g = StucknessGuard::new();
        // A→B→A→B with default cycle_window_size=2, cycle_match_threshold=2
        assert_eq!(
            g.record("read", &json!({"path": "/a"})),
            Stuckness::NotStuck
        );
        assert_eq!(
            g.record("write", &json!({"path": "/a", "content": "x"})),
            Stuckness::NotStuck
        );
        // After 2 calls, fingerprint {A} then {A,B}. No match yet.
        assert_eq!(
            g.record("read", &json!({"path": "/a"})),
            Stuckness::NotStuck
        );
        // After 3rd call, fingerprint of last 2 = {read, write} (unordered).
        // Previous fingerprint was also {read, write} → match. cycle_count = 1.
        // After 4th call, fingerprint still {read, write}. cycle_count = 2 → stuck!
        let r = g.record("write", &json!({"path": "/a", "content": "x"}));
        assert!(matches!(r, Stuckness::Cycling { .. }), "got {r:?}");
    }

    #[test]
    fn cycle_not_triggered_by_real_progress() {
        let mut g = StucknessGuard::new();
        // Read 3 different files, then write to one of them — not a cycle
        g.record("read", &json!({"path": "/a"}));
        g.record("read", &json!({"path": "/b"}));
        g.record("read", &json!({"path": "/c"}));
        let r = g.record("write", &json!({"path": "/a", "content": "x"}));
        assert_eq!(r, Stuckness::NotStuck);
    }

    #[test]
    fn reset_clears_history() {
        let mut g = StucknessGuard::new();
        for _ in 0..3 {
            g.record("read", &json!({"path": "/a"}));
        }
        // We should be stuck after 3 identical calls.
        g.reset();
        // After reset, a single different call should not be stuck.
        assert_eq!(
            g.record("write", &json!({"path": "/b"})),
            Stuckness::NotStuck
        );
    }

    #[test]
    fn custom_thresholds() {
        // Higher repeat threshold — 5 calls before stuck.
        let mut g = StucknessGuard::with_thresholds(5, 2, 2);
        for _ in 0..4 {
            assert_eq!(
                g.record("read", &json!({"path": "/a"})),
                Stuckness::NotStuck
            );
        }
        let r = g.record("read", &json!({"path": "/a"}));
        assert!(matches!(r, Stuckness::Repeated { count: 5, .. }));
    }
}
