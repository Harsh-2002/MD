# mdx

A fast terminal markdown renderer and toolchain built in Rust. Renders markdown with syntax highlighting, tables, math, mermaid diagrams, and images — directly in your terminal. Also includes a browser preview with live reload, a markdown formatter, linter, diff viewer, format converter, and static site generator.

## Install

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/Harsh-2002/MD/main/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/Harsh-2002/MD/main/install.ps1 | iex
```

Installs the binary and sets up shell completions automatically.

**From source (all platforms):**

```bash
cargo install --git https://github.com/Harsh-2002/MD --features serve
```

## Usage

```bash
mdx file.md                          # render in terminal
mdx file.md --pager                  # render and pipe through less
mdx serve file.md                    # browser preview with live reload
mdx serve ./notes/                   # serve a directory as a note-taking app
mdx serve a.md b.md                  # serve multiple files with index page
mdx stats file.md                    # show word count, headings, reading time
mdx fmt file.md                      # format/prettify markdown
mdx fmt --check file.md              # check formatting (for CI)
mdx lint file.md                     # check for broken links, issues
mdx diff old.md new.md               # colored side-by-side diff
mdx diff -u old.md new.md            # unified diff
mdx export --to html file.md         # export to standalone HTML
mdx export --to pdf file.md          # export to PDF (no browser needed)
mdx export --to json file.md         # export AST as JSON
mdx export --to txt file.md          # strip formatting to plain text
mdx toc file.md                      # print table of contents
mdx present file.md                  # slide presentation in terminal
mdx watch file.md                    # re-render on file changes
mdx publish ./blog --out ./dist      # generate a static site
cat README.md | mdx                  # read from stdin
mdx https://example.com/doc.md       # render from URL
```

## Options

| Flag | Description |
|------|-------------|
| `-w, --width <N>` | Output width in columns |
| `-p, --pager` | Pipe through `less` |
| `-o, --output <FILE>` | Export to HTML or PDF |
| `--plain` | Plain text, no colors or box-drawing |
| `--theme dark\|light` | Color theme |
| `--syntax-theme <NAME>` | Syntax highlighting theme |
| `--list-syntax-themes` | List available syntax themes |
| `--css <FILE>` | Custom CSS for HTML/serve output |
| `--completions <SHELL>` | Generate shell completions |
| `--generate-man` | Generate man page |

## Serve Mode

`mdx serve` turns your markdown into a live browser preview. It supports three modes:

**Single file** — `mdx serve file.md` opens the file with live reload. Edit in your favorite editor and the browser updates instantly.

**Directory** — `mdx serve ./notes/` shows all `.md` files as a card grid. Click any card to view it. Click the "+" card to create a new note directly from the browser.

**Multiple files** — `mdx serve a.md b.md c.md` serves the listed files with a sidebar for navigation and a search filter.

All modes include:
- Built-in markdown editor (toggle with pencil icon or `e`)
- Search & replace in editor (`Ctrl+F` / `Ctrl+H`)
- Drag & drop image upload (or paste from clipboard)
- Print / PDF export button (uses browser's native print)
- Dark/light theme toggle
- Table of contents sidebar

| Key | Action |
|-----|--------|
| `e` | Toggle editor |
| `t` | Toggle dark/light theme |
| `[` | Toggle sidebar |
| `]` | Switch sidebar tab |
| `Ctrl+F` | Search in editor |
| `Ctrl+H` | Search & replace in editor |

## CLI Tools

### `mdx stats` — Document statistics

```
$ mdx stats README.md
     Words: 1,247
     Lines: 89
     Chars: 7,832
  Headings: 12
     Links: 8
    Images: 0
Code blocks: 3
  Reading time: ~5 min
```

### `mdx fmt` — Markdown formatter

Normalizes markdown formatting. Use `--check` in CI to ensure consistent style.

```bash
mdx fmt README.md                # print formatted to stdout
mdx fmt --in-place README.md     # overwrite the file
mdx fmt --check README.md        # exit 1 if not formatted
```

### `mdx lint` — Markdown linter

Checks for broken relative links, duplicate headings, missing image alt text, and trailing whitespace.

```
$ mdx lint README.md
  README.md:12 broken link: ./missing.md
  README.md:34 image missing alt text
  README.md:45 duplicate heading: "Setup"
  3 issues found
```

### `mdx diff` — Markdown diff

Colored side-by-side or unified diff of two markdown files.

```bash
mdx diff old.md new.md           # side-by-side
mdx diff -u old.md new.md        # unified
mdx diff - new.md                # read first file from stdin
```

### `mdx export` — Format conversion

```bash
mdx export --to html README.md   # standalone HTML page
mdx export --to pdf README.md    # PDF document (native, no browser needed)
mdx export --to json README.md   # AST as JSON (for tooling)
mdx export --to txt README.md    # plain text (strip formatting)
```

### `mdx publish` — Static site generator

Generates a static site from a directory of markdown files. Supports YAML front matter for metadata.

```bash
mdx publish ./blog --out ./dist
```

Front matter format:

```yaml
---
title: My Post Title
date: 2024-01-15
tags: rust, cli
draft: true
---
```

- `draft: true` files are skipped
- Missing `date` falls back to file modification time
- Missing `title` falls back to first heading or filename
- Generates clean URLs: `my-post.md` becomes `my-post/index.html`
- Shared CSS, blog index with cards, dark/light theme

## Features

- **Syntax highlighting** — language-aware code blocks via syntect
- **Tables** — full GFM table rendering with alignment and cell wrapping
- **Mermaid diagrams** — rendered as interactive SVG in browser via mermaid.js
- **Math** — inline `$...$` and display `$$...$$` blocks via KaTeX
- **Images** — inline image rendering in supported terminals (iTerm2, kitty)
- **URL fetching** — render markdown directly from URLs
- **Live reload** — `mdx serve` opens a browser preview that updates on file changes
- **Built-in editor** — toggle a markdown editor in the browser, saves back to disk
- **Search & replace** — find and replace text in the editor with regex support
- **Image upload** — drag & drop or paste images into the editor
- **Print / PDF** — browser-native print with clean print-optimized CSS
- **Directory mode** — `mdx serve ./dir/` shows a file index with card grid
- **Multi-file mode** — `mdx serve a.md b.md` with sidebar file navigation
- **Dark/light theme** — toggle with button or `t` key, persisted in localStorage
- **Slide presentation** — `mdx present` splits on `---` for terminal slides
- **ToC generation** — `mdx toc` extracts headings with depth control
- **Document stats** — word count, reading time, heading/link/image counts
- **Formatter** — normalize markdown style with `mdx fmt`
- **Linter** — check for broken links, duplicates, missing alt text
- **Diff viewer** — colored side-by-side or unified diff
- **Format export** — export to HTML, PDF, JSON AST, or plain text
- **Static site generator** — `mdx publish` builds a blog from markdown files
- **Alerts** — GitHub-style note/tip/warning/caution blocks
- **Footnotes, task lists, strikethrough, autolinks** — full GFM support

## How It Works

`mdx` parses markdown into an AST using [comrak](https://github.com/kivikakk/comrak) (a CommonMark + GFM parser), then walks the tree to produce styled terminal output using ANSI escape codes. Text wrapping respects terminal width. For browser preview, it generates HTML and serves it via a local HTTP server with SSE-based live reload.

```
markdown file
     |
     v
  comrak ──> AST
     |
     ├──> terminal renderer ──> ANSI output
     |
     ├──> HTML renderer ──> axum server ──> browser (live reload via SSE)
     |
     └──> CLI tools (stats, fmt, lint, diff, export, publish)
```

## Credits

Built on these libraries:

| Library | Purpose |
|---------|---------|
| [comrak](https://github.com/kivikakk/comrak) | Markdown parsing (CommonMark + GFM) |
| [syntect](https://github.com/trishume/syntect) | Syntax highlighting for code blocks |
| [clap](https://github.com/clap-rs/clap) | CLI argument parsing and completions |
| [axum](https://github.com/tokio-rs/axum) | HTTP server (serve mode) |
| [tokio](https://github.com/tokio-rs/tokio) | Async runtime |
| [notify](https://github.com/notify-rs/notify) | File system watcher (live reload) |
| [ratatui](https://github.com/ratatui/ratatui) | TUI framework (watch/present modes) |
| [crossterm](https://github.com/crossterm-rs/crossterm) | Terminal manipulation |
| [similar](https://github.com/mitsuhiko/similar) | Text diffing |
| [textwrap](https://github.com/mgeisler/textwrap) | Text wrapping |
| [image](https://github.com/image-rs/image) | Image decoding (PNG, JPEG, GIF, WebP) |
| [genpdfi](https://docs.rs/genpdfi) | PDF generation |
| [ureq](https://github.com/algesten/ureq) | HTTP client (URL fetching) |

## License

MIT
