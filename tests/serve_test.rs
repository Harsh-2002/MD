use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

// Serialize all serve tests to avoid resource contention
static SERVE_LOCK: Mutex<()> = Mutex::new(());

// ── Helpers ──────────────────────────────────────────────────────────

struct ServeProcess {
    child: Child,
    port: u16,
    // Keep draining stderr so the server doesn't get SIGPIPE/broken pipe
    // when it writes to stderr after we've read the port line.
    _stderr_drain: Option<JoinHandle<()>>,
}

impl ServeProcess {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }
}

impl Drop for ServeProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_serve(args: &[&str]) -> ServeProcess {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdx"))
        .arg("serve")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start mdx serve");

    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr);
    let mut port = 0u16;
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                // Lines look like: "  Serving foo.md at http://127.0.0.1:PORT"
                if let Some(idx) = line.rfind(':') {
                    if let Ok(p) = line[idx + 1..].trim().parse::<u16>() {
                        port = p;
                        break;
                    }
                }
            }
        }
    }

    assert!(port > 0, "Failed to detect serve port");

    // Keep reading stderr in background so the pipe stays open and the
    // server doesn't panic on broken pipe when it writes more output.
    let drain = std::thread::spawn(move || {
        let mut buf = String::new();
        while reader.read_line(&mut buf).unwrap_or(0) > 0 {
            buf.clear();
        }
    });

    // Give server a moment to be fully ready
    std::thread::sleep(Duration::from_millis(500));

    ServeProcess {
        child,
        port,
        _stderr_drain: Some(drain),
    }
}

fn http_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build();
    config.into()
}

fn http_get(url: &str) -> (u16, String) {
    let agent = http_agent();
    for attempt in 0..3 {
        match agent.get(url).call() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.into_body().read_to_string().unwrap_or_default();
                return (status, body);
            }
            Err(ureq::Error::StatusCode(code)) => {
                let status = code;
                let body = String::new();
                return (status, body);
            }
            Err(_) if attempt < 2 => {
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(_) => return (0, String::new()),
        }
    }
    (0, String::new())
}

fn http_put(url: &str, body: &str) -> u16 {
    let agent = http_agent();
    for attempt in 0..3 {
        match agent.put(url).send(body) {
            Ok(resp) => return resp.status().as_u16(),
            Err(ureq::Error::StatusCode(code)) => return code,
            Err(_) if attempt < 2 => {
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(_) => return 0,
        }
    }
    0
}

fn http_post(url: &str, body: &str) -> (u16, String) {
    let agent = http_agent();
    for attempt in 0..3 {
        match agent.post(url).send(body) {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let resp_body = resp.into_body().read_to_string().unwrap_or_default();
                return (status, resp_body);
            }
            Err(ureq::Error::StatusCode(code)) => {
                return (code, String::new());
            }
            Err(_) if attempt < 2 => {
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(_) => return (0, String::new()),
        }
    }
    (0, String::new())
}

fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let unique_name = format!("md-test-{}-{}", id, name);
    let path = std::env::temp_dir().join(unique_name);
    std::fs::write(&path, content).unwrap();
    path
}

// ── Single file serve ────────────────────────────────────────────────

#[test]
fn test_serve_single_file_page() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-test.md", "# Hello Serve\n\nContent here.");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url("/"));
    assert_eq!(status, 200);
    assert!(
        body.contains("<!DOCTYPE html>"),
        "Should return full HTML page"
    );
    assert!(
        body.contains("Hello Serve"),
        "Should contain rendered heading"
    );
    assert!(
        body.contains("Content here"),
        "Should contain rendered content"
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_single_file_raw() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-raw.md", "# Raw Test\n\nParagraph.");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url("/raw"));
    assert_eq!(status, 200);
    assert!(body.contains("<h1>"), "Raw should contain HTML fragment");
    assert!(body.contains("Raw Test"), "Raw should contain heading text");
    assert!(!body.contains("<!DOCTYPE"), "Raw should NOT be a full page");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_single_file_source_get() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-source.md", "# Source Test\n\nOriginal content.");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url("/source"));
    assert_eq!(status, 200);
    assert_eq!(body.trim(), "# Source Test\n\nOriginal content.");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_single_file_source_put() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-put.md", "# Before\n\nOld content.");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let status = http_put(&srv.url("/source"), "# After\n\nNew content.");
    assert_eq!(status, 200);

    // Verify file was actually written
    std::thread::sleep(Duration::from_millis(100));
    let on_disk = std::fs::read_to_string(&tmp).unwrap();
    assert_eq!(on_disk, "# After\n\nNew content.");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_editor_ui_elements() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-editor-ui.md", "# Editor Test");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (_, body) = http_get(&srv.url("/"));
    assert!(
        body.contains("editor-toggle"),
        "Should have editor toggle button"
    );
    assert!(body.contains("editor-pane"), "Should have editor pane");
    assert!(
        body.contains("editor-textarea"),
        "Should have editor textarea"
    );
    assert!(
        body.contains("editor-toolbar"),
        "Should have editor toolbar"
    );
    assert!(body.contains("editor-format-bar"), "Should have format bar");
    assert!(body.contains("line-numbers"), "Should have line numbers");
    assert!(body.contains("fmt-btn"), "Should have format buttons");
    assert!(
        body.contains("has-editor"),
        "Body should have has-editor class"
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_format_buttons() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-fmt.md", "# Format Test");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (_, body) = http_get(&srv.url("/"));
    assert!(
        body.contains(r#"data-fmt="bold""#),
        "Should have bold button"
    );
    assert!(
        body.contains(r#"data-fmt="italic""#),
        "Should have italic button"
    );
    assert!(
        body.contains(r#"data-fmt="heading""#),
        "Should have heading button"
    );
    assert!(
        body.contains(r#"data-fmt="strikethrough""#),
        "Should have strikethrough button"
    );
    assert!(
        body.contains(r#"data-fmt="code""#),
        "Should have code button"
    );
    assert!(
        body.contains(r#"data-fmt="link""#),
        "Should have link button"
    );
    assert!(
        body.contains(r#"data-fmt="list""#),
        "Should have list button"
    );
    assert!(
        body.contains(r#"data-fmt="quote""#),
        "Should have quote button"
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_sse_endpoint() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-sse.md", "# SSE Test");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    // SSE endpoint should respond — use a short timeout since the connection stays open.
    // We just verify the endpoint exists and doesn't 404.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(1)))
        .build()
        .into();
    match agent.get(&srv.url("/events")).call() {
        Ok(_) => {} // Connected — endpoint exists
        Err(ureq::Error::StatusCode(code)) => {
            assert_ne!(code, 404, "SSE endpoint should exist");
        }
        Err(_) => {} // Timeout is expected for SSE
    }

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_custom_port() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-port.md", "# Port Test");
    let srv = start_serve(&["--port", "0", tmp.to_str().unwrap()]);

    let (status, _) = http_get(&srv.url("/"));
    assert_eq!(status, 200);

    let _ = std::fs::remove_file(&tmp);
}

// ── Directory serve ──────────────────────────────────────────────────

#[test]
fn test_serve_directory_index() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("alpha.md"), "# Alpha").unwrap();
    std::fs::write(dir.join("beta.md"), "# Beta").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url("/"));
    assert_eq!(status, 200);
    assert!(body.contains("alpha.md"), "Index should list alpha.md");
    assert!(body.contains("beta.md"), "Index should list beta.md");
    assert!(body.contains("file-grid"), "Should have file grid");
    assert!(
        body.contains(r#"id="new-note-card""#),
        "Directory mode should have new note card"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_file_page() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-page");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("readme.md"),
        "# Directory Readme\n\nHello from dir.",
    )
    .unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url("/readme.md"));
    assert_eq!(status, 200);
    assert!(
        body.contains("Directory Readme"),
        "Should render file content"
    );
    assert!(body.contains("Hello from dir"), "Should render paragraph");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_file_raw() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-raw");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("doc.md"), "# Doc Raw").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url("/doc.md/raw"));
    assert_eq!(status, 200);
    assert!(
        body.contains("Doc Raw"),
        "Raw endpoint should return HTML fragment"
    );
    assert!(!body.contains("<!DOCTYPE"), "Raw should not be a full page");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_source_get() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-source-get");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.md"), "# My Notes\n\nSome notes.").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url("/notes.md/source"));
    assert_eq!(status, 200);
    assert_eq!(body.trim(), "# My Notes\n\nSome notes.");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_source_put() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-source-put");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("edit.md"), "# Original").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    let status = http_put(&srv.url("/edit.md/source"), "# Updated\n\nNew text.");
    assert_eq!(status, 200);

    std::thread::sleep(Duration::from_millis(100));
    let on_disk = std::fs::read_to_string(dir.join("edit.md")).unwrap();
    assert_eq!(on_disk, "# Updated\n\nNew text.");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_create_file() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-create");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("existing.md"), "# Existing").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    let (status, body) = http_post(&srv.url("/create"), "new-note");
    assert_eq!(status, 200);
    assert_eq!(body.trim(), "new-note.md");

    // File should exist on disk
    let created = std::fs::read_to_string(dir.join("new-note.md")).unwrap();
    assert!(
        created.contains("# new-note"),
        "Created file should have title heading"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_create_file_with_md_extension() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-create-ext");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("seed.md"), "# Seed").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    let (status, body) = http_post(&srv.url("/create"), "readme.md");
    assert_eq!(status, 200);
    assert_eq!(body.trim(), "readme.md");
    assert!(dir.join("readme.md").exists(), "File should be created");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_create_duplicate() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-create-dup");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("exists.md"), "# Exists").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    let (status, _) = http_post(&srv.url("/create"), "exists");
    assert_eq!(status, 409, "Creating duplicate should return 409 Conflict");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_create_invalid_filename() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-create-invalid");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("seed.md"), "# Seed").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    // Path traversal
    let (status, _) = http_post(&srv.url("/create"), "../escape");
    assert_eq!(status, 400, "Path traversal should be rejected");

    // Slashes
    let (status, _) = http_post(&srv.url("/create"), "sub/dir");
    assert_eq!(status, 400, "Slashes should be rejected");

    // Empty
    let (status, _) = http_post(&srv.url("/create"), "");
    assert_eq!(status, 400, "Empty name should be rejected");

    // Too long
    let long_name = "a".repeat(256);
    let (status, _) = http_post(&srv.url("/create"), &long_name);
    assert_eq!(status, 400, "Too-long name should be rejected");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_serve_directory_create_then_serve() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join("md-serve-dir-create-serve");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("seed.md"), "# Seed").unwrap();

    let srv = start_serve(&[dir.to_str().unwrap()]);

    // Create a new file
    let (status, body) = http_post(&srv.url("/create"), "fresh-note");
    assert_eq!(status, 200);
    assert_eq!(body.trim(), "fresh-note.md");

    // The new file should be serveable
    let (status, body) = http_get(&srv.url("/fresh-note.md"));
    assert_eq!(status, 200);
    assert!(
        body.contains("fresh-note"),
        "New file page should contain its title"
    );

    // The index should now list the new file
    let (status, body) = http_get(&srv.url("/"));
    assert_eq!(status, 200);
    assert!(
        body.contains("fresh-note.md"),
        "Index should list the newly created file"
    );

    // The new file's source should be editable
    let status = http_put(
        &srv.url("/fresh-note.md/source"),
        "# Fresh Note\n\nEdited content.",
    );
    assert_eq!(status, 200);

    std::thread::sleep(Duration::from_millis(100));
    let on_disk = std::fs::read_to_string(dir.join("fresh-note.md")).unwrap();
    assert_eq!(on_disk, "# Fresh Note\n\nEdited content.");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── Multi-file serve ─────────────────────────────────────────────────

#[test]
fn test_serve_multi_file_no_create() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp1 = write_tmp("nc1.md", "# File A");
    let tmp2 = write_tmp("nc2.md", "# File B");

    let srv = start_serve(&[tmp1.to_str().unwrap(), tmp2.to_str().unwrap()]);

    // Index should NOT have new-note-card element in multi-file mode
    // (CSS contains the class name, so check for the actual HTML element id)
    let (_, body) = http_get(&srv.url("/"));
    assert!(
        !body.contains(r#"id="new-note-card""#),
        "Multi-file mode should NOT have new note card element"
    );

    let _ = std::fs::remove_file(&tmp1);
    let _ = std::fs::remove_file(&tmp2);
}

#[test]
fn test_serve_multi_file_index() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp1 = write_tmp("a.md", "# File A");
    let tmp2 = write_tmp("b.md", "# File B");
    let name1 = tmp1.file_name().unwrap().to_str().unwrap().to_string();
    let name2 = tmp2.file_name().unwrap().to_str().unwrap().to_string();

    let srv = start_serve(&[tmp1.to_str().unwrap(), tmp2.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url("/"));
    assert_eq!(status, 200);
    assert!(body.contains(&name1), "Index should list file A");
    assert!(body.contains(&name2), "Index should list file B");

    let _ = std::fs::remove_file(&tmp1);
    let _ = std::fs::remove_file(&tmp2);
}

#[test]
fn test_serve_multi_file_page() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp1 = write_tmp("p1.md", "# Page One\n\nFirst file.");
    let tmp2 = write_tmp("p2.md", "# Page Two\n\nSecond file.");
    let name1 = tmp1.file_name().unwrap().to_str().unwrap().to_string();
    let name2 = tmp2.file_name().unwrap().to_str().unwrap().to_string();

    let srv = start_serve(&[tmp1.to_str().unwrap(), tmp2.to_str().unwrap()]);

    let (status, body) = http_get(&srv.url(&format!("/{}", name1)));
    assert_eq!(status, 200);
    assert!(body.contains("Page One"), "Should render first file");

    let (status, body) = http_get(&srv.url(&format!("/{}", name2)));
    assert_eq!(status, 200);
    assert!(body.contains("Page Two"), "Should render second file");

    let _ = std::fs::remove_file(&tmp1);
    let _ = std::fs::remove_file(&tmp2);
}

#[test]
fn test_serve_multi_file_sidebar_navigation() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp1 = write_tmp("nav1.md", "# Nav One");
    let tmp2 = write_tmp("nav2.md", "# Nav Two");
    let name1 = tmp1.file_name().unwrap().to_str().unwrap().to_string();

    let srv = start_serve(&[tmp1.to_str().unwrap(), tmp2.to_str().unwrap()]);

    let (_, body) = http_get(&srv.url(&format!("/{}", name1)));
    assert!(
        body.contains("back-to-index"),
        "Should have back-to-index link"
    );
    assert!(
        body.contains("file-nav"),
        "Should have file navigation panel"
    );
    assert!(
        body.contains("file-search"),
        "Should have file search input"
    );
    assert!(body.contains("sidebar-tab"), "Should have sidebar tabs");

    let _ = std::fs::remove_file(&tmp1);
    let _ = std::fs::remove_file(&tmp2);
}

// ── HTML output structure ────────────────────────────────────────────

#[test]
fn test_serve_html_has_mermaid() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-mermaid.md", "# Mermaid Test");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (_, body) = http_get(&srv.url("/"));
    assert!(
        body.contains("mermaid") || body.contains("Mermaid"),
        "Should include mermaid script: body len={}",
        body.len()
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_html_has_katex() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-katex.md", "# KaTeX Test");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (_, body) = http_get(&srv.url("/"));
    assert!(
        body.contains("katex") || body.contains("KaTeX"),
        "Should include KaTeX: body len={}",
        body.len()
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_html_has_theme_toggle() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-theme.md", "# Theme Test");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (_, body) = http_get(&srv.url("/"));
    assert!(
        body.contains("theme-toggle"),
        "Should have theme toggle button"
    );
    assert!(
        body.contains("data-theme"),
        "Should have data-theme attribute"
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_serve_html_has_toc_sidebar() {
    let _guard = SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = write_tmp("md-serve-toc.md", "# Heading\n\n## Sub");
    let srv = start_serve(&[tmp.to_str().unwrap()]);

    let (_, body) = http_get(&srv.url("/"));
    assert!(body.contains("toc-panel"), "Should have ToC panel");
    assert!(
        body.contains("sidebar-toggle"),
        "Should have sidebar toggle"
    );
    assert!(body.contains("progress-bar"), "Should have progress bar");
    assert!(
        body.contains("back-to-top"),
        "Should have back-to-top button"
    );

    let _ = std::fs::remove_file(&tmp);
}
