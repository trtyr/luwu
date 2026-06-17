// core/textOverflow.ts — Pure function for progressive text overflow splitting.
//
// Used by useChatSession's commitTextOverflow() to decide where to split a
// growing text block so the early part can be committed to <Static>
// (terminal scrollback) while the recent part stays in the dynamic area.
// This keeps the dynamic render area bounded (~15 lines) so Ink's
// clearTerminal (ESC[2J) flicker path is never triggered.
//
// REGRESSION GUARD: This function was previously a nested closure that
// used `lines.length - MAX_DYNAMIC_LINES` as a char offset for text.slice(),
// which only worked if every line was 1 character long. For a 20-line
// text with 50 chars/line, splitAt=5 but text.slice(0,5) only took the
// first 5 CHARACTERS, leaving ~995 chars in the dynamic area and
// triggering the very flicker this code was supposed to prevent.
//
// This pure function returns a structured result with didSplit + earlyText
// + recentText so callers can apply the split without managing offsets
// themselves. Tests in ui/tests/textOverflow.test.ts lock down the
// correct behavior across:
//   - 20-line text with 50 chars/line (the regression case)
//   - single long line exceeding char budget
//   - CJK text where char count matters (not byte count)
//   - boundary conditions (exactly at max, empty text)
//   - varying line lengths

export interface OverflowSplit {
  didSplit: true;
  earlyText: string;
  recentText: string;
}

/**
 * Decide where to split a text block that has grown too large.
 *
 * Two overflow triggers, in priority order:
 *   (a) Line overflow: text has more than `maxLines` lines → split at
 *       the line boundary to keep the last `maxLines` lines in the
 *       recent part. CRITICAL: the split point must be a CHARACTER
 *       OFFSET for text.slice(), not a line index. We sum each
 *       skipped line's length + 1 (for the \n separator) to get the
 *       actual char position.
 *   (b) Char overflow: text has more than `maxChars` total chars but
 *       fewer than maxLines lines → split at the last \n before the
 *       char budget, falling back to a raw char slice if no \n exists
 *       (so we don't cut a single CJK character or word in half).
 *
 * Returns null when no split is needed (text fits within both budgets)
 * or when a split would produce an empty side.
 */
export function splitTextForOverflow(
  text: string,
  maxLines: number,
  maxChars: number,
): OverflowSplit | null {
  if (text.length === 0) return null;

  const lines = text.split('\n');

  let splitAt: number;
  if (lines.length > maxLines) {
    // ── Line-based split: convert "skip N lines" to char offset ──
    // For a 20-line text with 50 chars/line, we want to keep the last
    // 15 lines (skip 5). The naive approach would use splitAt = 5,
    // but text.slice(0, 5) only takes 5 characters, not 5 lines.
    //
    // Correct: sum each skipped line's length + 1 (for the \n
    // separator). Each line has its own length, so we cannot assume
    // uniform line widths.
    //
    // Example: "a\nbb\nccc\ndddd" split by \n = ["a","bb","ccc","dddd"]
    //   lines.length=4, keep last 2 lines → skip 2 lines
    //   charOffset = 1+1 + 2+1 = 5
    //   text.slice(0, 5) = "a\nbb\n"  ← correct early part
    //   text.slice(5) = "ccc\ndddd"  ← correct recent part
    const skipLines = lines.length - maxLines;
    let charOffset = 0;
    for (let i = 0; i < skipLines; i++) {
      charOffset += lines[i].length + 1; // +1 for the \n separator
    }
    splitAt = charOffset;
  } else if (text.length > maxChars) {
    // ── Char-based split: single long line, find last \n before budget ──
    // Avoid cutting in the middle of a word/character. Walk from the
    // start up to the char budget position, remember the last \n.
    // If no \n found in range, fall back to raw char slice (CJK
    // safety is handled by checking char boundaries elsewhere).
    const charBudget = text.length - maxChars;
    let lastNewline = -1;
    for (let i = 0; i < charBudget && i < text.length; i++) {
      if (text[i] === '\n') lastNewline = i;
    }
    splitAt = lastNewline >= 0 ? lastNewline + 1 : charBudget;
  } else {
    return null; // within both budgets, no split needed
  }

  // Guard against degenerate splits (zero-length side or split at end)
  if (splitAt <= 0 || splitAt >= text.length) return null;

  const earlyText = text.slice(0, splitAt);
  const recentText = text.slice(splitAt);

  // Guard against the split producing an empty side (would create a
  // ghost block in <Static> with no content)
  if (earlyText.length === 0 || recentText.length === 0) return null;

  return { didSplit: true, earlyText, recentText };
}
