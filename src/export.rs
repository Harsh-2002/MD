use std::path::PathBuf;

use comrak::nodes::{AlertType, AstNode, ListType, NodeValue};
#[cfg(not(feature = "images"))]
use genpdfi::Position;
#[cfg(feature = "images")]
use genpdfi::Scale;
use genpdfi::elements;
use genpdfi::style;
use genpdfi::{Element, Mm};

use crate::parse::parse_markdown;

/// A4 content area dimensions (margins: 20/15/20/15mm from 210×297mm)
#[cfg(feature = "images")]
const CONTENT_WIDTH_MM: f32 = 180.0;
#[cfg(feature = "images")]
const MAX_IMAGE_HEIGHT_MM: f32 = 230.0;

pub struct ExportArgs {
    pub file: Option<String>,
    pub to: String,
    pub output: Option<String>,
}

pub fn run(args: &ExportArgs) -> Result<(), Box<dyn std::error::Error>> {
    let content = read_input(&args.file)?;
    let title = args.file.as_deref().unwrap_or("document");

    match args.to.as_str() {
        "html" => {
            let html = crate::html::render_standalone(
                &content,
                "base16-ocean.dark",
                &crate::cli::ThemeName::Dark,
                title,
                "",
            );
            print!("{}", html);
        }
        "json" => {
            let arena = typed_arena::Arena::new();
            let root = parse_markdown(&arena, &content);
            let json = ast_to_json(root, 0);
            println!("{}", json);
        }
        "txt" => {
            let arena = typed_arena::Arena::new();
            let root = parse_markdown(&arena, &content);
            let text = extract_plain_text(root);
            print!("{}", text);
        }
        "pdf" => {
            let output_path = args
                .output
                .clone()
                .or_else(|| {
                    args.file
                        .as_ref()
                        .map(|f| f.replace(".md", ".pdf").replace(".markdown", ".pdf"))
                })
                .unwrap_or_else(|| "output.pdf".to_string());
            export_pdf(&content, &output_path)?;
        }
        "epub" => {
            let output_path = args
                .output
                .clone()
                .or_else(|| {
                    args.file
                        .as_ref()
                        .map(|f| f.replace(".md", ".epub").replace(".markdown", ".epub"))
                })
                .unwrap_or_else(|| "output.epub".to_string());
            export_epub(&content, &output_path, args.file.as_deref())?;
        }
        other => {
            return Err(format!(
                "Unsupported format: '{}'. Supported: html, json, txt, pdf, epub",
                other
            )
            .into());
        }
    }

    Ok(())
}

// ─── PDF Export via genpdfi ───────────────────────────────────────────────────

const HEADING_SIZES: [u8; 6] = [24, 20, 16, 14, 12, 11];

/// A wrapper element that draws a filled background color behind its content.
/// Uses PDF layers: background on current layer, content on next layer (on top).
struct FilledElement<E: Element> {
    element: E,
    bg_color: style::Color,
    pad_v: Mm,
    pad_h: Mm,
    corner_radius: f32,
}

impl<E: Element> FilledElement<E> {
    fn new(element: E, bg_color: style::Color, pad_v: impl Into<Mm>, pad_h: impl Into<Mm>) -> Self {
        Self {
            element,
            bg_color,
            pad_v: pad_v.into(),
            pad_h: pad_h.into(),
            corner_radius: 4.0,
        }
    }

    fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }
}

impl<E: Element> Element for FilledElement<E> {
    fn render(
        &mut self,
        context: &genpdfi::Context,
        area: genpdfi::render::Area<'_>,
        style: style::Style,
    ) -> Result<genpdfi::RenderResult, genpdfi::error::Error> {
        // Render content on the NEXT PDF layer (will appear visually on top)
        let mut content_area = area.next_layer();
        content_area.add_margins(genpdfi::Margins::vh(self.pad_v, self.pad_h));

        let mut result = self.element.render(context, content_area, style)?;

        // Calculate total size including padding, capped to available area
        let total_height = result.size.height + self.pad_v + self.pad_v;
        let area_height = area.size().height;
        let total_height = if total_height > area_height {
            area_height
        } else {
            total_height
        };
        let total_width = area.size().width;

        // Draw background on the ORIGINAL layer (behind text)
        draw_filled_background(
            self.bg_color,
            total_width,
            total_height,
            self.corner_radius,
            context,
            area,
            style,
        );

        result.size.width = total_width;
        result.size.height = total_height;

        Ok(result)
    }
}

/// Draw a filled background behind content. Tries rounded corners (image-based)
/// first, falls back to a thick-line rectangle if that fails or images feature is off.
#[cfg(feature = "images")]
fn draw_filled_background(
    color: style::Color,
    total_width: Mm,
    total_height: Mm,
    corner_radius: f32,
    context: &genpdfi::Context,
    area: genpdfi::render::Area<'_>,
    style: style::Style,
) {
    let w_f32: f32 = total_width.into();
    let h_f32: f32 = total_height.into();
    if !render_rounded_bg_on_area(w_f32, h_f32, corner_radius, color, context, area, style) {
        // Fallback: image rendering failed, but area was consumed. No background drawn.
        // This only happens if temp dir is unwritable or image encoding fails.
    }
}

#[cfg(not(feature = "images"))]
fn draw_filled_background(
    color: style::Color,
    total_width: Mm,
    total_height: Mm,
    _corner_radius: f32,
    _context: &genpdfi::Context,
    area: genpdfi::render::Area<'_>,
    _style: style::Style,
) {
    let mid_y = total_height / 2.0;
    let bg_style = style::LineStyle::new()
        .with_thickness(total_height)
        .with_color(color);
    area.draw_line(
        vec![
            Position::new(Mm::from(0), mid_y),
            Position::new(total_width, mid_y),
        ],
        bg_style,
    );
}

/// Create a rounded-rect background image, save to temp file, load via genpdfi,
/// and render it on the given area. Returns true on success.
/// Uses temp file to bridge image 0.25 (our crate) → image 0.24 (genpdfi's crate).
#[cfg(feature = "images")]
fn render_rounded_bg_on_area(
    width_mm: f32,
    height_mm: f32,
    radius_mm: f32,
    color: style::Color,
    context: &genpdfi::Context,
    area: genpdfi::render::Area<'_>,
    pdf_style: style::Style,
) -> bool {
    let dpi = 144.0_f32;
    let px_w = (width_mm * dpi / 25.4).round().max(1.0) as u32;
    let px_h = (height_mm * dpi / 25.4).round().max(1.0) as u32;
    // Clamp radius so it never exceeds half the smaller dimension
    let r = ((radius_mm * dpi / 25.4).round() as u32)
        .min(px_w / 2)
        .min(px_h / 2);

    let (cr, cg, cb) = match color {
        style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (243, 244, 248),
    };

    let mut img = image::RgbImage::from_pixel(px_w, px_h, image::Rgb([255, 255, 255]));

    if r > 0 {
        // Corner centers: inset by r from each edge
        let cx_left = r;
        let cx_right = px_w.saturating_sub(r);
        let cy_top = r;
        let cy_bottom = px_h.saturating_sub(r);
        let r_sq = r * r;

        for y in 0..px_h {
            for x in 0..px_w {
                let inside = if x < cx_left && y < cy_top {
                    // Top-left corner
                    let dx = cx_left - x;
                    let dy = cy_top - y;
                    dx * dx + dy * dy <= r_sq
                } else if x >= cx_right && y < cy_top {
                    // Top-right corner
                    let dx = x - cx_right;
                    let dy = cy_top - y;
                    dx * dx + dy * dy <= r_sq
                } else if x < cx_left && y >= cy_bottom {
                    // Bottom-left corner
                    let dx = cx_left - x;
                    let dy = y - cy_bottom;
                    dx * dx + dy * dy <= r_sq
                } else if x >= cx_right && y >= cy_bottom {
                    // Bottom-right corner
                    let dx = x - cx_right;
                    let dy = y - cy_bottom;
                    dx * dx + dy * dy <= r_sq
                } else {
                    true
                };

                if inside {
                    img.put_pixel(x, y, image::Rgb([cr, cg, cb]));
                }
            }
        }
    } else {
        // No rounding needed — fill entire image
        for y in 0..px_h {
            for x in 0..px_w {
                img.put_pixel(x, y, image::Rgb([cr, cg, cb]));
            }
        }
    }

    // Save as PNG (lossless — avoids JPEG compression artifacts on solid colors).
    // Use PID in filename to avoid race conditions with concurrent exports.
    let temp_path = std::env::temp_dir().join(format!("md-pdf-code-bg-{}.png", std::process::id()));
    if img.save(&temp_path).is_err() {
        return false;
    }

    let ok = if let Ok(mut bg_element) = elements::Image::from_path(&temp_path) {
        bg_element.set_dpi(dpi);
        let _ = bg_element.render(context, area, pdf_style);
        true
    } else {
        false
    };

    let _ = std::fs::remove_file(&temp_path);
    ok
}

/// Walk the AST to find the first H1 heading's text for use as document title.
fn extract_title<'a>(root: &'a AstNode<'a>) -> Option<String> {
    for node in root.descendants() {
        let data = node.data.borrow();
        if let NodeValue::Heading(h) = &data.value
            && h.level == 1
        {
            drop(data);
            let title = collect_text(node);
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}

// ─── EPUB Export via epub-builder ────────────────────────────────────────────

fn export_epub(
    markdown: &str,
    output_path: &str,
    source_file: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ZipLibrary};

    let fm = crate::frontmatter::parse(markdown);

    let title = fm
        .title
        .clone()
        .or_else(|| {
            let arena = typed_arena::Arena::new();
            let root = parse_markdown(&arena, markdown);
            extract_title(root)
        })
        .or_else(|| {
            source_file.map(|f| {
                std::path::Path::new(f)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
        })
        .unwrap_or_else(|| "Untitled".to_string());

    let html_fragment = crate::html::render_fragment(markdown, "base16-ocean.dark");

    let base_dir = source_file
        .map(|f| {
            std::path::Path::new(f)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf()
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let (processed_html, images) = process_images(&html_fragment, &base_dir);
    let xhtml_body = html_to_xhtml(&processed_html);

    let xhtml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
    <title>{title}</title>
    <link rel="stylesheet" type="text/css" href="stylesheet.css" />
</head>
<body>
{xhtml_body}
</body>
</html>"#
    );

    let mut builder = EpubBuilder::new(ZipLibrary::new()?)?;
    builder.epub_version(EpubVersion::V30);
    builder.metadata("title", &title)?;
    if let Some(ref date) = fm.date {
        builder.metadata("description", format!("Date: {}", date))?;
    }
    for tag in &fm.tags {
        builder.metadata("subject", tag)?;
    }

    builder.stylesheet(crate::html::assets::EPUB_CSS.as_bytes())?;

    for (epub_path, mime, bytes) in &images {
        builder.add_resource(epub_path, bytes.as_slice(), mime)?;
    }

    builder.add_content(EpubContent::new("content.xhtml", xhtml.as_bytes()).title(&title))?;

    let mut output_file = std::fs::File::create(output_path)?;
    builder.generate(&mut output_file)?;

    eprintln!("  Written to {}", output_path);
    Ok(())
}

fn process_images(
    html: &str,
    base_dir: &std::path::Path,
) -> (String, Vec<(String, String, Vec<u8>)>) {
    let mut images = Vec::new();
    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some(img_start) = remaining.find("<img") {
        result.push_str(&remaining[..img_start]);
        let after_img = &remaining[img_start..];

        if let Some(tag_end) = after_img.find('>') {
            let tag = &after_img[..=tag_end];

            if let Some(src) = extract_attr(tag, "src")
                && !src.starts_with("http://")
                && !src.starts_with("https://")
                && !src.starts_with("data:")
            {
                let file_path = base_dir.join(&src);
                if let Ok(bytes) = std::fs::read(&file_path) {
                    let filename = std::path::Path::new(&src)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let epub_path = format!("images/{}", filename);
                    let mime = mime_from_ext(&src);
                    let new_tag =
                        tag.replace(&format!("\"{}\"", src), &format!("\"{}\"", epub_path));
                    result.push_str(&new_tag);
                    images.push((epub_path, mime, bytes));
                    remaining = &remaining[img_start + tag_end + 1..];
                    continue;
                }
            }

            result.push_str(tag);
            remaining = &remaining[img_start + tag_end + 1..];
        } else {
            result.push_str(after_img);
            remaining = "";
        }
    }
    result.push_str(remaining);

    (result, images)
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr_name);
    if let Some(start) = tag.find(&pattern) {
        let value_start = start + pattern.len();
        if let Some(end) = tag[value_start..].find('"') {
            return Some(tag[value_start..value_start + end].to_string());
        }
    }
    None
}

fn mime_from_ext(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn html_to_xhtml(html: &str) -> String {
    html.replace("<br>", "<br />").replace("<hr>", "<hr />")
}

// ─── PDF Export via genpdfi ───────────────────────────────────────────────────

pub fn export_pdf(markdown: &str, output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let arena = typed_arena::Arena::new();
    let root = parse_markdown(&arena, markdown);

    // Load built-in Helvetica font (no external font files needed)
    let font = markdown2pdf::fonts::load_builtin_font_family("Helvetica")
        .map_err(|e| format!("Font error: {}", e))?;

    let mut doc = genpdfi::Document::new(font);

    // Load Courier for monospace code blocks
    let courier = markdown2pdf::fonts::load_builtin_font_family("Courier")
        .map_err(|e| format!("Font error: {}", e))?;
    let courier_ref = doc.add_font_family(courier);

    let title = extract_title(root).unwrap_or_else(|| "document".to_string());
    doc.set_title(&title);
    doc.set_font_size(11);
    doc.set_line_spacing(1.25);
    doc.set_paper_size(genpdfi::PaperSize::A4);
    doc.set_minimal_conformance();

    let mut decorator = genpdfi::SimplePageDecorator::new();
    decorator.set_margins(genpdfi::Margins::trbl(20, 15, 20, 15));
    decorator.set_header(|page| {
        let mut p = elements::Paragraph::default();
        p.set_alignment(genpdfi::Alignment::Right);
        p.push_styled(
            format!("Page {}", page),
            style::Style::new()
                .with_font_size(9)
                .with_color(style::Color::Rgb(150, 150, 155)),
        );
        elements::PaddedElement::new(p, genpdfi::Margins::trbl(0, 0, 3, 0))
    });
    doc.set_page_decorator(decorator);

    // Track temp files from mermaid rendering
    let mut temp_files: Vec<PathBuf> = Vec::new();
    let mut first_h1_seen = false;

    // Walk AST and push PDF elements
    render_blocks(
        &mut doc,
        root,
        &mut temp_files,
        courier_ref,
        &mut first_h1_seen,
    );

    // Render footnote definitions at the end of the document
    render_footnotes(&mut doc, root, courier_ref);

    // Render to file
    doc.render_to_file(output_path)
        .map_err(|e| format!("PDF write error: {}", e))?;

    // Clean up temp files
    for path in &temp_files {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_dir(std::env::temp_dir().join("md-mermaid-export"));

    eprintln!("  Wrote {}", output_path);
    Ok(())
}

fn render_blocks<'a>(
    doc: &mut genpdfi::Document,
    node: &'a AstNode<'a>,
    temp_files: &mut Vec<PathBuf>,
    mono_font: genpdfi::fonts::FontFamily<genpdfi::fonts::Font>,
    first_h1_seen: &mut bool,
) {
    for child in node.children() {
        render_block(doc, child, temp_files, mono_font, first_h1_seen);
    }
}

fn render_block<'a>(
    doc: &mut genpdfi::Document,
    node: &'a AstNode<'a>,
    temp_files: &mut Vec<PathBuf>,
    mono_font: genpdfi::fonts::FontFamily<genpdfi::fonts::Font>,
    first_h1_seen: &mut bool,
) {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Document => {
            drop(data);
            render_blocks(doc, node, temp_files, mono_font, first_h1_seen);
        }
        NodeValue::Heading(h) => {
            let level = h.level as usize;
            let size = HEADING_SIZES
                .get(level.saturating_sub(1))
                .copied()
                .unwrap_or(11);
            drop(data);

            // Page break before H1 (except the very first one)
            if level == 1 {
                if *first_h1_seen {
                    doc.push(elements::PageBreak::new());
                }
                *first_h1_seen = true;
            }

            doc.push(elements::Break::new(1.5));
            let mut p = elements::Paragraph::default();
            let base = style::Style::new()
                .with_font_size(size)
                .bold()
                .with_color(style::Color::Rgb(25, 25, 35));
            collect_inline(&mut p, node, base, mono_font);
            doc.push(p);

            // Underline for H1 and H2
            if level <= 2 {
                let rule_char = if level == 1 { "\u{2501}" } else { "\u{2500}" };
                let rule_color = if level == 1 {
                    style::Color::Rgb(50, 50, 60)
                } else {
                    style::Color::Rgb(210, 212, 218)
                };
                let mut rule = elements::Paragraph::default();
                rule.push_styled(
                    rule_char.repeat(200),
                    style::Style::new().with_font_size(4).with_color(rule_color),
                );
                doc.push(rule);
            }
            doc.push(elements::Break::new(0.8));
        }
        NodeValue::Paragraph => {
            drop(data);
            let mut p = elements::Paragraph::default();
            let base = style::Style::new()
                .with_font_size(11)
                .with_color(style::Color::Rgb(30, 30, 30));
            collect_inline(&mut p, node, base, mono_font);
            doc.push(p);
            doc.push(elements::Break::new(0.5));

            // Embed any local images found in this paragraph
            embed_inline_images(doc, node);
        }
        NodeValue::CodeBlock(cb) => {
            let info = cb.info.clone();
            let literal = cb.literal.clone();
            drop(data);

            // Mermaid diagrams: render as image via kroki.io
            if info == "mermaid"
                && let Some((img_element, path)) =
                    render_mermaid_to_image(&literal, temp_files.len())
            {
                doc.push(elements::Break::new(0.5));
                doc.push(img_element);
                doc.push(elements::Break::new(0.5));
                temp_files.push(path);
                return;
            }

            // Regular code block: monospace font with soft background
            doc.push(elements::Break::new(0.5));

            // Language label above the code block
            if !info.is_empty() {
                let lang = info.split_whitespace().next().unwrap_or(&info);
                let mut lang_p = elements::Paragraph::default();
                lang_p.push_styled(
                    format!("  {}", lang),
                    style::Style::new()
                        .with_font_size(8)
                        .with_font_family(mono_font)
                        .with_color(style::Color::Rgb(130, 130, 140)),
                );
                doc.push(lang_p);
                doc.push(elements::Break::new(0.15));
            }

            let code_style = style::Style::new()
                .with_font_size(9)
                .with_font_family(mono_font)
                .with_color(style::Color::Rgb(40, 42, 54));

            // Courier at 9pt ≈ 2.4mm/char → ~71 chars fit in content width minus padding
            let max_chars = 71;
            let mut layout = elements::LinearLayout::vertical();
            for line in literal.lines() {
                let mut p = elements::Paragraph::default();
                let display = truncate_line(line, max_chars);
                p.push_styled(display, code_style);
                layout.push(p);
            }

            // Soft gray background behind code content
            doc.push(FilledElement::new(
                layout,
                style::Color::Rgb(243, 244, 248),
                3, // vertical padding (mm)
                4, // horizontal padding (mm)
            ));
            doc.push(elements::Break::new(0.5));
        }
        NodeValue::List(list) => {
            let lt = list.list_type;
            let start = list.start;
            drop(data);
            render_list(doc, node, lt, start, temp_files, mono_font, first_h1_seen);
            doc.push(elements::Break::new(0.3));
        }
        NodeValue::Item(_) | NodeValue::TaskItem(_) => {
            // Handled by render_list
            drop(data);
        }
        NodeValue::BlockQuote => {
            drop(data);
            doc.push(elements::Break::new(0.3));
            let bar_color = style::Color::Rgb(180, 185, 195);
            for child in node.children() {
                let cd = child.data.borrow();
                if matches!(&cd.value, NodeValue::Paragraph) {
                    drop(cd);
                    let mut p = elements::Paragraph::default();
                    let qs = style::Style::new()
                        .with_font_size(11)
                        .italic()
                        .with_color(style::Color::Rgb(80, 80, 95));
                    p.push_styled(
                        "  \u{2503} ",
                        style::Style::new().with_color(bar_color).bold(),
                    );
                    collect_inline(&mut p, child, qs, mono_font);
                    doc.push(p);
                    doc.push(elements::Break::new(0.2));
                } else {
                    drop(cd);
                    render_block(doc, child, temp_files, mono_font, first_h1_seen);
                }
            }
            doc.push(elements::Break::new(0.3));
        }
        NodeValue::Table(_) => {
            drop(data);
            render_table(doc, node, mono_font);
        }
        NodeValue::ThematicBreak => {
            drop(data);
            doc.push(elements::Break::new(0.5));
            let mut p = elements::Paragraph::default();
            p.set_alignment(genpdfi::Alignment::Center);
            p.push_styled(
                "\u{2500}".repeat(60),
                style::Style::new().with_color(style::Color::Rgb(200, 200, 205)),
            );
            doc.push(p);
            doc.push(elements::Break::new(0.5));
        }
        NodeValue::FrontMatter(_)
        | NodeValue::HtmlBlock(_)
        | NodeValue::HtmlInline(_)
        | NodeValue::FootnoteDefinition(_) => {
            drop(data);
        }
        NodeValue::Alert(alert) => {
            let alert_type = alert.alert_type;
            drop(data);
            render_alert_block(doc, node, alert_type, temp_files, mono_font, first_h1_seen);
        }
        _ => {
            drop(data);
            render_blocks(doc, node, temp_files, mono_font, first_h1_seen);
        }
    }
}

fn collect_inline<'a>(
    p: &mut elements::Paragraph,
    node: &'a AstNode<'a>,
    base: style::Style,
    mono_font: genpdfi::fonts::FontFamily<genpdfi::fonts::Font>,
) {
    for child in node.children() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::Text(t) => {
                let t = t.clone();
                drop(data);
                p.push_styled(t, base);
            }
            NodeValue::Code(c) => {
                let literal = c.literal.clone();
                drop(data);
                let cs = style::Style::new()
                    .with_font_size(10)
                    .with_font_family(mono_font)
                    .with_color(style::Color::Rgb(200, 55, 90));
                p.push_styled(format!("`{}`", literal), cs);
            }
            NodeValue::Emph => {
                drop(data);
                collect_inline(p, child, base.italic(), mono_font);
            }
            NodeValue::Strong => {
                drop(data);
                collect_inline(p, child, base.bold(), mono_font);
            }
            NodeValue::Strikethrough => {
                drop(data);
                collect_inline(
                    p,
                    child,
                    base.with_color(style::Color::Rgb(150, 150, 150)),
                    mono_font,
                );
            }
            NodeValue::Link(link) => {
                let url = link.url.clone();
                drop(data);
                let link_style = base.with_color(style::Color::Rgb(0, 95, 204)).underline();
                let text = collect_text(child);
                p.push_link(text, url, link_style);
            }
            NodeValue::Image(img) => {
                let title = img.title.clone();
                drop(data);
                // Prefer alt text from children, fall back to title attribute
                let text = collect_text(child);
                let text = if text.is_empty() { title } else { text };
                if !text.is_empty() {
                    p.push_styled(format!("[{}]", text), base.italic());
                }
            }
            NodeValue::SoftBreak => {
                drop(data);
                p.push_styled(" ", base);
            }
            NodeValue::LineBreak => {
                drop(data);
                p.push_styled("\n", base);
            }
            NodeValue::Math(m) => {
                let literal = m.literal.clone();
                drop(data);
                p.push_styled(literal, base.italic());
            }
            NodeValue::FootnoteReference(r) => {
                let name = r.name.clone();
                drop(data);
                p.push_styled(
                    format!("[{}]", name),
                    style::Style::new()
                        .with_font_size(9)
                        .with_color(style::Color::Rgb(100, 100, 110)),
                );
            }
            _ => {
                drop(data);
                collect_inline(p, child, base, mono_font);
            }
        }
    }
}

/// Scan a paragraph node tree for Image nodes (including nested in links/emphasis)
/// and embed local files as block-level images.
/// URLs and missing files fall back to the [alt text] already rendered by collect_inline.
#[cfg(feature = "images")]
fn embed_inline_images<'a>(doc: &mut genpdfi::Document, node: &'a AstNode<'a>) {
    for descendant in node.descendants() {
        let data = descendant.data.borrow();
        if let NodeValue::Image(img) = &data.value {
            let url = img.url.clone();
            drop(data);

            // Skip remote URLs and data URIs — only embed local files
            if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:")
            {
                continue;
            }

            let path = std::path::Path::new(&url);
            if !path.exists() {
                continue;
            }

            if let Ok(img_element) = elements::Image::from_path(path) {
                let img_element = scale_image_to_fit(img_element, path);
                doc.push(img_element.with_alignment(genpdfi::Alignment::Center));
                doc.push(elements::Break::new(0.5));
            }
        } else {
            drop(data);
        }
    }
}

#[cfg(not(feature = "images"))]
fn embed_inline_images<'a>(_doc: &mut genpdfi::Document, _node: &'a AstNode<'a>) {}

/// Truncate a line to max_chars, adding ellipsis if truncated. UTF-8 safe.
fn truncate_line(line: &str, max_chars: usize) -> String {
    let char_count = line.chars().count();
    if char_count <= max_chars {
        return line.to_string();
    }
    // Find byte boundary at max_chars characters
    if let Some((idx, _)) = line.char_indices().nth(max_chars) {
        format!("{}\u{2026}", &line[..idx])
    } else {
        line.to_string()
    }
}

/// Collect all plain text from a node tree.
fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut s = String::new();
    for child in node.descendants() {
        let data = child.data.borrow();
        if let NodeValue::Text(t) = &data.value {
            s.push_str(t);
        } else if let NodeValue::Code(c) = &data.value {
            s.push_str(&c.literal);
        } else if matches!(&data.value, NodeValue::SoftBreak) {
            s.push(' ');
        }
    }
    s
}

fn render_list<'a>(
    doc: &mut genpdfi::Document,
    node: &'a AstNode<'a>,
    list_type: ListType,
    start: usize,
    temp_files: &mut Vec<PathBuf>,
    mono_font: genpdfi::fonts::FontFamily<genpdfi::fonts::Font>,
    first_h1_seen: &mut bool,
) {
    let body_style = style::Style::new()
        .with_font_size(11)
        .with_color(style::Color::Rgb(30, 30, 30));

    for (i, item) in node.children().enumerate() {
        // Check if this is a task list item
        let task_checked = {
            let item_data = item.data.borrow();
            if let NodeValue::TaskItem(task) = &item_data.value {
                Some(task.symbol.is_some())
            } else {
                None
            }
        };

        let bullet = if let Some(checked) = task_checked {
            if checked {
                "\u{2611}  ".to_string() // ☑
            } else {
                "\u{2610}  ".to_string() // ☐
            }
        } else {
            match list_type {
                ListType::Bullet => "\u{2022}  ".to_string(),
                ListType::Ordered => format!("{}.  ", start + i),
            }
        };

        let mut first_para = true;
        for item_child in item.children() {
            let cd = item_child.data.borrow();
            match &cd.value {
                NodeValue::Paragraph => {
                    drop(cd);
                    let mut p = elements::Paragraph::default();
                    if first_para {
                        p.push_styled(&bullet, body_style.bold());
                        first_para = false;
                    } else {
                        // Continuation paragraph — indent to align
                        p.push_styled("    ", body_style);
                    }
                    collect_inline(&mut p, item_child, body_style, mono_font);
                    doc.push(p);
                    doc.push(elements::Break::new(0.15));
                }
                NodeValue::List(sub_list) => {
                    let lt = sub_list.list_type;
                    let st = sub_list.start;
                    drop(cd);
                    // Nested list with indentation
                    doc.push(elements::PaddedElement::new(
                        elements::Break::new(0.0),
                        genpdfi::Margins::trbl(0, 0, 0, 6),
                    ));
                    render_list(
                        doc,
                        item_child,
                        lt,
                        st,
                        temp_files,
                        mono_font,
                        first_h1_seen,
                    );
                }
                _ => {
                    drop(cd);
                    render_block(doc, item_child, temp_files, mono_font, first_h1_seen);
                }
            }
        }
    }
}

fn render_table<'a>(
    doc: &mut genpdfi::Document,
    node: &'a AstNode<'a>,
    mono_font: genpdfi::fonts::FontFamily<genpdfi::fonts::Font>,
) {
    // Count columns from first row
    let num_cols = node
        .children()
        .next()
        .map(|r| r.children().count())
        .unwrap_or(0);
    if num_cols == 0 {
        return;
    }

    let column_weights = vec![1; num_cols];
    let mut table = elements::TableLayout::new(column_weights);
    table.set_cell_decorator(elements::FrameCellDecorator::new(true, true, false));

    let mut is_header = true;
    for row_node in node.children() {
        let rd = row_node.data.borrow();
        if !matches!(&rd.value, NodeValue::TableRow(_)) {
            drop(rd);
            continue;
        }
        drop(rd);

        let cell_style = if is_header {
            style::Style::new()
                .with_font_size(10)
                .bold()
                .with_color(style::Color::Rgb(25, 25, 35))
        } else {
            style::Style::new()
                .with_font_size(10)
                .with_color(style::Color::Rgb(30, 30, 30))
        };

        let mut row = table.row();
        for cell_node in row_node.children() {
            let mut p = elements::Paragraph::default();
            collect_inline(&mut p, cell_node, cell_style, mono_font);
            if is_header {
                row.push_element(
                    FilledElement::new(
                        elements::PaddedElement::new(p, genpdfi::Margins::trbl(1, 1, 1, 1)),
                        style::Color::Rgb(240, 241, 245),
                        1,
                        1,
                    )
                    .with_corner_radius(0.0),
                );
            } else {
                row.push_element(elements::PaddedElement::new(
                    p,
                    genpdfi::Margins::trbl(1, 1, 1, 1),
                ));
            }
        }
        let _ = row.push();
        is_header = false;
    }

    doc.push(elements::Break::new(0.3));
    doc.push(table);
    doc.push(elements::Break::new(0.5));
}

// ─── Alert Block Rendering ───────────────────────────────────────────────────

fn render_alert_block<'a>(
    doc: &mut genpdfi::Document,
    node: &'a AstNode<'a>,
    alert_type: AlertType,
    temp_files: &mut Vec<PathBuf>,
    mono_font: genpdfi::fonts::FontFamily<genpdfi::fonts::Font>,
    first_h1_seen: &mut bool,
) {
    let (label, color) = match alert_type {
        AlertType::Note => ("Note", style::Color::Rgb(9, 105, 218)),
        AlertType::Tip => ("Tip", style::Color::Rgb(26, 127, 55)),
        AlertType::Important => ("Important", style::Color::Rgb(130, 80, 223)),
        AlertType::Warning => ("Warning", style::Color::Rgb(191, 135, 0)),
        AlertType::Caution => ("Caution", style::Color::Rgb(207, 34, 46)),
    };

    doc.push(elements::Break::new(0.3));

    // Bold colored label
    let mut label_p = elements::Paragraph::default();
    label_p.push_styled(
        format!("  \u{2502} {}", label),
        style::Style::new()
            .with_font_size(11)
            .bold()
            .with_color(color),
    );
    doc.push(label_p);
    doc.push(elements::Break::new(0.2));

    // Render children with blockquote-style prefix
    for child in node.children() {
        let cd = child.data.borrow();
        if matches!(&cd.value, NodeValue::Paragraph) {
            drop(cd);
            let mut p = elements::Paragraph::default();
            let qs = style::Style::new()
                .with_font_size(11)
                .with_color(style::Color::Rgb(55, 55, 65));
            p.push_styled("  \u{2502} ", style::Style::new().with_color(color));
            collect_inline(&mut p, child, qs, mono_font);
            doc.push(p);
            doc.push(elements::Break::new(0.2));
        } else {
            drop(cd);
            render_block(doc, child, temp_files, mono_font, first_h1_seen);
        }
    }

    doc.push(elements::Break::new(0.3));
}

// ─── Footnote Rendering ─────────────────────────────────────────────────────

fn render_footnotes<'a>(
    doc: &mut genpdfi::Document,
    root: &'a AstNode<'a>,
    mono_font: genpdfi::fonts::FontFamily<genpdfi::fonts::Font>,
) {
    let _ = mono_font; // available if needed in future
    let mut footnotes: Vec<(String, String)> = Vec::new();

    for node in root.descendants() {
        let data = node.data.borrow();
        if let NodeValue::FootnoteDefinition(fd) = &data.value {
            let name = fd.name.clone();
            drop(data);
            let text = collect_text(node);
            footnotes.push((name, text));
        }
    }

    if footnotes.is_empty() {
        return;
    }

    // Separator
    doc.push(elements::Break::new(1.5));
    let mut sep = elements::Paragraph::default();
    sep.push_styled(
        "\u{2500}".repeat(40),
        style::Style::new().with_color(style::Color::Rgb(200, 200, 205)),
    );
    doc.push(sep);
    doc.push(elements::Break::new(0.5));

    // Footnote entries
    let fn_style = style::Style::new()
        .with_font_size(9)
        .with_color(style::Color::Rgb(80, 80, 90));

    for (name, text) in &footnotes {
        let mut p = elements::Paragraph::default();
        p.push_styled(format!("[{}] ", name), fn_style.bold());
        p.push_styled(text, fn_style);
        doc.push(p);
        doc.push(elements::Break::new(0.2));
    }
}

// ─── Mermaid Diagram Rendering ───────────────────────────────────────────────

/// Render a mermaid code block as a PNG image for PDF embedding.
/// Returns the genpdfi Image element and the temp file path for cleanup.
fn render_mermaid_to_image(code: &str, index: usize) -> Option<(elements::Image, PathBuf)> {
    let png_bytes = render_mermaid_png(code)?;

    let temp_dir = std::env::temp_dir().join("md-mermaid-export");
    let _ = std::fs::create_dir_all(&temp_dir);

    // Convert PNG (RGBA) to JPEG (RGB) — genpdfi doesn't support alpha channel
    let jpg_path = temp_dir.join(format!("diagram_{}.jpg", index));
    if convert_png_to_jpeg(&png_bytes, &jpg_path).is_err() {
        return None;
    }

    elements::Image::from_path(&jpg_path).ok().map(|img| {
        let img = scale_image_to_fit(img, &jpg_path);
        (img.with_alignment(genpdfi::Alignment::Center), jpg_path)
    })
}

/// Scale a genpdfi Image element to fit within page content margins.
/// Uses pixel dimensions from the file, converts to mm at 300 DPI,
/// then applies uniform downscale if needed (never upscales).
#[cfg(feature = "images")]
fn scale_image_to_fit(img: elements::Image, path: &std::path::Path) -> elements::Image {
    if let Ok((px_w, px_h)) = image::image_dimensions(path) {
        if px_w == 0 || px_h == 0 {
            return img;
        }
        let w_mm = 25.4 * px_w as f32 / 300.0;
        let h_mm = 25.4 * px_h as f32 / 300.0;
        let scale = (1.0_f32)
            .min(CONTENT_WIDTH_MM / w_mm)
            .min(MAX_IMAGE_HEIGHT_MM / h_mm);
        if scale < 1.0 {
            return img.with_scale(Scale::new(scale, scale));
        }
    }
    img
}

#[cfg(not(feature = "images"))]
fn scale_image_to_fit(img: elements::Image, _path: &std::path::Path) -> elements::Image {
    img
}

/// Convert PNG bytes (may have alpha) to JPEG file (no alpha).
/// Composites transparent pixels onto a white background.
#[cfg(feature = "images")]
fn convert_png_to_jpeg(
    png_bytes: &[u8],
    output: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let img = image::load_from_memory(png_bytes)?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = image::RgbImage::new(w, h);
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let a = pixel[3] as f32 / 255.0;
        // Alpha composite onto white background
        let r = (pixel[0] as f32 * a + 255.0 * (1.0 - a)) as u8;
        let g = (pixel[1] as f32 * a + 255.0 * (1.0 - a)) as u8;
        let b = (pixel[2] as f32 * a + 255.0 * (1.0 - a)) as u8;
        rgb.put_pixel(x, y, image::Rgb([r, g, b]));
    }
    rgb.save(output)?;
    Ok(())
}

#[cfg(not(feature = "images"))]
fn convert_png_to_jpeg(
    _png_bytes: &[u8],
    _output: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("Image conversion requires the 'images' feature".into())
}

/// Try to render mermaid to PNG. Tries mmdc CLI first, then kroki.io API.
fn render_mermaid_png(code: &str) -> Option<Vec<u8>> {
    if let Some(data) = render_mermaid_mmdc(code) {
        return Some(data);
    }

    #[cfg(feature = "url")]
    if let Some(data) = render_mermaid_kroki(code) {
        return Some(data);
    }

    None
}

/// Render mermaid to PNG using the mmdc CLI (mermaid-cli, works offline).
fn render_mermaid_mmdc(code: &str) -> Option<Vec<u8>> {
    let temp_dir = std::env::temp_dir().join("md-mermaid-export");
    let _ = std::fs::create_dir_all(&temp_dir);
    let input_path = temp_dir.join("mmdc_input.mmd");
    let output_path = temp_dir.join("mmdc_output.png");

    // Prepend neutral theme for clean, soft diagram style
    let themed_code = format!("%%{{init: {{\"theme\": \"neutral\"}}}}%%\n{}", code);
    std::fs::write(&input_path, themed_code).ok()?;

    let status = std::process::Command::new("mmdc")
        .args([
            "-i",
            &input_path.to_string_lossy(),
            "-o",
            &output_path.to_string_lossy(),
            "-b",
            "white",
            "-s",
            "2",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    let _ = std::fs::remove_file(&input_path);

    if status.success() {
        let data = std::fs::read(&output_path).ok()?;
        let _ = std::fs::remove_file(&output_path);
        Some(data)
    } else {
        None
    }
}

/// Render mermaid to PNG using the kroki.io web API (no browser needed).
#[cfg(feature = "url")]
fn render_mermaid_kroki(code: &str) -> Option<Vec<u8>> {
    // Use neutral theme for clean, soft diagram style
    let themed_code = format!("%%{{init: {{\"theme\": \"neutral\"}}}}%%\n{}", code);
    let resp = ureq::post("https://kroki.io/mermaid/png")
        .header("Content-Type", "text/plain")
        .send(&themed_code)
        .ok()?;

    let body = resp.into_body().read_to_vec().ok()?;

    if body.len() > 100 { Some(body) } else { None }
}

// ─── Other export formats ────────────────────────────────────────────────────

fn read_input(file: &Option<String>) -> Result<String, Box<dyn std::error::Error>> {
    match file {
        Some(path) => Ok(std::fs::read_to_string(path)
            .map_err(|e| format!("Error reading '{}': {}", path, e))?),
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
    }
}

fn ast_to_json<'a>(node: &'a AstNode<'a>, depth: usize) -> String {
    let data = node.data.borrow();
    let indent = "  ".repeat(depth);
    let indent1 = "  ".repeat(depth + 1);

    let node_type = match &data.value {
        NodeValue::Document => "document",
        NodeValue::Heading(_) => "heading",
        NodeValue::Paragraph => "paragraph",
        NodeValue::Text(_) => "text",
        NodeValue::Code(_) => "code",
        NodeValue::CodeBlock(_) => "code_block",
        NodeValue::Link(_) => "link",
        NodeValue::Image(_) => "image",
        NodeValue::List(_) => "list",
        NodeValue::Item(_) => "item",
        NodeValue::BlockQuote => "blockquote",
        NodeValue::ThematicBreak => "thematic_break",
        NodeValue::Table(_) => "table",
        NodeValue::TableRow(_) => "table_row",
        NodeValue::TableCell => "table_cell",
        NodeValue::Emph => "emphasis",
        NodeValue::Strong => "strong",
        NodeValue::Strikethrough => "strikethrough",
        NodeValue::SoftBreak => "softbreak",
        NodeValue::LineBreak => "linebreak",
        NodeValue::HtmlBlock(_) => "html_block",
        NodeValue::HtmlInline(_) => "html_inline",
        NodeValue::FrontMatter(_) => "front_matter",
        NodeValue::FootnoteDefinition(_) => "footnote_definition",
        NodeValue::FootnoteReference(_) => "footnote_reference",
        NodeValue::Math(_) => "math",
        NodeValue::Alert(_) => "alert",
        _ => "other",
    };

    let mut props = Vec::new();

    match &data.value {
        NodeValue::Text(t) => props.push(format!("{}\"value\": {}", indent1, json_escape(t))),
        NodeValue::Code(c) => {
            props.push(format!("{}\"value\": {}", indent1, json_escape(&c.literal)))
        }
        NodeValue::CodeBlock(cb) => {
            props.push(format!("{}\"info\": {}", indent1, json_escape(&cb.info)));
            props.push(format!(
                "{}\"literal\": {}",
                indent1,
                json_escape(&cb.literal)
            ));
        }
        NodeValue::Heading(h) => {
            props.push(format!("{}\"level\": {}", indent1, h.level));
        }
        NodeValue::Link(link) => {
            props.push(format!("{}\"url\": {}", indent1, json_escape(&link.url)));
            props.push(format!(
                "{}\"title\": {}",
                indent1,
                json_escape(&link.title)
            ));
        }
        NodeValue::Image(img) => {
            props.push(format!("{}\"url\": {}", indent1, json_escape(&img.url)));
            props.push(format!("{}\"title\": {}", indent1, json_escape(&img.title)));
        }
        NodeValue::FrontMatter(fm) => {
            props.push(format!("{}\"value\": {}", indent1, json_escape(fm)));
        }
        _ => {}
    }

    let children: Vec<String> = node
        .children()
        .map(|child| ast_to_json(child, depth + 2))
        .collect();

    let mut parts = vec![format!("{}\"type\": \"{}\"", indent1, node_type)];
    parts.extend(props);

    if !children.is_empty() {
        parts.push(format!(
            "{}\"children\": [\n{}\n{}]",
            indent1,
            children.join(",\n"),
            indent1
        ));
    }

    format!("{}{{\n{}\n{}}}", indent, parts.join(",\n"), indent)
}

fn json_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 2);
    result.push('"');
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c < '\x20' => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

fn extract_plain_text<'a>(root: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    let mut last_was_block = false;

    for node in root.descendants() {
        let data = node.data.borrow();
        match &data.value {
            NodeValue::Text(t) => {
                text.push_str(t);
                last_was_block = false;
            }
            NodeValue::Code(c) => {
                text.push_str(&c.literal);
                last_was_block = false;
            }
            NodeValue::CodeBlock(cb) => {
                if last_was_block {
                    text.push('\n');
                }
                text.push_str(&cb.literal);
                last_was_block = true;
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => {
                text.push('\n');
                last_was_block = false;
            }
            NodeValue::Paragraph => {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push_str("\n\n");
                }
                last_was_block = true;
            }
            NodeValue::Heading(_) => {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push_str("\n\n");
                }
                last_was_block = true;
            }
            _ => {}
        }
    }

    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{}\n", trimmed)
    }
}
