//! Web fetch tool — fetches web pages and extracts clean readable content.
//!
//! Uses kawat (trafilatura-inspired extraction) for main content + metadata,
//! with mdka as fallback for HTML → markdown conversion.

use async_trait::async_trait;
use kawat::{ExtractorOptions, bare_extraction};
use luwu_core::{LuwuError, Result, Tool, ToolContext, ToolOutput};
use serde_json::Value;
use std::time::Duration;
use tracing::info;

const DEFAULT_MAX_CHARS: usize = 50_000;
const DEFAULT_TIMEOUT_MS: u64 = 15_000;
const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024; // 5MB hard limit

pub struct WebFetchTool;

impl Default for WebFetchTool {
    fn default() -> Self {
        Self
    }
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetches a web page and extracts clean, readable content. \
         Returns the page's main text with metadata (title, author, site, language). \
         \
         Use this to read documentation, blog posts, articles, API references, \
         GitHub READMEs, or any web page where you need the actual content — \
         not the navigation, ads, or sidebar noise. \
         \
         Output formats: \
         - `markdown` (default): clean markdown with metadata header. Best for reading. \
         - `text`: plain text, all formatting stripped. \
         - `raw`: full HTML response without extraction. Use when you need the original HTML. \
         \
         Tips: \
         - For documentation pages, `markdown` format gives the best results. \
         - For API responses, use `raw` format to get unmodified content. \
         - Content is truncated at `max_chars` characters (default 50000). \
         - Timeout defaults to 15 seconds; increase for slow sites."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch (http or https only). \
                     Examples: \"https://docs.rs/tokio\", \"https://example.com/api/data\"."
                },
                "format": {
                    "type": "string",
                    "enum": ["markdown", "text", "raw"],
                    "description": "Output format. \
                     `markdown` (default): extracted content as clean markdown with metadata. \
                     `text`: plain text with all HTML stripped. \
                     `raw`: full original HTML without extraction."
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to return. Default: 50000. \
                     Content beyond this limit is truncated with a notice."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Request timeout in milliseconds. Default: 15000 (15 seconds). \
                     Increase for slow sites."
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> Result<ToolOutput> {
        let url = input["url"].as_str().ok_or_else(|| {
            LuwuError::Tool(
                "The 'url' parameter is required. \
                 Provide a full URL, e.g. \"https://docs.rs/tokio\"."
                    .into(),
            )
        })?;

        // Validate URL scheme.
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolOutput::error(
                "Only http:// and https:// URLs are supported. \
                 The URL must start with one of these schemes.",
            ));
        }

        let format = input["format"].as_str().unwrap_or("markdown");
        let max_chars = input["max_chars"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_CHARS as u64) as usize;
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(DEFAULT_TIMEOUT_MS);

        // Build the HTTP client.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .user_agent(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/131.0.0.0 Safari/537.36",
            )
            .build()
            .map_err(|e| LuwuError::Tool(format!("Failed to build HTTP client: {e}")))?;

        info!(url = %url, format = %format, "Fetching web page");

        let response = client.get(url).send().await.map_err(|e| {
            LuwuError::Tool(format!(
                "Request failed for `{url}`: {e}\n\
                 Check the URL is correct and the site is reachable."
            ))
        })?;

        let status = response.status();
        if !status.is_success() {
            return Ok(ToolOutput::error(format!(
                "HTTP {} for `{url}`\n\
                 The server returned an error. Check the URL or try again later.",
                status
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Check response size.
        let body = response.bytes().await.map_err(|e| {
            LuwuError::Tool(format!("Failed to read response body from `{url}`: {e}"))
        })?;

        if body.len() > MAX_RESPONSE_BYTES {
            return Ok(ToolOutput::error(format!(
                "Response too large ({} bytes, max {} bytes). \
                 The page is too big to process.",
                body.len(),
                MAX_RESPONSE_BYTES
            )));
        }

        let html = String::from_utf8_lossy(&body).into_owned();

        // Route based on format.
        let output = match format {
            "raw" => {
                // Return raw HTML as-is.
                let truncated = truncate_content(&html, max_chars);
                format!("URL: {url}\nStatus: {status}\nContent-Type: {content_type}\n\n{truncated}")
            }
            _ => {
                // Extract content using kawat (trafilatura-style).
                extract_content(&html, url, format, max_chars)
            }
        };

        Ok(ToolOutput::text(output))
    }
}

/// Extract readable content from HTML.
fn extract_content(html: &str, url: &str, format: &str, max_chars: usize) -> String {
    // Try kawat extraction with metadata.
    let options = ExtractorOptions {
        with_metadata: true,
        ..Default::default()
    };

    match bare_extraction(html, &options) {
        Ok(doc) => {
            // Build metadata header from Document.
            let title = doc
                .metadata
                .title
                .clone()
                .unwrap_or_else(|| extract_title(html));

            let mut meta_lines = vec![format!("Title: {title}")];
            if let Some(ref author) = doc.metadata.author {
                meta_lines.push(format!("Author: {author}"));
            }
            if let Some(ref site) = doc.metadata.sitename {
                meta_lines.push(format!("Site: {site}"));
            }
            if let Some(ref lang) = doc.metadata.language {
                meta_lines.push(format!("Language: {lang}"));
            }
            if let Some(ref date) = doc.metadata.date {
                meta_lines.push(format!("Date: {date}"));
            }
            meta_lines.push(format!("URL: {url}"));
            let metadata_block = meta_lines.join("\n");

            let text = doc.text.unwrap_or_default();

            // If kawat extracted nothing useful, fall back to mdka.
            let content = if text.trim().is_empty() {
                let md = mdka::html_to_markdown(html);
                match format {
                    "text" => truncate_content(&strip_html_tags(&md), max_chars),
                    _ => truncate_content(&md, max_chars),
                }
            } else {
                match format {
                    "text" => truncate_content(&strip_html_tags(&text), max_chars),
                    _ => {
                        if text.contains('<') && text.contains('>') {
                            truncate_content(&mdka::html_to_markdown(&text), max_chars)
                        } else {
                            truncate_content(&text, max_chars)
                        }
                    }
                }
            };

            format!("{metadata_block}\n\n---\n\n{content}")
        }
        Err(_) => {
            // Fallback: use mdka for HTML → markdown conversion.
            let title = extract_title(html);
            let markdown = mdka::html_to_markdown(html);
            let content = match format {
                "text" => truncate_content(&strip_html_tags(&markdown), max_chars),
                _ => truncate_content(&markdown, max_chars),
            };
            format!("Title: {title}\nURL: {url}\n\n---\n\n{content}")
        }
    }
}

/// Extract <title> from HTML.
fn extract_title(html: &str) -> String {
    let lower = html.to_lowercase();
    if let Some(start) = lower.find("<title>")
        && let Some(end) = lower.find("</title>")
    {
        let content = &html[start + 7..end];
        return content.trim().to_string();
    }
    "Untitled".to_string()
}

/// Strip HTML tags for plain text output.
fn strip_html_tags(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut in_entity = false;
    let mut entity = String::new();

    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '&' if !in_tag => {
                in_entity = true;
                entity.clear();
                entity.push(ch);
            }
            ';' if in_entity => {
                in_entity = false;
                entity.push(ch);
                // Decode common entities.
                match entity.as_str() {
                    "&amp;" => result.push('&'),
                    "&lt;" => result.push('<'),
                    "&gt;" => result.push('>'),
                    "&quot;" => result.push('"'),
                    "&#39;" | "&apos;" => result.push('\''),
                    "&nbsp;" => result.push(' '),
                    _ => result.push_str(&entity),
                }
            }
            _ if in_tag => {}
            _ if in_entity => entity.push(ch),
            _ => result.push(ch),
        }
    }

    // Collapse multiple blank lines.
    let mut cleaned = String::with_capacity(result.len());
    let mut last_was_newline = false;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !last_was_newline {
                cleaned.push('\n');
                last_was_newline = true;
            }
        } else {
            cleaned.push_str(trimmed);
            cleaned.push('\n');
            last_was_newline = false;
        }
    }

    cleaned.trim().to_string()
}

/// Truncate content to max_chars, adding a notice if truncated.
fn truncate_content(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }

    // Find a good break point (line boundary).
    let mut break_point = max_chars;
    for (i, c) in content.char_indices().take(max_chars) {
        if c == '\n' {
            break_point = i;
        }
    }

    let truncated: String = content.chars().take(break_point).collect();
    let total = content.len();
    format!(
        "{truncated}\n\n[... Content truncated at {max_chars} chars. Total length: {total} chars. \
         Increase max_chars to see more.]"
    )
}
