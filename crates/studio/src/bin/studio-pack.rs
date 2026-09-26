//! Retain the original visual CSS/assets, never ship the prototype's local business logic.
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string("web/studio/public/index.html")?;
    let out = Path::new("web/studio/dist");
    fs::create_dir_all(out.join("assets"))?;
    let mut css = String::new();
    for part in source.split("<style>").skip(1) {
        if let Some((style, _)) = part.split_once("</style>") {
            css.push_str(style);
            css.push('\n');
        }
    }
    if css.is_empty() {
        return Err("original Studio CSS missing".into());
    }
    css.push_str("\n[hidden]{display:none!important}.connected-nav{display:flex;gap:12px;flex-wrap:wrap;padding:12px 0}.connected-form{display:grid;gap:16px;max-width:640px}.connected-form input,.connected-form select{width:100%;padding:12px;border:1px solid var(--border);border-radius:12px;font:inherit}.connected-form label{display:block}#feedback{white-space:pre-wrap;margin:16px 0}.connected-row{display:flex;justify-content:space-between;gap:16px;padding:20px 0;border-bottom:1px solid var(--border)}button:disabled{opacity:.5;cursor:wait}.connected-detail{margin-top:24px} .connected-nav button{border:0;background:var(--surface-alt);padding:10px 16px;border-radius:12px}\n");
    fs::write(out.join("studio.css"), css)?;
    fs::copy(
        "web/studio/public/connected/index.html",
        out.join("index.html"),
    )?;
    fs::copy(
        "web/studio/public/assets/AUDENIQ_Logo_Light.svg",
        out.join("assets/AUDENIQ_Logo_Light.svg"),
    )?;
    // Minimal module bootstrap. All application state, requests and DOM handling are Rust.
    fs::write(
        out.join("boot.js"),
        "import init from './pkg/audeniq_studio.js';\ninit();\n",
    )?;
    fs::write(out.join("robots.txt"), "User-agent: *\nDisallow: /\n")?;
    Ok(())
}
