use std::time::Duration;

use crate::cli::FetchArgs;

/// Maximum response body size (50 MB) — prevents OOM on huge pages.
const MAX_BODY_SIZE: u64 = 50 * 1024 * 1024;

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

    let html = fetch_html(&args.url)?;

    let (markdown, meta) = if args.raw {
        (convert_raw(&html)?, None)
    } else {
        extract_readable(&html, &args.url)
    };

    let mut output = String::new();

    if args.metadata {
        if let Some(ref m) = meta {
            output.push_str(&format_front_matter(m, &args.url));
        } else {
            output.push_str(&format_front_matter(&ArticleMeta::default(), &args.url));
        }
    }

    output.push_str(&markdown);

    if let Some(ref path) = args.output {
        std::fs::write(path, &output)?;
        eprintln!("  Wrote {}", path);
    }

    Ok(output)
}

fn fetch_html(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let agent = http_agent();
    let resp = agent
        .get(url)
        .header("User-Agent", "mdx-cli (https://github.com/Harsh-2002/MDX)")
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

    if !content_type.contains("text/html") && !content_type.contains("application/xhtml") {
        return Err(format!(
            "URL returned non-HTML content ({}). Use 'mdx <url>' for markdown URLs.",
            content_type
        )
        .into());
    }

    let body = resp
        .into_body()
        .with_config()
        .limit(MAX_BODY_SIZE)
        .read_to_string()?;
    Ok(body)
}

#[derive(Default)]
struct ArticleMeta {
    title: Option<String>,
    byline: Option<String>,
    excerpt: Option<String>,
    published_time: Option<String>,
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

fn format_front_matter(meta: &ArticleMeta, url: &str) -> String {
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
    fm.push_str("---\n\n");
    fm
}
