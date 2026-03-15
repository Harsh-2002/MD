use std::process::Command;

// ── Helpers ──────────────────────────────────────────────────────────

fn run_md(args: &[&str], input_file: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(args)
        .arg(input_file)
        .env("NO_COLOR", "1")
        .output()
        .expect("Failed to execute mdx");
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn run_md_stdin(args: &[&str], input: &str) -> String {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start mdx");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn run_md_raw(args: &[&str], input: &[u8]) -> std::process::Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start mdx");
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let unique_name = format!("mdx-test-{}-{}", id, name);
    let path = std::env::temp_dir().join(unique_name);
    std::fs::write(&path, content).unwrap();
    path
}

// ── Headings ─────────────────────────────────────────────────────────

#[test]
fn test_h1_rendering() {
    let out = run_md_stdin(&["-w", "40"], "# Hello World");
    assert!(out.contains("HELLO WORLD"), "H1 should be uppercased");
    assert!(
        out.contains("════") || out.contains("===="),
        "H1 should have double-line rule"
    );
}

#[test]
fn test_h2_rendering() {
    let out = run_md_stdin(&["-w", "40"], "## Section");
    assert!(out.contains("Section"), "H2 text should appear");
    assert!(
        out.contains("────") || out.contains("----"),
        "H2 should have single-line rule"
    );
}

#[test]
fn test_h3_rendering() {
    let out = run_md_stdin(&["-w", "40"], "### Subsection");
    assert!(out.contains("Subsection"), "H3 text should appear");
    // H3 has no rule
    assert!(
        !out.contains("════") && !out.contains("────"),
        "H3 should not have rules"
    );
}

#[test]
fn test_h4_rendering() {
    let out = run_md_stdin(&["-w", "40"], "#### Detail");
    assert!(out.contains("Detail"), "H4 text should appear");
}

#[test]
fn test_h5_rendering() {
    let out = run_md_stdin(&["-w", "40"], "##### Minor");
    assert!(out.contains("Minor"), "H5 text should appear");
}

#[test]
fn test_h6_rendering() {
    let out = run_md_stdin(&["-w", "40"], "###### Smallest");
    assert!(out.contains("Smallest"), "H6 text should appear");
}

#[test]
fn test_all_heading_levels() {
    let input = "# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("H1"), "H1 present");
    assert!(out.contains("H2"), "H2 present");
    assert!(out.contains("H3"), "H3 present");
    assert!(out.contains("H4"), "H4 present");
    assert!(out.contains("H5"), "H5 present");
    assert!(out.contains("H6"), "H6 present");
}

#[test]
fn test_plain_headings_all_levels() {
    let input = "# One\n\n## Two\n\n### Three\n\n#### Four";
    let out = run_md_stdin(&["-w", "80", "--plain"], input);
    assert!(out.contains("# One"), "Plain H1 uses # prefix");
    assert!(out.contains("## Two"), "Plain H2 uses ## prefix");
    assert!(out.contains("### Three"), "Plain H3 uses ### prefix");
    assert!(out.contains("#### Four"), "Plain H4 uses #### prefix");
}

#[test]
fn test_empty_heading() {
    let out = run_md_stdin(&["-w", "80"], "# ");
    // Should not crash, may produce empty styled line
    assert!(
        out.contains("════") || out.contains("===="),
        "Empty H1 should still have rule"
    );
}

// ── Paragraphs ───────────────────────────────────────────────────────

#[test]
fn test_paragraph_wrapping() {
    let out = run_md_stdin(
        &["-w", "30"],
        "This is a long paragraph that should be word wrapped at the specified width.",
    );
    for line in out.lines() {
        assert!(line.len() <= 35, "Line too long: '{}'", line);
    }
}

#[test]
fn test_multiple_paragraphs() {
    let out = run_md_stdin(&["-w", "80"], "First paragraph.\n\nSecond paragraph.");
    assert!(out.contains("First paragraph"));
    assert!(out.contains("Second paragraph"));
}

// ── Inline formatting ────────────────────────────────────────────────

#[test]
fn test_bold_text() {
    let out = run_md_stdin(&["-w", "80"], "This is **bold** text");
    assert!(out.contains("bold"), "Bold text should appear");
    assert!(out.contains("text"), "Surrounding text should appear");
}

#[test]
fn test_italic_text() {
    let out = run_md_stdin(&["-w", "80"], "This is *italic* text");
    assert!(out.contains("italic"), "Italic text should appear");
}

#[test]
fn test_bold_italic_combined() {
    let out = run_md_stdin(&["-w", "80"], "This is ***bold and italic*** text");
    assert!(
        out.contains("bold and italic"),
        "Combined bold+italic text should appear"
    );
}

#[test]
fn test_inline_code() {
    let out = run_md_stdin(&["-w", "80"], "Use `println!` macro");
    assert!(out.contains("println!"), "Inline code should appear");
}

#[test]
fn test_strikethrough_fallback() {
    let out = run_md_stdin(&["-w", "80"], "This has ~~deleted~~ text");
    assert!(
        out.contains("~~deleted~~"),
        "Strikethrough should use ~~ markers when colors off: got '{}'",
        out
    );
}

#[test]
fn test_strikethrough_color_mode() {
    let output = run_md_raw(
        &["--color", "always", "-w", "80"],
        b"This has ~~deleted~~ text",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // In color mode, should use ANSI CrossedOut (\x1b[9m) instead of ~~ markers
    assert!(
        stdout.contains("\x1b[9m"),
        "Color mode should use ANSI strikethrough (CSI 9m): got '{}'",
        stdout
    );
    assert!(
        !stdout.contains("~~"),
        "Color mode should not use ~~ markers"
    );
}

// ── Links ────────────────────────────────────────────────────────────

#[test]
fn test_link_rendering() {
    let out = run_md_stdin(&["-w", "80"], "[Click here](https://example.com)");
    assert!(out.contains("Click here"), "Link text should appear");
    assert!(out.contains("https://example.com"), "URL should appear");
}

#[test]
fn test_autolink_no_duplicate() {
    let out = run_md_stdin(&["-w", "80"], "https://example.com");
    let count = out.matches("https://example.com").count();
    assert_eq!(
        count, 1,
        "Autolink URL should appear only once, got {}",
        count
    );
}

#[test]
fn test_osc8_hyperlinks() {
    let output = run_md_raw(
        &["--color", "always", "-w", "80"],
        b"[Click](https://example.com)",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\x1b]8;;https://example.com\x07"),
        "Should contain OSC 8 start: got '{}'",
        stdout
    );
    assert!(stdout.contains("\x1b]8;;\x07"), "Should contain OSC 8 end");
}

#[test]
fn test_osc8_not_in_no_color() {
    let out = run_md_stdin(&["-w", "80"], "[Click](https://example.com)");
    assert!(
        !out.contains("\x1b]8;;"),
        "OSC 8 should not appear when colors are off"
    );
}

#[test]
fn test_multiple_links_in_paragraph() {
    let out = run_md_stdin(
        &["-w", "80"],
        "Visit [one](https://one.com) and [two](https://two.com) sites.",
    );
    assert!(out.contains("one") && out.contains("two"));
    assert!(out.contains("https://one.com") && out.contains("https://two.com"));
}

// ── URL truncation ───────────────────────────────────────────────────

#[test]
fn test_short_url_renders_fully() {
    let out = run_md_stdin(&["-w", "80"], "[text](https://short.com)");
    assert!(
        out.contains("https://short.com"),
        "Short URL should render in full: got '{}'",
        out
    );
}

#[test]
fn test_long_url_truncated() {
    let long = format!("https://example.com/{}", "a".repeat(200));
    let input = format!("[text]({})", long);
    let out = run_md_stdin(&["-w", "80"], &input);
    assert!(
        out.contains("…"),
        "Long URL should be truncated with …: got '{}'",
        out
    );
    assert!(
        !out.contains(&long),
        "Full long URL should NOT appear in output"
    );
}

#[test]
fn test_autolink_not_truncated() {
    let long = format!("https://example.com/{}", "b".repeat(200));
    let out = run_md_stdin(&["-w", "80"], &long);
    // Autolinks show the URL as the link text; no " (url)" suffix is appended
    // so there's nothing to truncate — the URL should appear once as link text
    let count = out.matches("https://example.com").count();
    assert_eq!(
        count, 1,
        "Autolink should appear exactly once (no suffix): got {}",
        count
    );
}

#[test]
fn test_multiple_long_links_truncated() {
    let url1 = format!("https://one.com/{}", "x".repeat(200));
    let url2 = format!("https://two.com/{}", "y".repeat(200));
    let input = format!("[first]({}) and [second]({})", url1, url2);
    let out = run_md_stdin(&["-w", "80"], &input);
    let ellipsis_count = out.matches('…').count();
    assert!(
        ellipsis_count >= 2,
        "Both long URLs should be truncated with …, got {} ellipses in: {}",
        ellipsis_count,
        out
    );
}

// ── Images ───────────────────────────────────────────────────────────

#[test]
fn test_image_rendering() {
    let out = run_md_stdin(&["-w", "80"], "![Alt text](image.png)");
    assert!(out.contains("[Image:"), "Image should show [Image: marker");
    assert!(out.contains("Alt text"), "Alt text should appear");
}

#[test]
fn test_image_no_alt_text() {
    let out = run_md_stdin(&["-w", "80"], "![](photo.jpg)");
    assert!(
        out.contains("[Image:") || out.contains("photo.jpg"),
        "Image without alt text should still render"
    );
}

// ── Lists ────────────────────────────────────────────────────────────

#[test]
fn test_unordered_list() {
    let out = run_md_stdin(&["-w", "80"], "- Apple\n- Banana\n- Cherry");
    assert!(
        out.contains("Apple") && out.contains("Banana") && out.contains("Cherry"),
        "All list items should appear"
    );
}

#[test]
fn test_ordered_list() {
    let out = run_md_stdin(&["-w", "80"], "1. First\n2. Second\n3. Third");
    assert!(out.contains("1.") && out.contains("2.") && out.contains("3."));
}

#[test]
fn test_nested_unordered_list() {
    let input = "- Level 0\n  - Level 1\n    - Level 2";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Level 0"));
    assert!(out.contains("Level 1"));
    assert!(out.contains("Level 2"));
    // Should use different bullet chars per depth: ●/*/○/-/■/+ (Unicode or ASCII fallback)
    assert!(
        out.contains("●") || out.contains("*"),
        "Depth 0 should use ● or *"
    );
    assert!(
        out.contains("○") || out.contains("-"),
        "Depth 1 should use ○ or -"
    );
    assert!(
        out.contains("■") || out.contains("+"),
        "Depth 2 should use ■ or +"
    );
}

#[test]
fn test_nested_ordered_list() {
    let input = "1. Parent\n   1. Child A\n   2. Child B\n2. Next";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Parent") && out.contains("Child A") && out.contains("Child B"));
    assert!(out.contains("Next"));
}

#[test]
fn test_task_list() {
    let out = run_md_stdin(&["-w", "80"], "- [x] Done\n- [ ] Todo");
    assert!(out.contains("Done") && out.contains("Todo"));
    assert!(
        out.contains("✓") || out.contains("[x]"),
        "Checked task should use ✓ or [x]"
    );
    assert!(
        out.contains("☐") || out.contains("[ ]"),
        "Unchecked task should use ☐ or [ ]"
    );
}

#[test]
fn test_mixed_list_types() {
    let input = "- Unordered\n\n1. Ordered\n\n- [x] Task";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Unordered"));
    assert!(out.contains("Ordered"));
    assert!(out.contains("Task"));
}

// ── Code blocks ──────────────────────────────────────────────────────

#[test]
fn test_code_block() {
    let out = run_md_stdin(&["-w", "80"], "```rust\nfn main() {}\n```");
    assert!(out.contains("fn main()"), "Code content should appear");
    assert!(out.contains("rust"), "Language label should appear");
    assert!(
        out.contains("┌") || out.contains("+"),
        "Should have top border"
    );
    assert!(
        out.contains("└") || out.contains("+"),
        "Should have bottom border"
    );
}

#[test]
fn test_code_block_no_language() {
    let out = run_md_stdin(&["-w", "80"], "```\nhello world\n```");
    assert!(out.contains("hello world"), "Code content should appear");
    assert!(
        out.contains("┌") || out.contains("+"),
        "Should have borders without language"
    );
}

#[test]
fn test_code_block_multiple_languages() {
    let input = "```python\nprint('hi')\n```\n\n```javascript\nconsole.log('hi')\n```";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("python"), "Python label should appear");
    assert!(out.contains("javascript"), "JS label should appear");
    assert!(out.contains("print('hi')"));
    assert!(out.contains("console.log('hi')"));
}

#[test]
fn test_code_block_empty() {
    let out = run_md_stdin(&["-w", "80"], "```\n```");
    assert!(
        out.contains("┌") || out.contains("+"),
        "Empty code block should have borders"
    );
}

#[test]
fn test_indented_code_block() {
    let out = run_md_stdin(
        &["-w", "80"],
        "    fn hello() {\n        println!(\"hi\");\n    }\n",
    );
    assert!(
        out.contains("fn hello()"),
        "Indented code block content should appear"
    );
    assert!(
        out.contains("┌") || out.contains("+"),
        "Indented code block should have box borders"
    );
}

#[test]
fn test_code_block_long_line_truncation() {
    let long_line = "x".repeat(200);
    let input = format!("```\n{}\n```", long_line);
    let out = run_md_stdin(&["-w", "60"], &input);
    // Long lines should be truncated with ellipsis
    assert!(
        out.contains("…"),
        "Long code lines should be truncated with …: got '{}'",
        out
    );
}

// ── Blockquotes ──────────────────────────────────────────────────────

#[test]
fn test_blockquote() {
    let out = run_md_stdin(&["-w", "80"], "> This is a quote");
    assert!(
        out.contains("│") || out.contains("|"),
        "Should have blockquote bar"
    );
    assert!(out.contains("This is a quote"));
}

#[test]
fn test_nested_blockquotes() {
    let out = run_md_stdin(&["-w", "80"], "> Outer\n>\n> > Inner");
    assert!(out.contains("Outer") && out.contains("Inner"));
}

#[test]
fn test_deeply_nested_blockquotes() {
    let input = "> Level 1\n> > Level 2\n> > > Level 3";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Level 1"));
    assert!(out.contains("Level 2"));
    assert!(out.contains("Level 3"));
}

#[test]
fn test_blockquote_with_formatting() {
    let out = run_md_stdin(&["-w", "80"], "> Quote with **bold** and `code`");
    assert!(out.contains("bold"));
    assert!(out.contains("code"));
}

// ── Tables ───────────────────────────────────────────────────────────

#[test]
fn test_table() {
    let out = run_md_stdin(&["-w", "80"], "| A | B |\n|---|---|\n| 1 | 2 |");
    assert!(out.contains("A") && out.contains("B"));
    assert!(out.contains("1") && out.contains("2"));
    assert!(
        out.contains("┌") || out.contains("+"),
        "Should have table borders"
    );
}

#[test]
fn test_table_alignment() {
    let input = "| Left | Center | Right |\n|:-----|:------:|------:|\n| L | C | R |";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Left") && out.contains("Center") && out.contains("Right"));
    assert!(out.contains("L") && out.contains("C") && out.contains("R"));
}

#[test]
fn test_table_cell_wrapping() {
    let out = run_md_stdin(
        &["-w", "40"],
        "| Header | Description |\n|---|---|\n| Short | This is a very long cell content that should wrap across multiple lines instead of being truncated |",
    );
    assert!(out.contains("This is a"), "Wrapped content should appear");
    assert!(
        out.contains("wrap"),
        "Full content should appear across lines"
    );
    assert!(
        !out.contains("…"),
        "Content should wrap, not truncate: got '{}'",
        out
    );
}

#[test]
fn test_table_many_columns() {
    let input = "| A | B | C | D | E |\n|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 |";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("A") && out.contains("E"));
    assert!(out.contains("1") && out.contains("5"));
}

#[test]
fn test_table_with_inline_formatting() {
    let input = "| Feature | Status |\n|---|---|\n| **Bold** | `code` |";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Bold"));
    assert!(out.contains("code"));
}

#[test]
fn test_plain_mode_table() {
    let out = run_md_stdin(&["-w", "80", "--plain"], "| A | B |\n|---|---|\n| 1 | 2 |");
    assert!(out.contains("|"), "Plain table should use pipe separators");
    assert!(out.contains("A") && out.contains("B"));
    assert!(
        !out.contains("┌") && !out.contains("└"),
        "Plain table should not use box chars"
    );
}

#[test]
fn test_table_narrow_width() {
    let input =
        "| Column One | Column Two | Column Three |\n|---|---|---|\n| Data | More | Even More |";
    let out = run_md_stdin(&["-w", "30"], input);
    // Should not crash and content should appear
    assert!(
        out.contains("Column") || out.contains("Data"),
        "Table should render at narrow width"
    );
}

// ── Horizontal rules ─────────────────────────────────────────────────

#[test]
fn test_horizontal_rule() {
    let out = run_md_stdin(&["-w", "40"], "---");
    assert!(
        out.contains("────") || out.contains("----"),
        "Should render horizontal rule"
    );
}

#[test]
fn test_multiple_horizontal_rules() {
    let input = "Before\n\n---\n\nMiddle\n\n***\n\nAfter";
    let out = run_md_stdin(&["-w", "40"], input);
    assert!(out.contains("Before") && out.contains("Middle") && out.contains("After"));
}

// ── Alerts ───────────────────────────────────────────────────────────

#[test]
fn test_alert_note() {
    let out = run_md_stdin(&["-w", "80"], "> [!NOTE]\n> Important info here");
    assert!(out.contains("Note"), "Alert type should appear");
    assert!(out.contains("Important info here"));
}

#[test]
fn test_alert_tip() {
    let out = run_md_stdin(&["-w", "80"], "> [!TIP]\n> Helpful tip here");
    assert!(out.contains("Tip"), "TIP alert should appear");
    assert!(out.contains("Helpful tip here"));
}

#[test]
fn test_alert_important() {
    let out = run_md_stdin(&["-w", "80"], "> [!IMPORTANT]\n> Critical info");
    assert!(out.contains("Important"), "IMPORTANT alert should appear");
    assert!(out.contains("Critical info"));
}

#[test]
fn test_alert_warning() {
    let out = run_md_stdin(&["-w", "80"], "> [!WARNING]\n> Be careful here");
    assert!(out.contains("Warning"), "WARNING alert should appear");
    assert!(out.contains("Be careful here"));
}

#[test]
fn test_alert_caution() {
    let out = run_md_stdin(&["-w", "80"], "> [!CAUTION]\n> Danger ahead");
    assert!(out.contains("Caution"), "CAUTION alert should appear");
    assert!(out.contains("Danger ahead"));
}

#[test]
fn test_all_alerts_in_one_document() {
    let input = "> [!NOTE]\n> Note text\n\n> [!TIP]\n> Tip text\n\n> [!IMPORTANT]\n> Important text\n\n> [!WARNING]\n> Warning text\n\n> [!CAUTION]\n> Caution text";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Note") && out.contains("Note text"));
    assert!(out.contains("Tip") && out.contains("Tip text"));
    assert!(out.contains("Important") && out.contains("Important text"));
    assert!(out.contains("Warning") && out.contains("Warning text"));
    assert!(out.contains("Caution") && out.contains("Caution text"));
}

// ── Footnotes ────────────────────────────────────────────────────────

#[test]
fn test_footnote() {
    let out = run_md_stdin(&["-w", "80"], "Text[^1]\n\n[^1]: Footnote content.");
    assert!(out.contains("[1]"), "Footnote reference should appear");
    assert!(
        out.contains("Footnote content"),
        "Footnote definition should appear"
    );
}

#[test]
fn test_multiple_footnotes() {
    let input = "First[^1] and second[^2].\n\n[^1]: Note one.\n[^2]: Note two.";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(
        out.contains("[1]") && out.contains("[2]"),
        "Both references should appear"
    );
    assert!(out.contains("Note one") && out.contains("Note two"));
}

// ── Frontmatter ──────────────────────────────────────────────────────

#[test]
fn test_frontmatter_stripped() {
    let input = "---\ntitle: Test\nauthor: Me\n---\n\n# Hello World";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(!out.contains("title:"), "Frontmatter should be stripped");
    assert!(!out.contains("author:"), "Frontmatter should be stripped");
    assert!(
        out.contains("HELLO WORLD"),
        "Content after frontmatter should render"
    );
}

#[test]
fn test_frontmatter_complex() {
    let input = "---\ntitle: Test\ntags:\n  - rust\n  - markdown\n---\n\nContent here.";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(
        !out.contains("tags:"),
        "Complex frontmatter should be stripped"
    );
    assert!(out.contains("Content here"));
}

// ── Math ─────────────────────────────────────────────────────────────

#[test]
fn test_math_inline() {
    let input = "The value $\\alpha + \\beta$ is important.";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("α"), "Inline math should convert \\alpha to α");
    assert!(out.contains("β"), "Inline math should convert \\beta to β");
    assert!(
        !out.contains("$"),
        "Dollar signs should not appear in output"
    );
}

#[test]
fn test_math_display() {
    let input = "$$\n\\sum_{i=0}^{n} x^2\n$$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Σ"), "Display math should convert \\sum to Σ");
    assert!(
        out.contains("math"),
        "Display math should have 'math' label"
    );
}

#[test]
fn test_math_greek_letters() {
    let input = "$\\gamma \\delta \\theta \\lambda \\pi \\sigma \\omega$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("γ"), "gamma");
    assert!(out.contains("δ"), "delta");
    assert!(out.contains("θ"), "theta");
    assert!(out.contains("λ"), "lambda");
    assert!(out.contains("π"), "pi");
    assert!(out.contains("σ"), "sigma");
    assert!(out.contains("ω"), "omega");
}

#[test]
fn test_math_uppercase_greek() {
    let input = "$\\Gamma \\Delta \\Sigma \\Omega \\Pi$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Γ"), "Gamma");
    assert!(out.contains("Δ"), "Delta");
    assert!(out.contains("Σ"), "Sigma");
    assert!(out.contains("Ω"), "Omega");
    assert!(out.contains("Π"), "Pi");
}

#[test]
fn test_math_operators() {
    let input = "$\\pm \\times \\div \\cdot \\leq \\geq \\neq \\approx$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("±"), "pm");
    assert!(out.contains("×"), "times");
    assert!(out.contains("÷"), "div");
    assert!(out.contains("·"), "cdot");
    assert!(out.contains("≤"), "leq");
    assert!(out.contains("≥"), "geq");
    assert!(out.contains("≠"), "neq");
    assert!(out.contains("≈"), "approx");
}

#[test]
fn test_math_arrows() {
    let input = "$\\rightarrow \\leftarrow \\Rightarrow \\Leftarrow \\leftrightarrow$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("→"), "rightarrow");
    assert!(out.contains("←"), "leftarrow");
    assert!(out.contains("⇒"), "Rightarrow");
    assert!(out.contains("⇐"), "Leftarrow");
    assert!(out.contains("↔"), "leftrightarrow");
}

#[test]
fn test_math_sets_logic() {
    let input = "$\\in \\notin \\subset \\cup \\cap \\forall \\exists$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("∈"), "in");
    assert!(out.contains("∉"), "notin");
    assert!(out.contains("⊂"), "subset");
    assert!(out.contains("∪"), "cup");
    assert!(out.contains("∩"), "cap");
    assert!(out.contains("∀"), "forall");
    assert!(out.contains("∃"), "exists");
}

#[test]
fn test_math_special_symbols() {
    let input = "$\\infty \\partial \\nabla \\sqrt$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("∞"), "infty");
    assert!(out.contains("∂"), "partial");
    assert!(out.contains("∇"), "nabla");
    assert!(out.contains("√"), "sqrt");
}

#[test]
fn test_math_superscripts() {
    let input = "$x^0 x^1 x^2 x^3 x^4 x^5 x^6 x^7 x^8 x^9$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("⁰"), "superscript 0");
    assert!(out.contains("¹"), "superscript 1");
    assert!(out.contains("²"), "superscript 2");
    assert!(out.contains("³"), "superscript 3");
    assert!(out.contains("⁹"), "superscript 9");
}

#[test]
fn test_math_subscripts() {
    let input = "$x_0 x_1 x_2 x_3$";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("₀"), "subscript 0");
    assert!(out.contains("₁"), "subscript 1");
    assert!(out.contains("₂"), "subscript 2");
    assert!(out.contains("₃"), "subscript 3");
}

// ── Mermaid code blocks ──────────────────────────────────────────────

#[test]
fn test_mermaid_renders_as_code_block() {
    let input = "```mermaid\ngraph TD\n    A[Start] --> B[End]\n```";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(
        out.contains("mermaid"),
        "Should show mermaid language label"
    );
    assert!(out.contains("graph TD"), "Source code should be visible");
    assert!(
        out.contains("A[Start]") && out.contains("B[End]"),
        "Full source should be preserved"
    );
}

#[test]
fn test_mermaid_plain_mode_renders_as_code_block() {
    let input = "```mermaid\ngraph TD\n    A[Start] --> B[End]\n```";
    let out = run_md_stdin(&["-w", "80", "--plain"], input);
    assert!(
        out.contains("graph TD"),
        "Source code should be visible in plain mode"
    );
    assert!(
        out.contains("A[Start]") && out.contains("B[End]"),
        "Full source should be preserved in plain mode"
    );
}

// ── Themes and color modes ───────────────────────────────────────────

#[test]
fn test_no_color_mode() {
    let output = run_md_raw(&["--color", "never", "-w", "80"], b"# Test");
    assert!(
        !output.stdout.contains(&0x1b),
        "color=never should produce no ANSI escape codes"
    );
}

#[test]
fn test_color_always_mode() {
    let output = run_md_raw(&["--color", "always", "-w", "80"], b"# Test");
    assert!(
        output.stdout.contains(&0x1b),
        "color=always should produce ANSI escape codes"
    );
}

#[test]
fn test_light_theme() {
    let out = run_md_stdin(&["-w", "80", "--theme", "light"], "# Hello");
    assert!(
        out.contains("HELLO"),
        "Light theme should still uppercase H1"
    );
}

#[test]
fn test_dark_theme_explicit() {
    let out = run_md_stdin(&["-w", "80", "--theme", "dark"], "# Hello");
    assert!(out.contains("HELLO"), "Dark theme should uppercase H1");
}

// ── Plain mode ───────────────────────────────────────────────────────

#[test]
fn test_plain_mode_no_ansi() {
    let output = run_md_raw(
        &["--plain", "-w", "80"],
        b"# Hello\n\n---\n\n```rust\nlet x = 1;\n```",
    );
    assert!(
        !output.stdout.contains(&0x1b),
        "Plain mode should produce no ANSI escapes"
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("# Hello"),
        "Plain headings should use # markers"
    );
    assert!(text.contains("---"), "Plain rules should use ---");
    assert!(
        text.contains("    let x = 1;"),
        "Plain code should be indented by 4 spaces"
    );
}

#[test]
fn test_plain_mode_lists() {
    let out = run_md_stdin(&["-w", "80", "--plain"], "- Item A\n- Item B");
    assert!(out.contains("Item A") && out.contains("Item B"));
    // Plain mode uses ASCII bullets
    assert!(
        out.contains("*") || out.contains("-"),
        "Plain list should use ASCII bullets"
    );
}

#[test]
fn test_plain_mode_blockquote() {
    let out = run_md_stdin(&["-w", "80", "--plain"], "> Quoted text");
    assert!(out.contains("Quoted text"));
    assert!(out.contains("|"), "Plain blockquote should use | bar");
}

// ── Width handling ───────────────────────────────────────────────────

#[test]
fn test_width_constraint() {
    let out = run_md_stdin(&["-w", "30"], "---");
    for line in out.lines() {
        assert!(line.len() <= 100, "Line too long at w=30");
    }
}

#[test]
fn test_very_narrow_width() {
    let out = run_md_stdin(&["-w", "15"], "# Hello\n\nSome text here.");
    // Should not crash at very narrow width
    assert!(!out.is_empty(), "Should produce output at narrow width");
}

#[test]
fn test_wide_width() {
    let out = run_md_stdin(&["-w", "200"], "# Hello\n\nSome text.");
    assert!(out.contains("HELLO"));
    assert!(out.contains("Some text"));
}

// ── CLI options ──────────────────────────────────────────────────────

#[test]
fn test_list_syntax_themes() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .arg("--list-syntax-themes")
        .output()
        .expect("Failed to execute mdx");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("base16-ocean.dark"),
        "Should list default theme"
    );
    assert!(output.status.success());
}

#[test]
fn test_syntax_theme_flag() {
    let out = run_md_stdin(
        &["-w", "80", "--syntax-theme", "Solarized (dark)"],
        "```rust\nlet x = 42;\n```",
    );
    assert!(
        out.contains("let x"),
        "Code content should appear with custom theme"
    );
}

#[test]
fn test_generate_man() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .arg("--generate-man")
        .output()
        .expect("Failed to execute mdx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(".TH"),
        "Man page should contain .TH troff macro"
    );
}

#[test]
fn test_completions_bash() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["completions", "bash"])
        .output()
        .expect("Failed to execute mdx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("complete"),
        "Bash completions should contain 'complete'"
    );
}

#[test]
fn test_completions_zsh() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["completions", "zsh"])
        .output()
        .expect("Failed to execute mdx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("_mdx"),
        "Zsh completions should reference _mdx"
    );
}

#[test]
fn test_completions_fish() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["completions", "fish"])
        .output()
        .expect("Failed to execute mdx");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("complete") || stdout.contains("mdx"),
        "Fish completions should be generated"
    );
}

// ── Input sources ────────────────────────────────────────────────────

#[test]
fn test_empty_input() {
    let out = run_md_stdin(&["-w", "80"], "");
    assert!(
        out.is_empty() || out.trim().is_empty(),
        "Empty input should produce empty output"
    );
}

#[test]
fn test_file_input() {
    let tmp = write_tmp("md-test-file-input.md", "# From File\n\nFile content here.");
    let out = run_md(&["-w", "80"], tmp.to_str().unwrap());
    assert!(out.contains("FROM FILE"), "File input should render H1");
    assert!(out.contains("File content here"));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_nonexistent_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["-w", "80", "/tmp/md-nonexistent-file-12345.md"])
        .env("NO_COLOR", "1")
        .output()
        .expect("Failed to execute mdx");
    assert!(
        !output.status.success(),
        "Non-existent file should return error exit code"
    );
}

#[test]
fn test_url_detection() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["-w", "80", "http://127.0.0.1:1/nonexistent.md"])
        .env("NO_COLOR", "1")
        .output()
        .expect("Failed to execute mdx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Fetching") || stderr.contains("Error fetching"),
        "URL should be detected and fetched: got '{}'",
        stderr
    );
}

// ── TOC subcommand ───────────────────────────────────────────────────

#[test]
fn test_toc_output() {
    let tmp = write_tmp(
        "md-test-toc.md",
        "# Installation\n\n## Terminal Mode\n\n## Serve Mode\n\n### Advanced\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["toc"])
        .arg(&tmp)
        .output()
        .expect("Failed to execute md toc");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "toc command should succeed");
    assert!(stdout.contains("- [Installation](#installation)"));
    assert!(stdout.contains("- [Terminal Mode](#terminal-mode)"));
    assert!(stdout.contains("- [Serve Mode](#serve-mode)"));
    assert!(stdout.contains("- [Advanced](#advanced)"));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_toc_depth_limit() {
    let tmp = write_tmp(
        "md-test-toc-depth.md",
        "# H1\n\n## H2\n\n### H3\n\n#### H4\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["toc", "--depth", "2"])
        .arg(&tmp)
        .output()
        .expect("Failed to execute md toc");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("H1") && stdout.contains("H2"),
        "Should include h1 and h2"
    );
    assert!(!stdout.contains("H3"), "Should not include h3 at depth 2");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_toc_slug_generation() {
    let tmp = write_tmp(
        "md-test-toc-slug.md",
        "## Hello World\n\n## Special & Characters!\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["toc"])
        .arg(&tmp)
        .output()
        .expect("Failed to execute md toc");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("#hello-world"),
        "Slug should be lowercase with dashes"
    );
    let _ = std::fs::remove_file(&tmp);
}

// ── HTML output ──────────────────────────────────────────────────────

#[test]
fn test_html_output() {
    let tmp = write_tmp("md-test-html.md", "# Hello\n\nWorld");
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["export", "--to", "html"])
        .arg(&tmp)
        .output()
        .expect("Failed to execute mdx");
    assert!(output.status.success(), "HTML export should succeed");
    let html = String::from_utf8_lossy(&output.stdout);
    assert!(html.contains("<!DOCTYPE html>"), "Should be valid HTML");
    assert!(html.contains("Hello"), "HTML should contain heading");
    assert!(html.contains("World"), "HTML should contain content");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_html_output_contains_structure() {
    let tmp = write_tmp(
        "md-test-html-struct.md",
        "# Title\n\n```rust\nlet x = 1;\n```",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["export", "--to", "html"])
        .arg(&tmp)
        .output()
        .expect("Failed to execute mdx");
    assert!(output.status.success());
    let html = String::from_utf8_lossy(&output.stdout);
    assert!(html.contains("<article"), "Should have article element");
    assert!(
        html.contains("markdown-body"),
        "Should have markdown-body class"
    );
    let _ = std::fs::remove_file(&tmp);
}

// ── Edge cases ───────────────────────────────────────────────────────

#[test]
fn test_unicode_content() {
    let out = run_md_stdin(&["-w", "80"], "# 日本語テスト\n\nこんにちは世界");
    assert!(out.contains("日本語テスト") || out.contains("こんにちは世界"));
}

#[test]
fn test_emoji_content() {
    let out = run_md_stdin(&["-w", "80"], "Hello 🌍 World 🎉");
    assert!(out.contains("Hello") && out.contains("World"));
}

#[test]
fn test_mixed_content_document() {
    let input = r#"# Title

Paragraph with **bold** and *italic*.

> Blockquote

- List item 1
- List item 2

```python
print("hello")
```

| A | B |
|---|---|
| 1 | 2 |

---

[Link](https://example.com)

Text[^1]

[^1]: Footnote.
"#;
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("TITLE"), "H1 rendered");
    assert!(out.contains("bold"), "Bold text");
    assert!(out.contains("italic"), "Italic text");
    assert!(out.contains("Blockquote"), "Blockquote");
    assert!(out.contains("List item 1"), "List");
    assert!(out.contains("print"), "Code block");
    assert!(out.contains("A") && out.contains("B"), "Table");
    assert!(out.contains("Link"), "Link text");
    assert!(out.contains("[1]"), "Footnote ref");
}

#[test]
fn test_deeply_nested_structure() {
    let input = "> > > > Deep quote\n\n- L1\n  - L2\n    - L3\n      - L4";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Deep quote"));
    assert!(out.contains("L1") && out.contains("L2") && out.contains("L3") && out.contains("L4"));
}

#[test]
fn test_long_document() {
    let mut input = String::new();
    for i in 0..50 {
        input.push_str(&format!("## Section {}\n\nParagraph {} content.\n\n", i, i));
    }
    let out = run_md_stdin(&["-w", "80"], &input);
    assert!(out.contains("Section 0"), "First section");
    assert!(out.contains("Section 49"), "Last section");
}

#[test]
fn test_special_characters_in_text() {
    let out = run_md_stdin(&["-w", "80"], "Angle <brackets> and &amp; entities");
    assert!(
        out.contains("Angle"),
        "Text with special chars should render"
    );
}

#[test]
fn test_html_inline_stripped() {
    let out = run_md_stdin(&["-w", "80"], "This has <sub>subscript</sub> text");
    assert!(
        out.contains("subscript"),
        "HTML inline content should appear"
    );
}

#[test]
fn test_only_whitespace_input() {
    let out = run_md_stdin(&["-w", "80"], "   \n\n   \n");
    assert!(
        out.trim().is_empty(),
        "Whitespace-only input should produce empty output"
    );
}

#[test]
fn test_consecutive_code_blocks() {
    let input = "```\nBlock 1\n```\n\n```\nBlock 2\n```\n\n```\nBlock 3\n```";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Block 1") && out.contains("Block 2") && out.contains("Block 3"));
}

#[test]
fn test_list_with_paragraphs() {
    let input = "- Item 1\n\n  Continuation of item 1.\n\n- Item 2";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("Item 1"));
    assert!(out.contains("Continuation"));
    assert!(out.contains("Item 2"));
}

#[test]
fn test_blockquote_with_code() {
    let input = "> ```\n> code here\n> ```";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(
        out.contains("code here"),
        "Code in blockquote should render"
    );
}

#[test]
fn test_table_with_empty_cells() {
    let input = "| A | B | C |\n|---|---|---|\n| 1 | | 3 |";
    let out = run_md_stdin(&["-w", "80"], input);
    assert!(out.contains("A") && out.contains("C"));
    assert!(out.contains("1") && out.contains("3"));
}

// ── Fixture files ────────────────────────────────────────────────────

#[test]
fn test_fixture_files_no_crash() {
    let fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    for entry in std::fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
                .args(["-w", "80"])
                .arg(&path)
                .env("NO_COLOR", "1")
                .output()
                .expect("Failed to execute mdx");
            assert!(
                output.status.success(),
                "Failed to render {:?}: {}",
                path,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn test_fixture_files_plain_mode() {
    let fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    for entry in std::fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
                .args(["-w", "80", "--plain"])
                .arg(&path)
                .env("NO_COLOR", "1")
                .output()
                .expect("Failed to execute mdx");
            assert!(
                output.status.success(),
                "Failed to render {:?} in plain mode: {}",
                path,
                String::from_utf8_lossy(&output.stderr)
            );
            // Plain mode should have no ANSI codes
            assert!(
                !output.stdout.contains(&0x1b),
                "Plain mode has ANSI in {:?}",
                path
            );
        }
    }
}

#[test]
fn test_fixture_files_color_mode() {
    let fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    for entry in std::fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
                .args(["--color", "always", "-w", "80"])
                .arg(&path)
                .output()
                .expect("Failed to execute mdx");
            assert!(
                output.status.success(),
                "Failed to render {:?} in color mode: {}",
                path,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn test_fixture_files_narrow_width() {
    let fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    for entry in std::fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
                .args(["-w", "20"])
                .arg(&path)
                .env("NO_COLOR", "1")
                .output()
                .expect("Failed to execute mdx");
            assert!(
                output.status.success(),
                "Failed to render {:?} at width 20: {}",
                path,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

// ── EPUB export ─────────────────────────────────────────────────────

#[test]
fn test_export_epub_creates_file() {
    let tmp = write_tmp("md-test-epub.md", "# Hello World\n\nSome **bold** text.");
    let epub_path = tmp.with_extension("epub");
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["export", "--to", "epub"])
        .arg(&tmp)
        .output()
        .expect("Failed to execute mdx");
    assert!(
        output.status.success(),
        "EPUB export should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(epub_path.exists(), "EPUB file should be created");
    let bytes = std::fs::read(&epub_path).unwrap();
    assert!(bytes.len() > 100, "EPUB file should have content");
    assert_eq!(
        &bytes[0..2],
        b"PK",
        "EPUB should be a zip file (PK magic bytes)"
    );
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&epub_path);
}

#[test]
fn test_export_epub_custom_output() {
    let tmp = write_tmp("md-test-epub-custom.md", "# Custom Output\n\nTest content.");
    let custom_output = std::env::temp_dir().join("mdx-test-custom-output.epub");
    let output = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .args(["export", "--to", "epub", "-o"])
        .arg(&custom_output)
        .arg(&tmp)
        .output()
        .expect("Failed to execute mdx");
    assert!(
        output.status.success(),
        "EPUB export with -o should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        custom_output.exists(),
        "Custom output EPUB file should be created"
    );
    let bytes = std::fs::read(&custom_output).unwrap();
    assert_eq!(&bytes[0..2], b"PK", "EPUB should be a zip file");
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&custom_output);
}
