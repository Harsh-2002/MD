use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "md",
    version,
    about = "Render markdown beautifully in the terminal",
    subcommand_help_heading = "Commands",
    after_help = "Examples:\n  md README.md                          Render in terminal\n  md serve .                            Live preview in browser\n  md stats README.md                    Show word count & stats\n  md fmt --check README.md              Check formatting\n  md export --to html README.md         Export to HTML\n  md export --to pdf README.md          Export to PDF\n  md publish ./blog --out ./dist        Generate static site"
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Markdown file to render (reads stdin if omitted)
    pub file: Option<String>,

    /// Output width in columns (default: terminal width)
    #[arg(short, long)]
    pub width: Option<u16>,

    /// Pipe output through less -R
    #[arg(short, long)]
    pub pager: bool,

    /// Export to HTML or PDF file
    #[arg(short, long)]
    pub output: Option<String>,

    /// Color mode: auto, always, never
    #[arg(long, default_value = "auto")]
    pub color: ColorMode,

    /// Theme: dark (default), light
    #[arg(long, default_value = "dark")]
    pub theme: ThemeName,

    /// Plain text output (no ANSI, no box-drawing, no fancy bullets)
    #[arg(long)]
    pub plain: bool,

    /// Syntax highlighting theme for code blocks
    #[arg(long, default_value = "base16-ocean.dark")]
    pub syntax_theme: String,

    /// List available syntax highlighting themes and exit
    #[arg(long)]
    pub list_syntax_themes: bool,

    /// Generate shell completions for the given shell and exit
    #[arg(long, value_name = "SHELL")]
    pub completions: Option<clap_complete::Shell>,

    /// Custom CSS file to inject (serve and export modes)
    #[arg(long, value_name = "FILE")]
    pub css: Option<String>,

    /// Generate man page and exit
    #[arg(long)]
    pub generate_man: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Preview markdown in the browser with live reload
    #[cfg(feature = "serve")]
    Serve(ServeArgs),

    /// Watch a file and re-render on changes
    #[cfg(feature = "watch")]
    Watch(WatchArgs),

    /// Present markdown as slides in the terminal
    #[cfg(feature = "watch")]
    Present(PresentArgs),

    /// Generate a table of contents
    Toc(TocArgs),

    /// Show document statistics (words, lines, headings, etc.)
    Stats(StatsArgs),

    /// Format/prettify markdown
    Fmt(FmtArgs),

    /// Check markdown for common issues
    Lint(LintArgs),

    /// Compare two markdown files with colored diff
    Diff(DiffArgs),

    /// Export markdown to another format (html, pdf, json, txt)
    Export(ExportArgs),

    /// Generate a static site from a directory of markdown files
    Publish(PublishArgs),

    /// Update md to the latest version
    #[cfg(feature = "url")]
    Update,
}

#[cfg(feature = "serve")]
#[derive(clap::Args, Debug)]
pub struct ServeArgs {
    /// Markdown file(s) or directory to preview (reads stdin if omitted or "-")
    pub files: Vec<String>,

    /// Port (default: random available port)
    #[arg(short, long)]
    pub port: Option<u16>,

    /// Custom CSS file to inject
    #[arg(long, value_name = "FILE")]
    pub css: Option<String>,
}

#[cfg(feature = "watch")]
#[derive(clap::Args, Debug)]
pub struct WatchArgs {
    /// Markdown file to watch
    pub file: String,
}

#[derive(clap::Args, Debug)]
pub struct TocArgs {
    /// Markdown file to generate TOC from
    pub file: String,

    /// Maximum heading depth to include (1-6)
    #[arg(long, default_value = "3")]
    pub depth: u8,
}

#[cfg(feature = "watch")]
#[derive(clap::Args, Debug)]
pub struct PresentArgs {
    /// Markdown file to present
    pub file: String,
}

#[derive(clap::Args, Debug)]
pub struct StatsArgs {
    /// Markdown file (reads stdin if omitted)
    pub file: Option<String>,
}

#[derive(clap::Args, Debug)]
#[command(
    after_help = "Examples:\n  md fmt README.md                Print formatted to stdout\n  md fmt --in-place README.md     Format file in place\n  md fmt --check README.md        Exit 1 if not formatted (CI)"
)]
pub struct FmtArgs {
    /// Markdown file (reads stdin if omitted)
    pub file: Option<String>,

    /// Format file in place (overwrites the file)
    #[arg(short, long)]
    pub in_place: bool,

    /// Check if file is formatted (exit 1 if not, for CI)
    #[arg(short, long)]
    pub check: bool,
}

#[derive(clap::Args, Debug)]
pub struct LintArgs {
    /// Markdown file to lint
    pub file: String,
}

#[derive(clap::Args, Debug)]
#[command(
    after_help = "Examples:\n  md diff old.md new.md           Side-by-side diff\n  md diff -u old.md new.md        Unified diff\n  md diff - new.md                Read old from stdin"
)]
pub struct DiffArgs {
    /// First file (use "-" for stdin)
    pub file_a: String,

    /// Second file
    pub file_b: String,

    /// Show unified diff instead of side-by-side
    #[arg(long, short)]
    pub unified: bool,
}

#[derive(clap::Args, Debug)]
#[command(
    after_help = "Examples:\n  md export --to html README.md          Standalone HTML page\n  md export --to pdf README.md           PDF document\n  md export --to pdf -o out.pdf file.md  PDF with custom output path\n  md export --to json README.md          AST as JSON\n  md export --to txt README.md           Plain text (strip formatting)"
)]
pub struct ExportArgs {
    /// Markdown file (reads stdin if omitted)
    pub file: Option<String>,

    /// Output format
    #[arg(long, value_parser = ["html", "json", "txt", "pdf"])]
    pub to: String,

    /// Output file path (for pdf: defaults to input filename with .pdf extension)
    #[arg(short, long)]
    pub output: Option<String>,
}

#[derive(clap::Args, Debug)]
#[command(
    after_help = "Front matter (optional):\n  ---\n  title: My Post\n  date: 2024-01-15\n  tags: rust, cli\n  draft: true\n  ---"
)]
pub struct PublishArgs {
    /// Directory containing markdown files
    pub dir: String,

    /// Output directory for the generated site
    #[arg(long, short, default_value = "dist")]
    pub out: String,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ThemeName {
    Dark,
    Light,
}
