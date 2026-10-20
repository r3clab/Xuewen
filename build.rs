use std::fs;
use std::path::Path;

// rust-embed embeds `frontend/dist` at compile time and errors if the folder is
// absent. Ensure it exists with a placeholder page so `cargo build`/tests work
// without a frontend build; a real `npm run build` (Plan B) overwrites it.
fn main() {
    let dist = Path::new("frontend/dist");
    let index = dist.join("index.html");
    if !index.exists() {
        fs::create_dir_all(dist).expect("create frontend/dist");
        fs::write(&index, PLACEHOLDER).expect("write placeholder index.html");
    }
    println!("cargo:rerun-if-changed=frontend/dist");
}

const PLACEHOLDER: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Xuewen</title></head><body style=\"font-family:system-ui;max-width:40rem;margin:4rem auto;padding:0 1rem\">\
<h1>Xuewen</h1><p>The API is running. The web UI has not been built yet — run \
<code>npm --prefix frontend run build</code> and rebuild.</p>\
<p>Try <a href=\"/api/stats\">/api/stats</a>.</p></body></html>";
