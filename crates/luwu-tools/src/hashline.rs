//! Shared hashline utilities for LINE:HASH anchored read/edit.
//!
//! Format: `{line_number}:{hash}|{content}`
//!
//! The hash is a 3-hex-char fingerprint of the line content,
//! computed via SipHash (Rust's DefaultHasher). This gives 12 bits
//! of entropy (4096 distinct values) — sufficient for detecting
//! edits in files under a few thousand lines.

use std::hash::{Hash, Hasher};

/// Number of hex characters in the line hash.
const HASH_LEN: usize = 3;

/// Compute a short hash fingerprint for a single line of text.
///
/// The hash covers the exact line content (no trailing newline).
/// Returns a lowercase hex string of length `HASH_LEN` (default 3 chars).
pub fn line_hash(line: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut hasher);
    let hash = hasher.finish();
    // Take the lowest 12 bits → 3 hex chars.
    format!("{:0width$x}", hash & 0xFFF, width = HASH_LEN)
}

/// Format a single line with LINE:HASH prefix.
///
/// Output: `{line_num}:{hash}|{content}`
pub fn format_line(line_num: usize, content: &str) -> String {
    let hash = line_hash(content);
    format!("{line_num}:{hash}|{content}")
}

/// Format multiple lines with LINE:HASH prefixes.
///
/// Takes an iterator of `(1-indexed line number, line content)` pairs.
pub fn format_lines<'a, I>(lines: I) -> String
where
    I: Iterator<Item = (usize, &'a str)>,
{
    lines
        .map(|(num, content)| format_line(num, content))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse an anchor string in the format `{line}:{hash}`.
///
/// Returns `(line_number, hash_string)` on success.
pub fn parse_anchor(anchor: &str) -> Option<(usize, &str)> {
    let colon_pos = anchor.find(':')?;
    let line_str = &anchor[..colon_pos];
    let hash_str = &anchor[colon_pos + 1..];
    let line_num = line_str.parse::<usize>().ok()?;
    if hash_str.is_empty() {
        return None;
    }
    Some((line_num, hash_str))
}

/// Verify that a given line in the content matches the expected hash.
///
/// Returns `Ok(line_content)` if the hash matches, or an error message
/// describing the mismatch.
pub fn verify_anchor<'a>(content: &'a str, line_num: usize, expected_hash: &str) -> Result<&'a str, String> {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    if line_num == 0 || line_num > total {
        return Err(format!(
            "Anchor line {line_num} is out of range (file has {total} lines)."
        ));
    }

    let actual_line = lines[line_num - 1];
    let actual_hash = line_hash(actual_line);

    if actual_hash == expected_hash {
        Ok(actual_line)
    } else {
        Err(format!(
            "Anchor mismatch at line {line_num}: expected hash `{expected_hash}`, \
             but current line has hash `{actual_hash}`.\n  \
             Expected: {line_num}:{expected_hash}|...\n  \
             Actual:   {line_num}:{actual_hash}|{actual_line}\n\n\
             The file has been modified since you last read it. \
             Use `read` to get the current content and updated anchors."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_hash_deterministic() {
        let line = "fn main() {";
        let h1 = line_hash(line);
        let h2 = line_hash(line);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), HASH_LEN);
    }

    #[test]
    fn test_line_hash_different_for_different_content() {
        let h1 = line_hash("fn main() {");
        let h2 = line_hash("fn other() {");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_format_line() {
        let result = format_line(42, "pub fn hello() {");
        assert!(result.starts_with("42:"));
        assert!(result.contains("|pub fn hello() {"));
    }

    #[test]
    fn test_parse_anchor() {
        assert_eq!(parse_anchor("45:4bf"), Some((45, "4bf")));
        assert_eq!(parse_anchor("1:abc"), Some((1, "abc")));
        assert_eq!(parse_anchor("100:fff"), Some((100, "fff")));
        assert_eq!(parse_anchor("0:abc"), Some((0, "abc"))); // 0 is valid parse, but invalid for verify
        assert_eq!(parse_anchor("abc"), None);
        assert_eq!(parse_anchor(":"), None);
        assert_eq!(parse_anchor("45:"), None);
        assert_eq!(parse_anchor("abc:def"), None);
    }

    #[test]
    fn test_verify_anchor_match() {
        let content = "line one\nline two\nline three";
        let hash = line_hash("line two");
        assert!(verify_anchor(content, 2, &hash).is_ok());
        assert_eq!(verify_anchor(content, 2, &hash).unwrap(), "line two");
    }

    #[test]
    fn test_verify_anchor_mismatch() {
        let content = "line one\nmodified line\nline three";
        let old_hash = line_hash("original line");
        let result = verify_anchor(content, 2, &old_hash);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Anchor mismatch"));
    }

    #[test]
    fn test_verify_anchor_out_of_range() {
        let content = "only one line";
        assert!(verify_anchor(content, 2, "abc").is_err());
        assert!(verify_anchor(content, 0, "abc").is_err());
    }
}
