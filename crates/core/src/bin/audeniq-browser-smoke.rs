//! Rust WebDriver client. Runs only against a disposable local development stack.
use serde_json::{Value, json};
struct Browser {
    http: reqwest::Client,
    base: String,
}
impl Browser {
    async fn post(&self, path: &str, value: Value) -> anyhow::Result<Value> {
        let r: Value = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&value)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::ensure!(
            r["value"]["error"].is_null(),
            "WebDriver command failed: {}",
            r["value"]["error"]
        );
        Ok(r["value"].clone())
    }
    async fn find(&self, selector: &str) -> anyhow::Result<String> {
        for _ in 0..60 {
            if let Ok(v) = self
                .post("/element", json!({"using":"css selector","value":selector}))
                .await
                && let Some(id) = v["element-6066-11e4-a52e-4f735466cecf"].as_str()
            {
                return Ok(id.into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        anyhow::bail!("element not found: {selector}")
    }
    async fn type_in(&self, selector: &str, text: &str) -> anyhow::Result<()> {
        let id = self.find(selector).await?;
        self.post(&format!("/element/{id}/clear"), json!({}))
            .await?;
        self.post(&format!("/element/{id}/value"), json!({"text":text}))
            .await?;
        Ok(())
    }
    async fn click(&self, selector: &str) -> anyhow::Result<()> {
        let id = self.find(selector).await?;
        self.post(&format!("/element/{id}/click"), json!({}))
            .await?;
        // Wait for Rust's async action dispatch to finish (DOM-only WebDriver glue).
        for _ in 0..120 {
            let v=self.post("/execute/sync",json!({"script":"return document.querySelector('#workspace').getAttribute('aria-busy') !== 'true'","args":[]})).await?;
            if v == true {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        anyhow::bail!("action timeout")
    }
    async fn assert_text(&self, value: &str) -> anyhow::Result<()> {
        let text = self
            .post(
                "/execute/sync",
                json!({"script":"return document.body.innerText","args":[]}),
            )
            .await?;
        anyhow::ensure!(
            text.as_str().unwrap_or_default().contains(value),
            "expected UI text: {value}; actual: {text}"
        );
        Ok(())
    }
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let v: Value=http.post("http://127.0.0.1:9515/session").json(&json!({"capabilities":{"alwaysMatch":{"browserName":"chrome","goog:chromeOptions":{"args":["--headless=new","--no-sandbox","--disable-dev-shm-usage"]}}}})).send().await?.error_for_status()?.json().await?;
    let id = v["value"]["sessionId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no browser session"))?;
    let browser = Browser {
        http,
        base: format!("http://127.0.0.1:9515/session/{id}"),
    };
    let result = run(&browser).await;
    browser.http.delete(&browser.base).send().await?;
    result
}
async fn run(b: &Browser) -> anyhow::Result<()> {
    b.post("/url", json!({"url":"http://localhost:5173"}))
        .await?;
    b.type_in(
        "#email",
        &format!("browser-{}@example.test", uuid::Uuid::new_v4()),
    )
    .await?;
    b.type_in("#password", "browser-test-password-123").await?;
    b.click("[data-action=register]").await?;
    b.find("#org").await?;
    b.click("[data-action=artists]").await?;
    b.click("[data-action=new]").await?;
    b.type_in("#name", "Browser Artist <safe>").await?;
    b.click("[data-form=save] button[type=submit]").await?;
    b.assert_text("Browser Artist <safe>").await?;
    b.click("[data-action=releases]").await?;
    b.click("[data-action=new]").await?;
    b.type_in("#name", "Browser Release").await?;
    b.click("[data-form=save] button[type=submit]").await?;
    b.assert_text("Browser Release").await?;
    let screenshot: Value = b
        .http
        .get(format!("{}/screenshot", b.base))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    std::fs::create_dir_all("test-results")?;
    std::fs::write(
        "test-results/studio.png.base64",
        screenshot["value"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing screenshot"))?,
    )?;
    b.click("[data-action=preflight]").await?;
    b.assert_text("최종 제출할 수 없어요").await?;
    b.post("/refresh", json!({})).await?;
    b.find("[data-action=detail]").await?;
    b.assert_text("Browser Release").await?;
    b.click("[data-action=logout]").await?;
    b.find("#password").await?;
    println!(
        "Browser smoke passed: registration, session, artist/release writes, escaped metadata, preflight gate, reload CSRF recovery and logout"
    );
    Ok(())
}
