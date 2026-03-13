use std::time::Duration;

use crate::cli::FetchArgs;

/// Maximum response body size (50 MB) — prevents OOM on huge pages.
const MAX_BODY_SIZE: u64 = 50 * 1024 * 1024;

enum FetchResult {
    /// Server provided markdown directly (MFA-enabled site).
    Markdown {
        body: String,
        server_tokens: Option<u64>,
        content_signal: Option<String>,
    },
    /// Server returned HTML — needs local conversion.
    Html {
        body: String,
        content_signal: Option<String>,
    },
}

fn http_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    config.into()
}

/// Fetches a URL, extracts content as markdown, and returns it.
/// File output (`-o`) is handled here; terminal rendering is handled by main.
pub fn run(args: &FetchArgs) -> Result<String, Box<dyn std::error::Error>> {
    if !args.url.starts_with("http://") && !args.url.starts_with("https://") {
        return Err("URL must start with http:// or https://".into());
    }

    let result = fetch_content(&args.url)?;

    // Check Content-Signal header
    let content_signal = match &result {
        FetchResult::Markdown { content_signal, .. } => content_signal.clone(),
        FetchResult::Html { content_signal, .. } => content_signal.clone(),
    };
    check_content_signal(content_signal.as_deref());

    let (markdown, meta, tokens) = match result {
        FetchResult::Markdown {
            body,
            server_tokens,
            ..
        } => {
            eprintln!("  Server provided markdown directly");
            let token_count = server_tokens.unwrap_or_else(|| crate::estimate_tokens(&body));
            let meta = extract_front_matter_meta(&body);
            (body, meta, token_count)
        }
        FetchResult::Html { body, .. } => {
            let (md, meta) = if args.raw {
                let sanitized = sanitize_html(&body);
                (convert_raw(&sanitized)?, None)
            } else {
                extract_readable(&body, &args.url)
            };
            let token_count = crate::estimate_tokens(&md);
            (md, meta, token_count)
        }
    };

    if args.tokens {
        eprintln!("  ~{} tokens", tokens);
    }

    let mut output = String::new();

    if args.metadata {
        let token_arg = if args.tokens { Some(tokens) } else { None };
        if let Some(ref m) = meta {
            output.push_str(&format_front_matter(m, &args.url, token_arg));
        } else {
            output.push_str(&format_front_matter(
                &ArticleMeta::default(),
                &args.url,
                token_arg,
            ));
        }
    }

    output.push_str(&markdown);

    if let Some(ref path) = args.output {
        std::fs::write(path, &output)?;
        eprintln!("  Wrote {}", path);
    }

    Ok(output)
}

fn fetch_content(url: &str) -> Result<FetchResult, Box<dyn std::error::Error>> {
    let agent = http_agent();
    let resp = agent
        .get(url)
        .header("User-Agent", "mdx-cli (https://github.com/Harsh-2002/mdx)")
        .header("Accept", "text/markdown, text/html;q=0.9")
        .call()?;

    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {} for {}", status, url).into());
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let server_tokens: Option<u64> = resp
        .headers()
        .get("x-markdown-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());

    let content_signal: Option<String> = resp
        .headers()
        .get("content-signal")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());

    if content_type.contains("text/markdown") {
        let body = resp
            .into_body()
            .with_config()
            .limit(MAX_BODY_SIZE)
            .read_to_string()?;
        return Ok(FetchResult::Markdown {
            body,
            server_tokens,
            content_signal,
        });
    }

    if !content_type.contains("text/html") && !content_type.contains("application/xhtml") {
        return Err(format!(
            "URL returned unsupported content type ({}). Expected text/markdown or text/html.",
            content_type
        )
        .into());
    }

    let body = resp
        .into_body()
        .with_config()
        .limit(MAX_BODY_SIZE)
        .read_to_string()?;
    Ok(FetchResult::Html {
        body,
        content_signal,
    })
}

/// Check Content-Signal header for ai-input=no directive.
fn check_content_signal(header: Option<&str>) {
    let Some(value) = header else { return };
    for part in value.split(',') {
        let part = part.trim();
        if let Some((key, val)) = part.split_once('=') {
            if key.trim().eq_ignore_ascii_case("ai-input") && val.trim().eq_ignore_ascii_case("no")
            {
                eprintln!("  Warning: site signals ai-input=no via Content-Signal header");
            }
        }
    }
}

/// Strip dangerous elements from HTML before raw conversion.
fn sanitize_html(html: &str) -> String {
    let mut result = html.to_string();

    // Remove dangerous elements (case-insensitive)
    for tag in &["script", "style", "noscript", "iframe", "object", "embed"] {
        // Remove both opening+content+closing and self-closing variants
        loop {
            let lower = result.to_lowercase();
            let open = format!("<{}", tag);
            if let Some(start) = lower.find(&open) {
                let close_tag = format!("</{}>", tag);
                if let Some(end_pos) = lower[start..].find(&close_tag) {
                    let end = start + end_pos + close_tag.len();
                    result.replace_range(start..end, "");
                } else {
                    // Self-closing or unclosed — remove to next >
                    if let Some(gt) = result[start..].find('>') {
                        result.replace_range(start..start + gt + 1, "");
                    } else {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    // Neutralize dangerous URL schemes in href and src attributes
    for attr in &["href", "src"] {
        loop {
            let lower = result.to_lowercase();
            let mut found = false;
            for scheme in &["javascript:", "data:", "vbscript:"] {
                // Look for attr="scheme..." or attr='scheme...'
                let pattern_dq = format!("{}=\"{}", attr, scheme);
                let pattern_sq = format!("{}='{}", attr, scheme);
                if let Some(pos) = lower.find(&pattern_dq) {
                    let value_start = pos + attr.len() + 2; // skip attr="
                    if let Some(end) = result[value_start..].find('"') {
                        let replacement = format!("{}=\"#\"", attr);
                        result.replace_range(pos..value_start + end + 1, &replacement);
                        found = true;
                        break;
                    }
                } else if let Some(pos) = lower.find(&pattern_sq) {
                    let value_start = pos + attr.len() + 2; // skip attr='
                    if let Some(end) = result[value_start..].find('\'') {
                        let replacement = format!("{}='#'", attr);
                        result.replace_range(pos..value_start + end + 1, &replacement);
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                break;
            }
        }
    }

    result
}

#[derive(Default)]
struct ArticleMeta {
    title: Option<String>,
    byline: Option<String>,
    excerpt: Option<String>,
    published_time: Option<String>,
    image: Option<String>,
    url: Option<String>,
    site_name: Option<String>,
}

fn extract_readable(html: &str, url: &str) -> (String, Option<ArticleMeta>) {
    let cfg = dom_smoothie::Config {
        text_mode: dom_smoothie::TextMode::Markdown,
        ..Default::default()
    };

    match dom_smoothie::Readability::new(html, Some(url), Some(cfg)) {
        Ok(mut reader) => match reader.parse() {
            Ok(article) => {
                let text = article.text_content.to_string();
                if text.trim().is_empty() {
                    eprintln!(
                        "  Warning: readability returned empty content, falling back to raw conversion"
                    );
                    let md = clean_markdown(&convert_raw(html).unwrap_or_default());
                    return (md, None);
                }
                let meta = ArticleMeta {
                    title: if article.title.is_empty() {
                        None
                    } else {
                        Some(article.title.clone())
                    },
                    byline: article.byline.clone(),
                    excerpt: article.excerpt.clone(),
                    published_time: article.published_time.clone(),
                    image: article.image.clone(),
                    url: article.url.clone(),
                    site_name: article.site_name.clone(),
                };
                let mut md = String::new();
                if !article.title.is_empty() {
                    md.push_str(&format!("# {}\n\n", article.title));
                }
                md.push_str(&text);
                (clean_markdown(&md), Some(meta))
            }
            Err(e) => {
                eprintln!(
                    "  Warning: readability extraction failed ({}), falling back to raw conversion",
                    e
                );
                let md = clean_markdown(&convert_raw(html).unwrap_or_default());
                (md, None)
            }
        },
        Err(e) => {
            eprintln!(
                "  Warning: readability init failed ({}), falling back to raw conversion",
                e
            );
            let md = clean_markdown(&convert_raw(html).unwrap_or_default());
            (md, None)
        }
    }
}

fn convert_raw(html: &str) -> Result<String, Box<dyn std::error::Error>> {
    let md = htmd::convert(html)?;
    Ok(clean_markdown(&md))
}

/// Remove unnecessary backslash escapes and collapse excessive blank lines.
/// dom_smoothie's markdown mode over-escapes characters like `.`, `(`, `)`,
/// which wastes LLM tokens and hurts readability.
fn clean_markdown(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut consecutive_blanks = 0u32;
    for line in input.lines() {
        if line.trim().is_empty() {
            consecutive_blanks += 1;
            if consecutive_blanks <= 2 {
                out.push('\n');
            }
            continue;
        }
        consecutive_blanks = 0;

        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(&next) = chars.peek() {
                    // Keep escapes that are meaningful in markdown
                    if matches!(
                        next,
                        '*' | '_' | '`' | '[' | ']' | '<' | '>' | '~' | '|' | '\\' | '#'
                    ) {
                        out.push(c);
                        out.push(next);
                        chars.next();
                    } else {
                        // Drop the backslash, keep the character
                        out.push(next);
                        chars.next();
                    }
                } else {
                    out.push(c);
                }
            } else {
                out.push(c);
            }
        }
        out.push('\n');
    }

    // Trim trailing whitespace
    let trimmed = out.trim_end();
    let mut result = trimmed.to_string();
    result.push('\n');
    result
}

/// Escape a string value for safe embedding in a YAML double-quoted string.
fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn format_front_matter(meta: &ArticleMeta, url: &str, tokens: Option<u64>) -> String {
    let mut fm = String::from("---\n");
    if let Some(ref title) = meta.title {
        fm.push_str(&format!("title: \"{}\"\n", yaml_escape(title)));
    }
    if let Some(ref byline) = meta.byline {
        fm.push_str(&format!("author: \"{}\"\n", yaml_escape(byline)));
    }
    if let Some(ref date) = meta.published_time {
        fm.push_str(&format!("date: \"{}\"\n", yaml_escape(date)));
    }
    fm.push_str(&format!("source: \"{}\"\n", yaml_escape(url)));
    if let Some(ref excerpt) = meta.excerpt {
        fm.push_str(&format!("excerpt: \"{}\"\n", yaml_escape(excerpt)));
    }
    if let Some(ref image) = meta.image {
        fm.push_str(&format!("image: \"{}\"\n", yaml_escape(image)));
    }
    if let Some(ref og_url) = meta.url {
        fm.push_str(&format!("url: \"{}\"\n", yaml_escape(og_url)));
    }
    if let Some(ref site_name) = meta.site_name {
        fm.push_str(&format!("site_name: \"{}\"\n", yaml_escape(site_name)));
    }
    if let Some(t) = tokens {
        fm.push_str(&format!("tokens: {}\n", t));
    }
    fm.push_str("---\n\n");
    fm
}

/// Parse YAML front matter from server-provided markdown into ArticleMeta.
fn extract_front_matter_meta(markdown: &str) -> Option<ArticleMeta> {
    let trimmed = markdown.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let after_open = &trimmed[3..];
    let after_open = after_open.trim_start_matches(['\r', '\n']);
    let close_pos = after_open.find("\n---")?;
    let front_matter = &after_open[..close_pos];

    let mut meta = ArticleMeta::default();
    for line in front_matter.lines() {
        let line = line.trim();
        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim();
            let val = val.trim().trim_matches('"');
            if val.is_empty() {
                continue;
            }
            match key {
                "title" => meta.title = Some(val.to_string()),
                "author" => meta.byline = Some(val.to_string()),
                "date" => meta.published_time = Some(val.to_string()),
                "excerpt" => meta.excerpt = Some(val.to_string()),
                "image" => meta.image = Some(val.to_string()),
                "url" => meta.url = Some(val.to_string()),
                "site_name" => meta.site_name = Some(val.to_string()),
                _ => {}
            }
        }
    }

    Some(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_strips_script() {
        let html = r#"<p>Hello</p><script>alert("xss")</script><p>World</p>"#;
        let result = sanitize_html(html);
        assert!(!result.to_lowercase().contains("<script"));
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
    }

    #[test]
    fn test_sanitize_strips_iframe() {
        let html = r#"<p>Text</p><iframe src="https://evil.com"></iframe><p>More</p>"#;
        let result = sanitize_html(html);
        assert!(!result.to_lowercase().contains("<iframe"));
        assert!(result.contains("Text"));
        assert!(result.contains("More"));
    }

    #[test]
    fn test_sanitize_strips_javascript_href() {
        let html = r#"<a href="javascript:alert(1)">Click</a>"#;
        let result = sanitize_html(html);
        assert!(!result.to_lowercase().contains("javascript:"));
        assert!(result.contains("Click"));
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(crate::estimate_tokens(""), 0);
        assert_eq!(crate::estimate_tokens("abcd"), 1);
        assert_eq!(crate::estimate_tokens("abcde"), 2); // 5/4 = 1.25, ceil = 2
        assert_eq!(crate::estimate_tokens("abcdefgh"), 2); // 8/4 = 2
    }

    #[test]
    fn test_extract_front_matter_meta() {
        let md = "---\ntitle: \"My Page\"\nauthor: \"Jane\"\nimage: \"https://example.com/img.png\"\nsite_name: \"Example\"\n---\n\n# Content";
        let meta = extract_front_matter_meta(md).unwrap();
        assert_eq!(meta.title.as_deref(), Some("My Page"));
        assert_eq!(meta.byline.as_deref(), Some("Jane"));
        assert_eq!(meta.image.as_deref(), Some("https://example.com/img.png"));
        assert_eq!(meta.site_name.as_deref(), Some("Example"));
    }

    #[test]
    fn test_extract_front_matter_meta_none() {
        let md = "# No front matter\n\nJust content.";
        assert!(extract_front_matter_meta(md).is_none());
    }

    #[test]
    fn test_clean_markdown() {
        let input = "# Hello\n\nSome text with escaped \\. period.\n";
        let result = clean_markdown(input);
        assert!(result.contains("# Hello"));
        assert!(result.contains("Some text with escaped . period."));
    }

    #[test]
    fn test_sanitize_strips_style() {
        let html = "<style>.evil { display: none; }</style><p>Safe</p>";
        let result = sanitize_html(html);
        assert!(!result.to_lowercase().contains("<style"));
        assert!(result.contains("Safe"));
    }

    #[test]
    fn test_sanitize_case_insensitive() {
        let html = r#"<SCRIPT>alert(1)</SCRIPT><p>OK</p>"#;
        let result = sanitize_html(html);
        assert!(!result.to_lowercase().contains("<script"));
        assert!(result.contains("OK"));
    }

    #[test]
    fn test_check_content_signal_no_warning() {
        // Should not panic or produce errors
        check_content_signal(None);
        check_content_signal(Some("ai-input=yes"));
        check_content_signal(Some("other=value"));
    }

    #[test]
    fn test_format_front_matter_with_tokens() {
        let meta = ArticleMeta {
            title: Some("Test".to_string()),
            ..Default::default()
        };
        let fm = format_front_matter(&meta, "https://example.com", Some(500));
        assert!(fm.contains("tokens: 500"));
        assert!(fm.contains("title: \"Test\""));
    }

    #[test]
    fn test_format_front_matter_without_tokens() {
        let meta = ArticleMeta {
            title: Some("Test".to_string()),
            ..Default::default()
        };
        let fm = format_front_matter(&meta, "https://example.com", None);
        assert!(!fm.contains("tokens:"));
    }

    #[test]
    fn test_format_front_matter_new_fields() {
        let meta = ArticleMeta {
            title: Some("Test".to_string()),
            image: Some("https://example.com/img.jpg".to_string()),
            url: Some("https://example.com/page".to_string()),
            site_name: Some("Example Site".to_string()),
            ..Default::default()
        };
        let fm = format_front_matter(&meta, "https://example.com", None);
        assert!(fm.contains("image: \"https://example.com/img.jpg\""));
        assert!(fm.contains("url: \"https://example.com/page\""));
        assert!(fm.contains("site_name: \"Example Site\""));
    }
}
