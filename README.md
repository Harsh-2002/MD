# md

A fast terminal markdown renderer and toolchain built in Rust. Renders markdown with syntax highlighting, tables, math, mermaid diagrams, and images — directly in your terminal. Also includes a browser preview with live reload, a markdown formatter, linter, diff viewer, format converter, and static site generator.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/Harsh-2002/MD/main/install.sh | sh
```

Installs the binary to `/usr/local/bin` and sets up shell completions automatically (bash, zsh, fish).

**From source:**

```bash
cargo install --git https://github.com/Harsh-2002/MD --features serve
```

## Usage

```bash
md file.md                          # render in terminal
md file.md --pager                  # render and pipe through less
md serve file.md                    # browser preview with live reload
md serve ./notes/                   # serve a directory as a note-taking app
md serve a.md b.md                  # serve multiple files with index page
md stats file.md                    # show word count, headings, reading time
md fmt file.md                      # format/prettify markdown
md fmt --check file.md              # check formatting (for CI)
md lint file.md                     # check for broken links, issues
md diff old.md new.md               # colored side-by-side diff
md diff -u old.md new.md            # unified diff
md export --to html file.md         # export to standalone HTML
md export --to pdf file.md          # export to PDF (no browser needed)
md export --to json file.md         # export AST as JSON
md export --to txt file.md          # strip formatting to plain text
md toc file.md                      # print table of contents
md present file.md                  # slide presentation in terminal
md watch file.md                    # re-render on file changes
md publish ./blog --out ./dist      # generate a static site
cat README.md | md                  # read from stdin
md https://example.com/doc.md       # render from URL
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

`md serve` turns your markdown into a live browser preview. It supports three modes:

**Single file** — `md serve file.md` opens the file with live reload. Edit in your favorite editor and the browser updates instantly.

**Directory** — `md serve ./notes/` shows all `.md` files as a card grid. Click any card to view it. Click the "+" card to create a new note directly from the browser.

**Multiple files** — `md serve a.md b.md c.md` serves the listed files with a sidebar for navigation and a search filter.

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

### `md stats` — Document statistics

```
$ md stats README.md
     Words: 1,247
     Lines: 89
     Chars: 7,832
  Headings: 12
     Links: 8
    Images: 0
Code blocks: 3
  Reading time: ~5 min
```

### `md fmt` — Markdown formatter

Normalizes markdown formatting. Use `--check` in CI to ensure consistent style.

```bash
md fmt README.md                # print formatted to stdout
md fmt --in-place README.md     # overwrite the file
md fmt --check README.md        # exit 1 if not formatted
```

### `md lint` — Markdown linter

Checks for broken relative links, duplicate headings, missing image alt text, and trailing whitespace.

```
$ md lint README.md
  README.md:12 broken link: ./missing.md
  README.md:34 image missing alt text
  README.md:45 duplicate heading: "Setup"
  3 issues found
```

### `md diff` — Markdown diff

Colored side-by-side or unified diff of two markdown files.

```bash
md diff old.md new.md           # side-by-side
md diff -u old.md new.md        # unified
md diff - new.md                # read first file from stdin
```

### `md export` — Format conversion

```bash
md export --to html README.md   # standalone HTML page
md export --to pdf README.md    # PDF document (native, no browser needed)
md export --to json README.md   # AST as JSON (for tooling)
md export --to txt README.md    # plain text (strip formatting)
```

### `md publish` — Static site generator

Generates a static site from a directory of markdown files. Supports YAML front matter for metadata.

```bash
md publish ./blog --out ./dist
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
- **Live reload** — `md serve` opens a browser preview that updates on file changes
- **Built-in editor** — toggle a markdown editor in the browser, saves back to disk
- **Search & replace** — find and replace text in the editor with regex support
- **Image upload** — drag & drop or paste images into the editor
- **Print / PDF** — browser-native print with clean print-optimized CSS
- **Directory mode** — `md serve ./dir/` shows a file index with card grid
- **Multi-file mode** — `md serve a.md b.md` with sidebar file navigation
- **Dark/light theme** — toggle with button or `t` key, persisted in localStorage
- **Slide presentation** — `md present` splits on `---` for terminal slides
- **ToC generation** — `md toc` extracts headings with depth control
- **Document stats** — word count, reading time, heading/link/image counts
- **Formatter** — normalize markdown style with `md fmt`
- **Linter** — check for broken links, duplicates, missing alt text
- **Diff viewer** — colored side-by-side or unified diff
- **Format export** — export to HTML, PDF, JSON AST, or plain text
- **Static site generator** — `md publish` builds a blog from markdown files
- **Alerts** — GitHub-style note/tip/warning/caution blocks
- **Footnotes, task lists, strikethrough, autolinks** — full GFM support

## How It Works

`md` parses markdown into an AST using [comrak](https://github.com/kivikakk/comrak) (a CommonMark + GFM parser), then walks the tree to produce styled terminal output using ANSI escape codes. Text wrapping respects terminal width. For browser preview, it generates HTML and serves it via a local HTTP server with SSE-based live reload.

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
| [syntect](https://github.com/trishume/syntect) | Syntax highlighting |
| [similar](https://github.com/mitsuhiko/similar) | Text diffing |
| [clap](https://github.com/clap-rs/clap) | CLI argument parsing |
| [crossterm](https://github.com/crossterm-rs/crossterm) | Terminal manipulation |
| [ratatui](https://github.com/ratatui/ratatui) | TUI framework (watch/present modes) |
| [axum](https://github.com/tokio-rs/axum) | HTTP server (serve mode) |
| [notify](https://github.com/notify-rs/notify) | File system watcher |
| [textwrap](https://github.com/mgeisler/textwrap) | Text wrapping |
| [image](https://github.com/image-rs/image) | Image decoding |
| [markdown2pdf](https://github.com/theiskaa/mdPdf) | Native PDF generation |

## License

MIT
