use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::process::{Command, Stdio};

use clap::Parser;
use comrak::nodes::AstNode;

use md::cli::Args;
use md::parse::parse_markdown;
use md::render::{self, RenderContext};
use md::style::theme::Theme;
use md::terminal::TerminalInfo;

fn main() {
    // Clean up leftover mdx.old.exe from a previous self-update on Windows
    #[cfg(all(windows, feature = "url"))]
    md::update::cleanup_old_binary();

    let args = Args::parse();

    // Handle subcommands
    #[cfg(feature = "serve")]
    if let Some(md::cli::Command::Serve(ref serve_args)) = args.command {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(md::serve::start_server(serve_args))
            .unwrap_or_else(|e| {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            });
        return;
    }

    #[cfg(feature = "watch")]
    if let Some(md::cli::Command::Watch(ref watch_args)) = args.command {
        md::watch::watch(watch_args).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    if let Some(md::cli::Command::Toc(ref toc_args)) = args.command {
        md::toc::generate_toc(toc_args).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    #[cfg(feature = "watch")]
    if let Some(md::cli::Command::Present(ref present_args)) = args.command {
        md::present::present(present_args).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    if let Some(md::cli::Command::Stats(ref stats_args)) = args.command {
        let sa = md::stats::StatsArgs {
            file: stats_args.file.clone(),
        };
        md::stats::run(&sa).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    if let Some(md::cli::Command::Fmt(ref fmt_args)) = args.command {
        let fa = md::fmt::FmtArgs {
            file: fmt_args.file.clone(),
            in_place: fmt_args.in_place,
            check: fmt_args.check,
        };
        md::fmt::run(&fa).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    if let Some(md::cli::Command::Lint(ref lint_args)) = args.command {
        let la = md::lint::LintArgs {
            file: lint_args.file.clone(),
        };
        md::lint::run(&la).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    if let Some(md::cli::Command::Diff(ref diff_args)) = args.command {
        let da = md::diff::DiffArgs {
            file_a: diff_args.file_a.clone(),
            file_b: diff_args.file_b.clone(),
            unified: diff_args.unified,
        };
        md::diff::run(&da).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    if let Some(md::cli::Command::Export(ref export_args)) = args.command {
        let ea = md::export::ExportArgs {
            file: export_args.file.clone(),
            to: export_args.to.clone(),
            output: export_args.output.clone(),
        };
        md::export::run(&ea).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    #[cfg(feature = "url")]
    if let Some(md::cli::Command::Fetch(ref fetch_args)) = args.command {
        let markdown = md::fetch::run(fetch_args).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        // If saved to file, we're done
        if fetch_args.output.is_some() {
            return;
        }
        // If stdout is not a terminal (piped), output raw markdown
        if !std::io::stdout().is_terminal() {
            let stdout = io::stdout();
            let mut writer = stdout.lock();
            let _ = writer.write_all(markdown.as_bytes());
            if !markdown.ends_with('\n') {
                let _ = writer.write_all(b"\n");
            }
            return;
        }
        // Render in terminal — same as md <file>
        let mut term = TerminalInfo::detect(&args.color, args.width);
        if args.plain {
            term.color_level = md::terminal::ColorLevel::None;
            term.unicode = false;
        }
        let theme = Theme::from_name(&args.theme);
        let arena = typed_arena::Arena::new();
        let root = parse_markdown(&arena, &markdown);
        if args.pager {
            render_with_pager(root, &term, &theme, &args.syntax_theme, args.plain, None);
        } else {
            let stdout = io::stdout();
            let mut writer = io::BufWriter::new(stdout.lock());
            let mut ctx = RenderContext::new(&term, &theme, args.syntax_theme.clone(), args.plain);
            if let Err(e) = render::render(&mut writer, root, &mut ctx)
                && e.kind() != io::ErrorKind::BrokenPipe
            {
                eprintln!("Error rendering: {}", e);
                std::process::exit(1);
            }
            let _ = writer.flush();
        }
        return;
    }

    #[cfg(feature = "url")]
    if let Some(md::cli::Command::Update) = args.command {
        md::update::run().unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    if let Some(md::cli::Command::Completions(ref comp_args)) = args.command {
        match comp_args.shell_or_action.as_str() {
            "install" => {
                md::completions::install().unwrap_or_else(|e| {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                });
            }
            shell => md::completions::generate(shell),
        }
        return;
    }

    if let Some(md::cli::Command::Publish(ref publish_args)) = args.command {
        let pa = md::publish::PublishArgs {
            dir: publish_args.dir.clone(),
            out: publish_args.out.clone(),
        };
        md::publish::run(&pa).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });
        return;
    }

    #[cfg(not(any(feature = "serve", feature = "watch")))]
    if args.command.is_some() {
        eprintln!("Subcommand not available. Rebuild with appropriate features:");
        eprintln!("  cargo install mdx --features serve");
        eprintln!("  cargo install mdx --features watch");
        std::process::exit(1);
    }

    // Handle --generate-man
    if args.generate_man {
        let cmd = <Args as clap::CommandFactory>::command();
        let man = clap_mangen::Man::new(cmd);
        man.render(&mut io::stdout()).unwrap_or_else(|e| {
            eprintln!("Error generating man page: {}", e);
            std::process::exit(1);
        });
        return;
    }

    // Handle --list-syntax-themes
    if args.list_syntax_themes {
        let ts = syntect::highlighting::ThemeSet::load_defaults();
        let mut names: Vec<&str> = ts.themes.keys().map(|s| s.as_str()).collect();
        names.sort();
        for name in names {
            println!("{}", name);
        }
        return;
    }

    // Read input from file, URL, or stdin
    let input = match &args.file {
        Some(path) if path.starts_with("http://") || path.starts_with("https://") => {
            #[cfg(feature = "url")]
            {
                eprintln!("  Fetching {}...", path);
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .timeout_global(Some(std::time::Duration::from_secs(30)))
                    .build()
                    .into();
                match agent.get(path).call() {
                    Ok(resp) => resp.into_body().read_to_string().unwrap_or_else(|e| {
                        eprintln!("Error reading response: {}", e);
                        std::process::exit(1);
                    }),
                    Err(e) => {
                        eprintln!("Error fetching URL: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "url"))]
            {
                eprintln!(
                    "URL fetching not available. Rebuild with: cargo install mdx --features url"
                );
                std::process::exit(1);
            }
        }
        Some(path) => match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error reading file '{}': {}", path, e);
                std::process::exit(1);
            }
        },
        None => {
            // Check if stdin is a terminal (no input piped)
            if std::io::stdin().is_terminal() {
                eprintln!("Usage: mdx [FILE]");
                eprintln!("  Reads from stdin if no file is given.");
                std::process::exit(1);
            }
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
                eprintln!("Error reading stdin: {}", e);
                std::process::exit(1);
            });
            buf
        }
    };

    // Detect terminal capabilities
    let mut term = TerminalInfo::detect(&args.color, args.width);
    if args.plain {
        term.color_level = md::terminal::ColorLevel::None;
        term.unicode = false;
    }

    // Set up theme
    let theme = Theme::from_name(&args.theme);

    // Parse markdown
    let arena = typed_arena::Arena::new();
    let root = parse_markdown(&arena, &input);

    // Determine image base directory from file path
    let image_base_dir = args.file.as_ref().and_then(|f| {
        if f.starts_with("http://") || f.starts_with("https://") {
            None
        } else {
            std::path::Path::new(f).parent().map(|p| p.to_path_buf())
        }
    });

    // Render
    if args.pager {
        render_with_pager(
            root,
            &term,
            &theme,
            &args.syntax_theme,
            args.plain,
            image_base_dir.clone(),
        );
    } else {
        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        let mut ctx = RenderContext::new(&term, &theme, args.syntax_theme.clone(), args.plain);
        ctx.image_base_dir = image_base_dir;
        if let Err(e) = render::render(&mut writer, root, &mut ctx) {
            // Ignore broken pipe errors (e.g., piping to head)
            if e.kind() != io::ErrorKind::BrokenPipe {
                eprintln!("Error rendering: {}", e);
                std::process::exit(1);
            }
        }
        let _ = writer.flush();
    }
}

fn render_with_pager<'a>(
    root: &'a AstNode<'a>,
    term: &TerminalInfo,
    theme: &Theme,
    syntax_theme: &str,
    plain: bool,
    image_base_dir: Option<std::path::PathBuf>,
) {
    // Render to a buffer first
    let mut buf = Vec::new();
    let mut ctx = RenderContext::new(term, theme, syntax_theme.to_string(), plain);
    ctx.image_base_dir = image_base_dir;
    if let Err(e) = render::render(&mut buf, root, &mut ctx) {
        eprintln!("Error rendering: {}", e);
        std::process::exit(1);
    }

    // Respect $PAGER env var; default to less (Unix) or more (Windows)
    #[cfg(windows)]
    let default_pager = "more".to_string();
    #[cfg(not(windows))]
    let default_pager = "less".to_string();
    let pager = std::env::var("PAGER").unwrap_or(default_pager);
    let pager_name = std::path::Path::new(&pager)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let (cmd, default_args): (&str, Vec<&str>) = if pager_name == "less" {
        (&pager, vec!["-RFX"])
    } else {
        (&pager, vec![])
    };

    let mut child = match Command::new(cmd)
        .args(&default_args)
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            // If custom pager fails, fall back to stdout
            eprintln!("Could not start pager '{}', writing to stdout", cmd);
            let _ = io::stdout().write_all(&buf);
            return;
        }
    };

    if let Some(ref mut stdin) = child.stdin {
        let _ = stdin.write_all(&buf);
    }
    let _ = child.wait();
}
