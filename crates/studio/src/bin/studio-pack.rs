//! Retain the original visual CSS/assets, never ship the prototype's local business logic.
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The original Studio CSS, extracted from the prototype shell before
    // web/studio/public became the React build (see assets/studio-original.css).
    let mut css = include_str!("../../assets/studio-original.css").to_string();
    if css.trim().is_empty() {
        return Err("original Studio CSS missing".into());
    }
    let out = Path::new("web/studio/dist");
    fs::create_dir_all(out.join("assets"))?;
    css.push_str("\n[hidden]{display:none!important}.connected-nav{display:flex;gap:12px;flex-wrap:wrap;padding:12px 0}.connected-form{display:grid;gap:16px;max-width:640px}.connected-form input,.connected-form select{width:100%;padding:12px;border:1px solid var(--border);border-radius:12px;font:inherit}.connected-form label{display:block}#feedback{white-space:pre-wrap;margin:16px 0}.connected-row{display:flex;justify-content:space-between;gap:16px;padding:20px 0;border-bottom:1px solid var(--border)}button:disabled{opacity:.5;cursor:wait}.connected-detail{margin-top:24px} .connected-nav button{border:0;background:var(--surface-alt);padding:10px 16px;border-radius:12px}\n");
    fs::write(out.join("studio.css"), css)?;
    // Minimal module bootstrap. All application state, requests and DOM handling are Rust.
    // NOTE: this shell is the original studio shell (it provides #workspace / #feedback
    // for the wasm app). Do not reuse the old Vite prototype shell
    // (web/studio/public/connected/index.html); it references a bundled JS artifact
    // that no longer exists and never loads boot.js.
    fs::write(
        out.join("index.html"),
        concat!(
            "<!doctype html><html lang=\"ko\"><head>",
            "<meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1,viewport-fit=cover\">",
            "<meta name=\"robots\" content=\"noindex,nofollow\">",
            "<title>AUDENIQ STUDIO</title>",
            "<link rel=\"stylesheet\" href=\"/studio.css\">",
            "<script type=\"module\" src=\"/boot.js\"></script>",
            "</head>\n",
            "<body>",
            "<a class=\"skip-link\" href=\"#main\">본문으로 바로가기</a>",
            "<header class=\"site-header portal-header\"><div class=\"nav-shell\">",
            "<a href=\"/\" class=\"brand\" aria-label=\"AUDENIQ STUDIO 홈\">",
            "<img src=\"/assets/AUDENIQ_Logo_Light.svg\" alt=\"AUDENIQ\"></a>",
            "<span class=\"workspace-label\">STUDIO</span>",
            "</div></header>\n",
            "<main id=\"main\" class=\"portal-layout\">",
            "<div id=\"feedback\" role=\"status\" aria-live=\"polite\">작업실을 불러오고 있어요.</div>",
            "<div id=\"workspace\"></div>",
            "<noscript>작업실을 사용하려면 브라우저의 JavaScript와 WebAssembly를 활성화해 주세요.</noscript>",
            "</main></body></html>\n",
        ),
    )?;
    fs::copy(
        "web/studio/public/static/AUDENIQ_Logo_Light.svg",
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
