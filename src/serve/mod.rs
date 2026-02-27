use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::cli::ServeArgs;
use crate::cli::ThemeName;
use crate::html;

const SYNTAX_THEME: &str = "base16-ocean.dark";

struct AppState {
    /// Single-file mode: one entry with key ""
    /// Multi-file mode: entries keyed by filename
    files: RwLock<HashMap<String, FileEntry>>,
    index_html: RwLock<Option<String>>,
    tx: broadcast::Sender<String>,
    custom_css: String,
    #[allow(dead_code)]
    multi: bool,
    #[allow(dead_code)]
    stdin_mode: bool,
    file_paths: HashMap<String, PathBuf>,
    dir_path: Option<PathBuf>,
    filenames: RwLock<Vec<String>>,
}

struct FileEntry {
    full_html: String,
    raw_html: String,
    markdown: String,
}

pub async fn start_server(args: &ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Spawn shutdown handler: on signal, print message and exit.
    // This runs independently of axum's graceful shutdown so that
    // long-lived SSE connections and watcher threads cannot prevent exit.
    spawn_shutdown_handler();

    // Determine mode: stdin, single file, directory, or multi-file
    let files_arg = &args.files;

    let is_stdin = files_arg.is_empty() || (files_arg.len() == 1 && files_arg[0] == "-");

    if is_stdin {
        return serve_stdin(args).await;
    }

    // Check if it's a directory
    if files_arg.len() == 1 {
        let p = std::path::Path::new(&files_arg[0]);
        if p.is_dir() {
            return serve_directory(args, p).await;
        }
        // Single file
        return serve_single_file(args, &files_arg[0]).await;
    }

    // Multiple files
    serve_multi_files(args).await
}

async fn serve_stdin(args: &ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    if std::io::stdin().is_terminal() {
        return Err("No input on stdin. Pipe markdown or specify a file.".into());
    }

    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;

    let theme = ThemeName::Dark;
    let custom_css = load_custom_css(args.css.as_deref());
    let full = html::render_standalone(&buf, SYNTAX_THEME, &theme, "stdin", &custom_css);
    let raw = html::render_fragment(&buf, SYNTAX_THEME);

    let (tx, _) = broadcast::channel::<String>(16);

    let mut files_map = HashMap::new();
    files_map.insert(
        String::new(),
        FileEntry {
            full_html: full,
            raw_html: raw,
            markdown: buf,
        },
    );

    let state = Arc::new(AppState {
        files: RwLock::new(files_map),
        index_html: RwLock::new(None),
        tx,
        custom_css,
        multi: false,
        stdin_mode: true,
        file_paths: HashMap::new(),
        dir_path: None,
        filenames: RwLock::new(vec![]),
    });

    let app = Router::new()
        .route("/", get(serve_page_single))
        .route("/raw", get(serve_raw_single))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port.unwrap_or(0)));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_port = listener.local_addr()?.port();
    let url = format!("http://0.0.0.0:{}", actual_port);

    eprintln!("  Serving from stdin at {} (no live reload)", url);
    eprintln!("  Press Ctrl+C to stop");

    let _ = open::that(&url);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn serve_single_file(args: &ServeArgs, file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file_path = PathBuf::from(file)
        .canonicalize()
        .map_err(|e| format!("Cannot open '{}': {}", file, e))?;
    let markdown = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Cannot read '{}': {}", file, e))?;

    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "preview".to_string());

    let theme = ThemeName::Dark;
    let custom_css = load_custom_css(args.css.as_deref());
    let full = html::render_page(&markdown, SYNTAX_THEME, &theme, &filename, &custom_css);
    let raw = html::render_fragment(&markdown, SYNTAX_THEME);

    let (tx, _) = broadcast::channel::<String>(16);

    let mut files_map = HashMap::new();
    files_map.insert(
        String::new(),
        FileEntry {
            full_html: full,
            raw_html: raw,
            markdown,
        },
    );

    let mut file_paths = HashMap::new();
    file_paths.insert(String::new(), file_path.clone());

    let state = Arc::new(AppState {
        files: RwLock::new(files_map),
        index_html: RwLock::new(None),
        tx,
        custom_css,
        multi: false,
        stdin_mode: false,
        file_paths,
        dir_path: None,
        filenames: RwLock::new(vec![]),
    });

    // File watcher
    {
        let state = state.clone();
        let path = file_path.clone();
        let fname = filename.clone();

        std::thread::spawn(move || {
            use notify::{RecursiveMode, Watcher};

            let (ntx, nrx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                if let Ok(event) = res
                    && event.kind.is_modify()
                {
                    let _ = ntx.send(());
                }
            })
            .expect("Failed to create file watcher");

            watcher
                .watch(&path, RecursiveMode::NonRecursive)
                .expect("Failed to watch file");

            let theme = ThemeName::Dark;
            let mut last = Instant::now();
            for _ in nrx {
                if last.elapsed() < Duration::from_millis(300) {
                    continue;
                }
                last = Instant::now();

                if let Ok(content) = std::fs::read_to_string(&path) {
                    let css = &state.custom_css;
                    let full = html::render_page(&content, SYNTAX_THEME, &theme, &fname, css);
                    let raw = html::render_fragment(&content, SYNTAX_THEME);
                    let mut files = state.files.write().unwrap();
                    files.insert(
                        String::new(),
                        FileEntry {
                            full_html: full,
                            raw_html: raw,
                            markdown: content,
                        },
                    );
                    let _ = state.tx.send("reload".to_string());
                }
            }
        });
    }

    let app = Router::new()
        .route("/", get(serve_page_single))
        .route("/raw", get(serve_raw_single))
        .route("/source", get(get_source_single).put(put_source_single))
        .route("/upload", post(upload_handler))
        .route("/events", get(sse_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port.unwrap_or(0)));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_port = listener.local_addr()?.port();
    let url = format!("http://0.0.0.0:{}", actual_port);

    eprintln!("  Serving {} at {}", filename, url);
    eprintln!("  Press Ctrl+C to stop");

    let _ = open::that(&url);

    axum::serve(listener, app).await?;

    Ok(())
}

fn scan_md_files(dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(ext) = path.extension()
                && (ext == "md" || ext == "markdown")
                && let Some(name) = path.file_name()
            {
                results.push((name.to_string_lossy().to_string(), path));
            }
        }
    }
    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}

async fn serve_directory(
    args: &ServeArgs,
    dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = dir.canonicalize()?;
    let md_files = scan_md_files(&dir);

    if md_files.is_empty() {
        return Err(format!("No .md files found in '{}'", dir.display()).into());
    }

    let theme = ThemeName::Dark;
    let custom_css = load_custom_css(args.css.as_deref());
    let filenames: Vec<String> = md_files.iter().map(|(n, _)| n.clone()).collect();
    let index = html::render_index_page(&filenames, &theme, true);

    let mut files_map = HashMap::new();
    let mut file_paths = HashMap::new();
    for (name, path) in &md_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            let full = html::render_page_multi(
                &content,
                SYNTAX_THEME,
                &theme,
                name,
                &filenames,
                name,
                &custom_css,
            );
            let raw = html::render_fragment(&content, SYNTAX_THEME);
            files_map.insert(
                name.clone(),
                FileEntry {
                    full_html: full,
                    raw_html: raw,
                    markdown: content,
                },
            );
            file_paths.insert(name.clone(), path.clone());
        }
    }

    let (tx, _) = broadcast::channel::<String>(16);

    let state = Arc::new(AppState {
        files: RwLock::new(files_map),
        index_html: RwLock::new(Some(index)),
        tx,
        custom_css,
        multi: true,
        stdin_mode: false,
        file_paths,
        dir_path: Some(dir.clone()),
        filenames: RwLock::new(filenames.clone()),
    });

    // Watch the directory
    {
        let state = state.clone();
        let dir = dir.clone();

        std::thread::spawn(move || {
            use notify::{RecursiveMode, Watcher};

            let (ntx, nrx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                if let Ok(event) = res
                    && event.kind.is_modify()
                {
                    for path in &event.paths {
                        if let Some(name) = path.file_name() {
                            let _ = ntx.send(name.to_string_lossy().to_string());
                        }
                    }
                }
            })
            .expect("Failed to create file watcher");

            watcher
                .watch(&dir, RecursiveMode::NonRecursive)
                .expect("Failed to watch directory");

            let theme = ThemeName::Dark;
            let mut last = Instant::now();
            for changed_file in nrx {
                if last.elapsed() < Duration::from_millis(300) {
                    continue;
                }
                last = Instant::now();

                let current_filenames = state.filenames.read().unwrap().clone();
                if !current_filenames.contains(&changed_file) {
                    continue;
                }

                let path = dir.join(&changed_file);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let css = &state.custom_css;
                    let full = html::render_page_multi(
                        &content,
                        SYNTAX_THEME,
                        &theme,
                        &changed_file,
                        &current_filenames,
                        &changed_file,
                        css,
                    );
                    let raw = html::render_fragment(&content, SYNTAX_THEME);
                    let mut files = state.files.write().unwrap();
                    files.insert(
                        changed_file.clone(),
                        FileEntry {
                            full_html: full,
                            raw_html: raw,
                            markdown: content,
                        },
                    );
                    let _ = state.tx.send(format!(r#"{{"file":"{}"}}"#, changed_file));
                }
            }
        });
    }

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/create", post(create_file))
        .route("/upload", post(upload_handler))
        .route("/events", get(sse_handler))
        .route("/{file}", get(serve_page_multi))
        .route("/{file}/raw", get(serve_raw_multi))
        .route(
            "/{file}/source",
            get(get_source_multi).put(put_source_multi),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port.unwrap_or(0)));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_port = listener.local_addr()?.port();
    let url = format!("http://0.0.0.0:{}", actual_port);

    eprintln!(
        "  Serving {} files from {} at {}",
        filenames.len(),
        dir.display(),
        url
    );
    eprintln!("  Press Ctrl+C to stop");

    let _ = open::that(&url);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn serve_multi_files(args: &ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let theme = ThemeName::Dark;
    let custom_css = load_custom_css(args.css.as_deref());

    let mut entries: Vec<(String, PathBuf, String)> = Vec::new();
    for file in &args.files {
        let file_path = PathBuf::from(file)
            .canonicalize()
            .map_err(|e| format!("Cannot open '{}': {}", file, e))?;
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("Cannot read '{}': {}", file, e))?;
        let name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| file.clone());
        entries.push((name, file_path, content));
    }

    let filenames: Vec<String> = entries.iter().map(|(n, _, _)| n.clone()).collect();
    let paths: Vec<PathBuf> = entries.iter().map(|(_, p, _)| p.clone()).collect();

    let mut files_map = HashMap::new();
    for (name, _, content) in &entries {
        let full = html::render_page_multi(
            content,
            SYNTAX_THEME,
            &theme,
            name,
            &filenames,
            name,
            &custom_css,
        );
        let raw = html::render_fragment(content, SYNTAX_THEME);
        files_map.insert(
            name.clone(),
            FileEntry {
                full_html: full,
                raw_html: raw,
                markdown: content.clone(),
            },
        );
    }

    let file_paths: HashMap<String, PathBuf> = entries
        .iter()
        .map(|(n, p, _)| (n.clone(), p.clone()))
        .collect();

    let index = html::render_index_page(&filenames, &theme, false);
    let (tx, _) = broadcast::channel::<String>(16);

    let state = Arc::new(AppState {
        files: RwLock::new(files_map),
        index_html: RwLock::new(Some(index)),
        tx,
        custom_css,
        multi: true,
        stdin_mode: false,
        file_paths,
        dir_path: None,
        filenames: RwLock::new(filenames.clone()),
    });

    // Watch each file
    {
        let state = state.clone();
        let paths = paths.clone();
        let filenames = filenames.clone();

        std::thread::spawn(move || {
            use notify::{RecursiveMode, Watcher};

            let (ntx, nrx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                if let Ok(event) = res
                    && event.kind.is_modify()
                {
                    for path in &event.paths {
                        if let Some(name) = path.file_name() {
                            let _ = ntx.send(name.to_string_lossy().to_string());
                        }
                    }
                }
            })
            .expect("Failed to create file watcher");

            for path in &paths {
                watcher
                    .watch(path, RecursiveMode::NonRecursive)
                    .expect("Failed to watch file");
            }

            let theme = ThemeName::Dark;
            let mut last = Instant::now();
            let name_to_path: HashMap<String, PathBuf> = filenames
                .iter()
                .cloned()
                .zip(paths.iter().cloned())
                .collect();

            for changed_file in nrx {
                if last.elapsed() < Duration::from_millis(300) {
                    continue;
                }
                last = Instant::now();

                if let Some(path) = name_to_path.get(&changed_file)
                    && let Ok(content) = std::fs::read_to_string(path)
                {
                    let css = &state.custom_css;
                    let full = html::render_page_multi(
                        &content,
                        SYNTAX_THEME,
                        &theme,
                        &changed_file,
                        &filenames,
                        &changed_file,
                        css,
                    );
                    let raw = html::render_fragment(&content, SYNTAX_THEME);
                    let mut files = state.files.write().unwrap();
                    files.insert(
                        changed_file.clone(),
                        FileEntry {
                            full_html: full,
                            raw_html: raw,
                            markdown: content,
                        },
                    );
                    let _ = state.tx.send(format!(r#"{{"file":"{}"}}"#, changed_file));
                }
            }
        });
    }

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/upload", post(upload_handler))
        .route("/events", get(sse_handler))
        .route("/{file}", get(serve_page_multi))
        .route("/{file}/raw", get(serve_raw_multi))
        .route(
            "/{file}/source",
            get(get_source_multi).put(put_source_multi),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port.unwrap_or(0)));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_port = listener.local_addr()?.port();
    let url = format!("http://0.0.0.0:{}", actual_port);

    eprintln!("  Serving {} files at {}", filenames.len(), url);
    eprintln!("  Press Ctrl+C to stop");

    let _ = open::that(&url);

    axum::serve(listener, app).await?;

    Ok(())
}

// --- Route handlers ---

async fn serve_page_single(State(state): State<Arc<AppState>>) -> Html<String> {
    let files = state.files.read().unwrap();
    Html(
        files
            .get("")
            .map(|f| f.full_html.clone())
            .unwrap_or_default(),
    )
}

async fn serve_raw_single(State(state): State<Arc<AppState>>) -> Html<String> {
    let files = state.files.read().unwrap();
    Html(
        files
            .get("")
            .map(|f| f.raw_html.clone())
            .unwrap_or_default(),
    )
}

async fn serve_index(State(state): State<Arc<AppState>>) -> Html<String> {
    let index = state.index_html.read().unwrap();
    Html(index.clone().unwrap_or_default())
}

async fn serve_page_multi(
    State(state): State<Arc<AppState>>,
    Path(file): Path<String>,
) -> Html<String> {
    let files = state.files.read().unwrap();
    Html(
        files
            .get(&file)
            .map(|f| f.full_html.clone())
            .unwrap_or_else(|| "Not found".to_string()),
    )
}

async fn serve_raw_multi(
    State(state): State<Arc<AppState>>,
    Path(file): Path<String>,
) -> Html<String> {
    let files = state.files.read().unwrap();
    Html(
        files
            .get(&file)
            .map(|f| f.raw_html.clone())
            .unwrap_or_default(),
    )
}

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx)
        .map(|r| Ok(Event::default().data(r.unwrap_or_else(|_| "reload".to_string()))));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Spawn a background task that exits the process on shutdown signal.
/// This bypasses axum's graceful shutdown (which blocks on active SSE
/// connections) and also kills any watcher threads that would otherwise
/// keep the process alive.
fn spawn_shutdown_handler() {
    tokio::spawn(async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};

            let mut sigint =
                signal(SignalKind::interrupt()).expect("Failed to install SIGINT handler");
            let mut sigterm =
                signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");

            tokio::select! {
                _ = sigint.recv() => {}
                _ = sigterm.recv() => {}
            }
        }

        #[cfg(windows)]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        }

        eprintln!("\n  Stopped.");
        std::process::exit(0);
    });
}

// --- Source (editor) handlers ---

async fn get_source_single(State(state): State<Arc<AppState>>) -> String {
    let files = state.files.read().unwrap();
    files
        .get("")
        .map(|f| f.markdown.clone())
        .unwrap_or_default()
}

async fn put_source_single(State(state): State<Arc<AppState>>, body: String) -> StatusCode {
    let path = match state.file_paths.get("") {
        Some(p) => p.clone(),
        None => return StatusCode::NOT_FOUND,
    };
    match atomic_write(&path, &body) {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn get_source_multi(State(state): State<Arc<AppState>>, Path(file): Path<String>) -> String {
    let files = state.files.read().unwrap();
    files
        .get(&file)
        .map(|f| f.markdown.clone())
        .unwrap_or_default()
}

async fn put_source_multi(
    State(state): State<Arc<AppState>>,
    Path(file): Path<String>,
    body: String,
) -> StatusCode {
    let path = match state
        .file_paths
        .get(&file)
        .cloned()
        .or_else(|| state.dir_path.as_ref().map(|d| d.join(&file)))
    {
        Some(p) => p,
        None => return StatusCode::NOT_FOUND,
    };
    match atomic_write(&path, &body) {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn create_file(State(state): State<Arc<AppState>>, body: String) -> (StatusCode, String) {
    let dir = match &state.dir_path {
        Some(d) => d,
        None => return (StatusCode::BAD_REQUEST, "Not in directory mode".to_string()),
    };

    let mut filename = body.trim().to_string();
    if filename.is_empty() {
        return (StatusCode::BAD_REQUEST, "Filename is empty".to_string());
    }
    if !filename.ends_with(".md") {
        filename.push_str(".md");
    }
    if filename.contains('/')
        || filename.contains('\\')
        || filename.contains("..")
        || filename.len() > 255
    {
        return (StatusCode::BAD_REQUEST, "Invalid filename".to_string());
    }

    // Check for duplicates
    {
        let names = state.filenames.read().unwrap();
        if names.contains(&filename) {
            return (StatusCode::CONFLICT, "File already exists".to_string());
        }
    }

    // Derive a title from the filename (strip .md)
    let title = filename.strip_suffix(".md").unwrap_or(&filename);
    let markdown = format!("# {title}\n");

    let file_path = dir.join(&filename);
    if let Err(e) = atomic_write(&file_path, &markdown) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create file: {e}"),
        );
    }

    // Update shared filenames list
    let updated_filenames = {
        let mut names = state.filenames.write().unwrap();
        names.push(filename.clone());
        names.sort();
        names.clone()
    };

    // Render the new file's page
    let theme = ThemeName::Dark;
    let full = html::render_page_multi(
        &markdown,
        SYNTAX_THEME,
        &theme,
        &filename,
        &updated_filenames,
        &filename,
        &state.custom_css,
    );
    let raw = html::render_fragment(&markdown, SYNTAX_THEME);

    // Insert into files map
    {
        let mut files = state.files.write().unwrap();
        files.insert(
            filename.clone(),
            FileEntry {
                full_html: full,
                raw_html: raw,
                markdown,
            },
        );
    }

    // Rebuild index page
    {
        let new_index = html::render_index_page(&updated_filenames, &theme, true);
        let mut index = state.index_html.write().unwrap();
        *index = Some(new_index);
    }

    (StatusCode::OK, filename)
}

async fn upload_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> (StatusCode, String) {
    // Determine the base directory for assets
    let base_dir = if let Some(ref dir) = state.dir_path {
        dir.clone()
    } else if let Some(path) = state.file_paths.values().next() {
        path.parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf()
    } else {
        return (
            StatusCode::BAD_REQUEST,
            r#"{"error":"No file context"}"#.to_string(),
        );
    };

    let assets_dir = base_dir.join("assets");
    if let Err(e) = std::fs::create_dir_all(&assets_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(r#"{{"error":"Failed to create assets dir: {}"}}"#, e),
        );
    }

    // Extract filename from Content-Disposition or Content-Type
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Handle multipart form data
    let (filename, file_data) = if content_type.starts_with("multipart/form-data") {
        // Simple multipart parser: find the file content between boundaries
        match parse_multipart(&body, content_type) {
            Some(result) => result,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"Failed to parse upload"}"#.to_string(),
                );
            }
        }
    } else {
        // Raw upload - determine extension from content type
        let ext = match content_type {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            _ => "png",
        };
        (format!("image.{}", ext), body.to_vec())
    };

    // Deduplicate filename
    let final_name = dedup_filename(&assets_dir, &filename);
    let file_path = assets_dir.join(&final_name);

    if let Err(e) = std::fs::write(&file_path, &file_data) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(r#"{{"error":"Failed to write file: {}"}}"#, e),
        );
    }

    let path = format!("assets/{}", final_name);
    (StatusCode::OK, format!(r#"{{"path":"{}"}}"#, path))
}

fn parse_multipart(body: &[u8], content_type: &str) -> Option<(String, Vec<u8>)> {
    // Extract boundary from content-type
    let boundary = content_type
        .split("boundary=")
        .nth(1)?
        .trim_matches('"')
        .to_string();

    let boundary_marker = format!("--{}", boundary);
    let body_str = String::from_utf8_lossy(body);

    // Find the filename in Content-Disposition header
    let filename = body_str
        .lines()
        .find(|line| line.contains("filename="))
        .and_then(|line| {
            line.split("filename=")
                .nth(1)
                .map(|s| s.trim_matches('"').trim_matches('\'').to_string())
        })
        .unwrap_or_else(|| "upload.png".to_string());

    // Find the file data: it's after the empty line following headers, before the next boundary
    let parts: Vec<&[u8]> = split_bytes(body, boundary_marker.as_bytes());
    for part in parts.iter().skip(1) {
        // skip preamble
        // Find \r\n\r\n or \n\n (end of headers)
        if let Some(header_end) = find_double_newline(part) {
            let data_start = header_end;
            let mut data = &part[data_start..];
            // Trim trailing \r\n before boundary
            if data.ends_with(b"\r\n") {
                data = &data[..data.len() - 2];
            } else if data.ends_with(b"\n") {
                data = &data[..data.len() - 1];
            }
            if !data.is_empty() {
                return Some((filename, data.to_vec()));
            }
        }
    }

    None
}

fn split_bytes<'a>(haystack: &'a [u8], needle: &[u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    let nlen = needle.len();

    while start <= haystack.len() {
        if let Some(pos) = haystack[start..].windows(nlen).position(|w| w == needle) {
            parts.push(&haystack[start..start + pos]);
            start = start + pos + nlen;
        } else {
            parts.push(&haystack[start..]);
            break;
        }
    }
    parts
}

fn find_double_newline(data: &[u8]) -> Option<usize> {
    // Look for \r\n\r\n
    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        return Some(pos + 4);
    }
    // Look for \n\n
    if let Some(pos) = data.windows(2).position(|w| w == b"\n\n") {
        return Some(pos + 2);
    }
    None
}

fn dedup_filename(dir: &std::path::Path, filename: &str) -> String {
    if !dir.join(filename).exists() {
        return filename.to_string();
    }

    let stem = std::path::Path::new(filename)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let ext = std::path::Path::new(filename)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let mut counter = 1;
    loop {
        let new_name = if ext.is_empty() {
            format!("{}-{}", stem, counter)
        } else {
            format!("{}-{}.{}", stem, counter, ext)
        };
        if !dir.join(&new_name).exists() {
            return new_name;
        }
        counter += 1;
    }
}

fn atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(path);
    let tmp = dir.join(format!(".md-tmp-{}", std::process::id()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

use std::io::IsTerminal;

fn load_custom_css(path: Option<&str>) -> String {
    match path {
        Some(p) => std::fs::read_to_string(p).unwrap_or_else(|e| {
            eprintln!("Warning: could not read CSS file '{}': {}", p, e);
            String::new()
        }),
        None => String::new(),
    }
}
