use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use comrak::nodes::NodeValue;
use rayon::prelude::*;

pub struct SearchArgs {
    pub query: String,
    pub paths: Vec<String>,
    pub limit: usize,
    pub tag: Option<String>,
    pub files_only: bool,
}

struct Document {
    path: PathBuf,
    title: Option<String>,
    headings: Vec<(u8, String, usize)>, // (level, text, line_number)
    body_text: String,
    code_text: String,
}

// Directories to skip during recursive search
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    ".build",
    "__pycache__",
    ".venv",
    "vendor",
    "dist",
    "build",
];

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "md" || ext == "markdown")
}

// -- File discovery (Phase 3: walkdir + inode dedup) --

#[cfg(unix)]
type FileId = (u64, u64);

#[cfg(not(unix))]
type FileId = PathBuf;

fn file_id(path: &Path) -> Option<FileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        path.canonicalize().ok()
    }
}

fn collect_files(paths: &[String]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();

    for p in paths {
        let path = Path::new(p);
        if path.is_file() {
            if !is_markdown_file(path) {
                eprintln!("Warning: '{}' is not a markdown file, skipping", p);
                continue;
            }
            if let Some(id) = file_id(path)
                && seen.insert(id)
            {
                files.push(path.to_path_buf());
            }
        } else if path.is_dir() {
            for entry in walkdir::WalkDir::new(path)
                .follow_links(true)
                .into_iter()
                .filter_entry(|e| {
                    if e.file_type().is_dir() && e.depth() > 0 {
                        let name = e.file_name().to_str().unwrap_or("");
                        !name.starts_with('.') && !SKIP_DIRS.contains(&name)
                    } else {
                        true
                    }
                })
            {
                match entry {
                    Ok(e) if e.file_type().is_file() && is_markdown_file(e.path()) => {
                        if let Some(id) = file_id(e.path())
                            && seen.insert(id)
                        {
                            files.push(e.into_path());
                        }
                    }
                    Err(e) => eprintln!("Warning: {}", e),
                    _ => {}
                }
            }
        } else {
            eprintln!("Warning: '{}' not found, skipping", p);
        }
    }

    files.sort();
    files
}

// -- Document parsing (Phase 4: lighter comrak options for search) --

fn parse_markdown_for_search<'a>(
    arena: &'a typed_arena::Arena<comrak::nodes::AstNode<'a>>,
    input: &str,
) -> &'a comrak::nodes::AstNode<'a> {
    let mut options = comrak::Options::default();
    options.extension.front_matter_delimiter = Some("---".to_owned());
    options.extension.table = true;
    options.extension.strikethrough = true;
    // Everything else OFF: autolink, tasklist, footnotes, alerts, math_dollars, math_code
    comrak::parse_document(arena, input, &options)
}

fn parse_document(path: &Path) -> Result<Document, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let fm = crate::frontmatter::parse(&content);
    let body_content = crate::frontmatter::strip(&content);

    let arena = typed_arena::Arena::new();
    let root = parse_markdown_for_search(&arena, body_content);

    let mut headings = Vec::new();
    let mut body_text = String::new();
    let mut code_text = String::new();

    // Include title in body text so it's searchable
    if let Some(ref title) = fm.title {
        body_text.push_str(title);
        body_text.push(' ');
    }

    for node in root.descendants() {
        let data = node.data.borrow();
        match &data.value {
            NodeValue::Heading(h) => {
                let text = extract_node_text(node);
                if !text.is_empty() {
                    headings.push((h.level, text, data.sourcepos.start.line));
                }
            }
            NodeValue::Text(t) => {
                body_text.push_str(t);
                body_text.push(' ');
            }
            NodeValue::Code(c) => {
                body_text.push_str(&c.literal);
                body_text.push(' ');
            }
            NodeValue::CodeBlock(cb) => {
                code_text.push_str(&cb.literal);
                code_text.push(' ');
            }
            _ => {}
        }
    }

    Ok(Document {
        path: path.to_path_buf(),
        title: fm.title,
        headings,
        body_text,
        code_text,
    })
}

fn extract_node_text<'a>(node: &'a comrak::nodes::AstNode<'a>) -> String {
    let mut text = String::new();
    for child in node.descendants() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::Text(t) => text.push_str(t),
            NodeValue::Code(c) => {
                text.push('`');
                text.push_str(&c.literal);
                text.push('`');
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => text.push(' '),
            _ => {}
        }
    }
    text
}

// -- BM25 Engine (Phase 6: pre-computed doc_freqs) --

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Deduplicate tokens while preserving order
fn dedup_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    tokens
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

const NUM_FIELDS: usize = 3;

struct FieldStats {
    term_freqs: HashMap<String, u32>,
    total_tokens: u32,
}

struct DocEntry {
    fields: [FieldStats; NUM_FIELDS],
}

struct SearchIndex {
    entries: Vec<DocEntry>,
    avg_field_lens: [f64; NUM_FIELDS],
    num_docs: usize,
    doc_freqs: HashMap<String, [u32; NUM_FIELDS]>,
}

impl SearchIndex {
    fn build(docs: &[Document]) -> Self {
        let num_docs = docs.len();
        let mut entries = Vec::with_capacity(num_docs);
        let mut total_field_lens = [0u64; NUM_FIELDS];
        let mut doc_freqs: HashMap<String, [u32; NUM_FIELDS]> = HashMap::new();

        for doc in docs {
            let heading_text = doc
                .headings
                .iter()
                .map(|(_, t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let fields = [
                Self::build_field_stats(&heading_text),
                Self::build_field_stats(&doc.body_text),
                Self::build_field_stats(&doc.code_text),
            ];

            for (i, f) in fields.iter().enumerate() {
                total_field_lens[i] += f.total_tokens as u64;
                for term in f.term_freqs.keys() {
                    doc_freqs.entry(term.clone()).or_insert([0; NUM_FIELDS])[i] += 1;
                }
            }

            entries.push(DocEntry { fields });
        }

        let avg_field_lens = [
            if num_docs > 0 {
                total_field_lens[0] as f64 / num_docs as f64
            } else {
                1.0
            },
            if num_docs > 0 {
                total_field_lens[1] as f64 / num_docs as f64
            } else {
                1.0
            },
            if num_docs > 0 {
                total_field_lens[2] as f64 / num_docs as f64
            } else {
                1.0
            },
        ];

        SearchIndex {
            entries,
            avg_field_lens,
            num_docs,
            doc_freqs,
        }
    }

    fn build_field_stats(text: &str) -> FieldStats {
        let tokens = tokenize(text);
        let total_tokens = tokens.len() as u32;
        let mut term_freqs = HashMap::with_capacity(tokens.len() * 2 / 3);
        for token in tokens {
            *term_freqs.entry(token).or_insert(0u32) += 1;
        }
        FieldStats {
            term_freqs,
            total_tokens,
        }
    }

    fn search(&self, query_tokens: &[String]) -> Vec<(usize, f64)> {
        let k1: f64 = 1.2;
        let b: f64 = 0.75;
        let field_boosts: [f64; NUM_FIELDS] = [3.0, 1.0, 0.5];

        let n = self.num_docs as f64;
        let mut scores: Vec<(usize, f64)> = Vec::new();

        for (doc_id, entry) in self.entries.iter().enumerate() {
            let mut total_score = 0.0;

            for qt in query_tokens {
                // Phase 6: O(1) lookup instead of O(D) scan
                let df = self.doc_freqs.get(qt).copied().unwrap_or([0; NUM_FIELDS]);

                for fi in 0..NUM_FIELDS {
                    let tf = entry.fields[fi].term_freqs.get(qt).copied().unwrap_or(0) as f64;

                    if tf == 0.0 {
                        continue;
                    }

                    let df_val = df[fi] as f64;
                    // IDF with floor for single-doc edge case
                    let idf = ((n - df_val + 0.5) / (df_val + 0.5) + 1.0).ln().max(0.1);

                    let field_len = entry.fields[fi].total_tokens as f64;
                    let avg_len = self.avg_field_lens[fi];
                    let norm_len = if avg_len > 0.0 {
                        field_len / avg_len
                    } else {
                        1.0
                    };

                    let tf_score = (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * norm_len));
                    total_score += field_boosts[fi] * idf * tf_score;
                }
            }

            if total_score > 0.0 {
                scores.push((doc_id, total_score));
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores
    }
}

// -- Snippet generation (Phase 2: lazy, re-reads file for displayed results only) --

/// Truncate a string to at most `max_chars` characters (not bytes), at a word boundary.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    // Find the char boundary at max_chars
    let byte_end = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let truncated = &s[..byte_end];
    if let Some(last_space) = truncated.rfind(' ') {
        format!("{}...", &truncated[..last_space])
    } else {
        format!("{}...", truncated)
    }
}

fn generate_snippet(
    raw_content: &str,
    headings: &[(u8, String, usize)],
    query_tokens: &[String],
) -> Option<(String, Option<String>)> {
    let lines: Vec<&str> = raw_content.lines().collect();
    if lines.is_empty() {
        return None;
    }

    // Skip front matter lines for scoring
    let content_start = find_content_start(raw_content);

    // Score each line by query token matches
    let mut best_line = content_start;
    let mut best_score = 0usize;

    for (i, line) in lines.iter().enumerate() {
        if i < content_start {
            continue;
        }
        let lower = line.to_lowercase();
        let score: usize = query_tokens
            .iter()
            .filter(|qt| lower.contains(qt.as_str()))
            .count();
        if score > best_score {
            best_score = score;
            best_line = i;
        }
    }

    if best_score == 0 {
        return None;
    }

    // Gather +/-1 context lines
    let start = if best_line > content_start {
        best_line - 1
    } else {
        content_start
    };
    let end = (best_line + 2).min(lines.len());

    let snippet: String = lines[start..end]
        .iter()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ");

    let snippet = truncate_chars(&snippet, 200);

    // Find which heading this line falls under
    let context_heading = headings
        .iter()
        .rfind(|(_, _, ln)| *ln <= best_line + 1) // sourcepos is 1-indexed
        .map(|(level, text, _)| format!("{} {}", "#".repeat(*level as usize), text));

    Some((snippet, context_heading))
}

fn find_content_start(content: &str) -> usize {
    if !content.starts_with("---") {
        return 0;
    }
    let rest = &content[3..];
    if let Some(end) = rest.find("\n---") {
        // Count lines up to and including the closing ---
        let front_matter_section = &content[..3 + end + 4];
        front_matter_section.lines().count()
    } else {
        0
    }
}

// -- Output formatting --

fn print_results(
    results: &[(usize, f64)],
    docs: &[Document],
    query_tokens: &[String],
    args: &SearchArgs,
) {
    let use_color = std::io::stdout().is_terminal();

    if args.files_only {
        for (doc_id, _) in results {
            println!("{}", docs[*doc_id].path.display());
        }
        return;
    }

    let (bold_cyan, yellow, bold_red, reset) = if use_color {
        ("\x1b[1;36m", "\x1b[33m", "\x1b[1;31m", "\x1b[0m")
    } else {
        ("", "", "", "")
    };

    for (i, (doc_id, _score)) in results.iter().enumerate() {
        let doc = &docs[*doc_id];

        if i > 0 {
            println!();
        }

        // File path (with title if available)
        if let Some(ref title) = doc.title {
            println!("{}{}{} ({})", bold_cyan, doc.path.display(), reset, title);
        } else {
            println!("{}{}{}", bold_cyan, doc.path.display(), reset);
        }

        // Phase 2: Re-read file only for displayed results (will be in OS page cache)
        if let Ok(raw_content) = std::fs::read_to_string(&doc.path)
            && let Some((snippet, heading)) =
                generate_snippet(&raw_content, &doc.headings, query_tokens)
        {
            if let Some(ref h) = heading {
                println!("  {}{}{}", yellow, h, reset);
            }
            // Highlight query tokens in snippet
            let highlighted = highlight_terms(&snippet, query_tokens, bold_red, reset);
            println!("  {}", highlighted);
        }
    }
}

fn highlight_terms(text: &str, query_tokens: &[String], bold_red: &str, reset: &str) -> String {
    if bold_red.is_empty() {
        return text.to_string();
    }

    let lower = text.to_lowercase();
    let mut result = String::with_capacity(text.len() + 64);

    // Walk both original and lowercased strings by char to keep indices in sync
    let orig_chars: Vec<char> = text.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i < orig_chars.len() {
        let mut matched = false;
        for qt in query_tokens {
            let qt_chars: Vec<char> = qt.chars().collect();
            if i + qt_chars.len() <= lower_chars.len()
                && lower_chars[i..i + qt_chars.len()] == qt_chars[..]
            {
                result.push_str(bold_red);
                for &ch in &orig_chars[i..i + qt_chars.len()] {
                    result.push(ch);
                }
                result.push_str(reset);
                i += qt_chars.len();
                matched = true;
                break;
            }
        }
        if !matched {
            result.push(orig_chars[i]);
            i += 1;
        }
    }

    result
}

// -- Public entry point --

pub fn run(args: &SearchArgs) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = std::time::Instant::now();

    // 1. Default paths
    let paths = if args.paths.is_empty() {
        vec![".".to_string()]
    } else {
        args.paths.clone()
    };

    // 2. Collect files
    let files = collect_files(&paths);
    if files.is_empty() {
        return Err("No markdown files found".into());
    }

    // 3. Phase 1: Early tag filtering (before parsing)
    let files = if let Some(ref tag) = args.tag {
        let tag_lower = tag.to_lowercase();
        let filtered: Vec<PathBuf> = files
            .into_iter()
            .filter(|f| {
                crate::frontmatter::read_front_matter_only(f)
                    .map(|fm| fm.tags.iter().any(|t| t.to_lowercase() == tag_lower))
                    .unwrap_or(false)
            })
            .collect();
        if filtered.is_empty() {
            return Err(format!("No files matching tag '{}'", tag).into());
        }
        filtered
    } else {
        files
    };

    // 4. Phase 5: Parallel parsing with rayon
    let docs: Vec<Document> = files
        .par_iter()
        .filter_map(|f| match parse_document(f) {
            Ok(doc) => Some(doc),
            Err(e) => {
                eprintln!("Warning: skipping '{}': {}", f.display(), e);
                None
            }
        })
        .collect();

    if docs.is_empty() {
        return Err("No readable markdown files found".into());
    }

    // 5. Tokenize query and deduplicate
    let query_tokens = dedup_tokens(tokenize(&args.query));
    if query_tokens.is_empty() {
        return Err("Search query is empty after tokenization".into());
    }

    // 6. Build index and search
    let index = SearchIndex::build(&docs);
    let results = index.search(&query_tokens);

    // 7. Take top N
    let results: Vec<(usize, f64)> = results.into_iter().take(args.limit).collect();

    if results.is_empty() {
        eprintln!("No results found for '{}'", args.query);
        return Ok(());
    }

    // 8. Print results
    print_results(&results, &docs, &query_tokens, args);

    // 9. Summary to stderr
    let elapsed = start_time.elapsed();
    let unique_files: HashSet<usize> = results.iter().map(|(id, _)| *id).collect();
    eprintln!(
        "\n  {} results in {} files ({})",
        results.len(),
        unique_files.len(),
        format_duration(elapsed),
    );

    Ok(())
}

fn format_duration(d: std::time::Duration) -> String {
    let micros = d.as_micros();
    if micros < 1_000 {
        format!("{}us", micros)
    } else if micros < 1_000_000 {
        format!("{:.1}ms", micros as f64 / 1_000.0)
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}
