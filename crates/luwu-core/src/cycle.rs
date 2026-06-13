//! Cycle management types for long-running agent sessions.
//!
//! A **cycle** is a window-bounded segment of a logical session.
//! When the token budget approaches its limit, a **rebuild** happens:
//! the current window is cleared and reconstructed from persisted memory.
//!
//! Between rebuilds, **checkpoints** are written at configurable thresholds
//! (default: 20%, 45%, 70%) by an independent Writer subagent.

/// What the main loop should do after checking token usage.
#[derive(Debug, Clone, PartialEq)]
pub enum CycleAction {
    /// Continue normally — no action needed.
    Continue,
    /// Token usage has crossed a checkpoint threshold — trigger Writer.
    Checkpoint,
    /// Token usage is near the budget limit — rebuild context.
    Rebuild,
}

/// Cycle management state. Tracks token usage and checkpoint triggers.
#[derive(Debug, Clone)]
pub struct CycleState {
    /// Current cycle index (0-based). Incremented on each rebuild.
    pub cycle_index: usize,
    /// Estimated tokens consumed in the current cycle.
    pub tokens_used: usize,
    /// Token budget per cycle. When exceeded, rebuild triggers.
    pub token_budget: usize,
    /// Checkpoint trigger thresholds (percentage, e.g. [20, 45, 70]).
    pub checkpoint_thresholds: Vec<u8>,
    /// Thresholds already triggered in the current cycle.
    pub triggered: Vec<u8>,
    /// Whether memory/cycle management is enabled.
    pub enabled: bool,
    /// Tool calls in the current cycle.
    pub tool_calls: usize,
    /// Tool call threshold for triggering a checkpoint.
    pub tool_call_threshold: usize,
    /// Whether the tool-call checkpoint has already fired this cycle.
    pub tool_checkpoint_done: bool,
}

impl Default for CycleState {
    fn default() -> Self {
        Self {
            cycle_index: 0,
            tokens_used: 0,
            token_budget: 100_000,
            checkpoint_thresholds: vec![20, 45, 70],
            triggered: Vec::new(),
            enabled: true,
            tool_calls: 0,
            tool_call_threshold: 15,
            tool_checkpoint_done: false,
        }
    }
}

impl CycleState {
    /// Create a new CycleState with the given token budget.
    pub fn new(token_budget: usize) -> Self {
        Self {
            token_budget,
            ..Default::default()
        }
    }

    /// Create a disabled CycleState (short tasks, no memory).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Add tokens consumed. Returns the action to take.
    pub fn add_tokens(&mut self, tokens: usize) -> CycleAction {
        if !self.enabled {
            return CycleAction::Continue;
        }
        self.tokens_used += tokens;
        self.check()
    }

    /// Check current token usage and return the appropriate action.
    pub fn check(&self) -> CycleAction {
        if !self.enabled || self.token_budget == 0 {
            return CycleAction::Continue;
        }

        let pct = (self.tokens_used * 100 / self.token_budget) as u8;

        // Rebuild at 90%.
        if pct >= 90 {
            return CycleAction::Rebuild;
        }

        // Check untriggered checkpoints.
        for threshold in &self.checkpoint_thresholds {
            if pct >= *threshold && !self.triggered.contains(threshold) {
                return CycleAction::Checkpoint;
            }
        }

        CycleAction::Continue
    }

    /// Mark a checkpoint threshold as triggered.
    pub fn mark_checkpoint(&mut self, threshold: u8) {
        if !self.triggered.contains(&threshold) {
            self.triggered.push(threshold);
        }
    }

    /// Record a tool call. Returns Checkpoint if threshold is crossed.
    pub fn add_tool_call(&mut self) -> CycleAction {
        if !self.enabled {
            return CycleAction::Continue;
        }
        self.tool_calls += 1;
        if self.tool_calls >= self.tool_call_threshold && !self.tool_checkpoint_done {
            return CycleAction::Checkpoint;
        }
        CycleAction::Continue
    }

    /// Mark the tool-call checkpoint as done (prevents re-trigger).
    pub fn mark_tool_call_checkpoint(&mut self) {
        self.tool_checkpoint_done = true;
    }

    /// Current tool call count.
    pub fn tool_usage(&self) -> usize {
        self.tool_calls
    }

    /// Reset for a new cycle (after rebuild).
    pub fn reset_cycle(&mut self) {
        self.cycle_index += 1;
        self.tokens_used = 0;
        self.triggered.clear();
        self.tool_calls = 0;
        self.tool_checkpoint_done = false;
    }

    /// Get current usage percentage.
    pub fn usage_pct(&self) -> u8 {
        if self.token_budget == 0 {
            return 0;
        }
        (self.tokens_used * 100 / self.token_budget) as u8
    }

    /// Is memory enabled?
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_progression() {
        let mut cs = CycleState::new(1000);
        assert_eq!(cs.usage_pct(), 0);

        // 15% → Continue
        assert_eq!(cs.add_tokens(150), CycleAction::Continue);

        // 25% → Checkpoint (threshold 20)
        assert_eq!(cs.add_tokens(100), CycleAction::Checkpoint);
        cs.mark_checkpoint(20);

        // 30% → Continue (20 already triggered)
        assert_eq!(cs.add_tokens(50), CycleAction::Continue);

        // 50% → Checkpoint (threshold 45)
        assert_eq!(cs.add_tokens(200), CycleAction::Checkpoint);
        cs.mark_checkpoint(45);

        // 75% → Checkpoint (threshold 70)
        assert_eq!(cs.add_tokens(250), CycleAction::Checkpoint);
        cs.mark_checkpoint(70);

        // 92% → Rebuild
        assert_eq!(cs.add_tokens(170), CycleAction::Rebuild);
    }

    #[test]
    fn cycle_reset() {
        let mut cs = CycleState::new(1000);
        cs.add_tokens(500);
        cs.mark_checkpoint(20);
        cs.mark_checkpoint(45);

        cs.reset_cycle();
        assert_eq!(cs.cycle_index, 1);
        assert_eq!(cs.tokens_used, 0);
        assert!(cs.triggered.is_empty());
    }

    #[test]
    fn disabled_cycle() {
        let mut cs = CycleState::disabled();
        assert_eq!(cs.add_tokens(999999), CycleAction::Continue);
    }

    #[test]
    fn cycle_tool_call() {
        let mut cs = CycleState::new(1000);
        // Below threshold → Continue
        for _ in 0..14 {
            assert_eq!(cs.add_tool_call(), CycleAction::Continue);
        }
        // 15th call → Checkpoint
        assert_eq!(cs.add_tool_call(), CycleAction::Checkpoint);
        cs.mark_tool_call_checkpoint();
        // Further calls don't re-trigger
        for _ in 0..10 {
            assert_eq!(cs.add_tool_call(), CycleAction::Continue);
        }
        assert_eq!(cs.tool_usage(), 25);
        // Reset clears tool state
        cs.reset_cycle();
        assert_eq!(cs.tool_usage(), 0);
        assert!(!cs.tool_checkpoint_done);
    }
}
