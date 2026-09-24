use crate::{audio_mime, escape};
use serde_json::{Value, json};
use std::cell::RefCell;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    Element, HtmlInputElement, HtmlSelectElement, Request, RequestCredentials, RequestInit,
    Response,
};

type Result<T> = std::result::Result<T, String>;
#[derive(Default)]
struct State {
    csrf: String,
    org: String,
    party: String,
    orgs: Vec<Value>,
    kind: String,
    detail: Value,
    next: Option<String>,
    busy: bool,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
fn document() -> web_sys::Document {
    web_sys::window()
        .expect("browser window")
        .document()
        .expect("document")
}
fn element(id: &str) -> Element {
    document()
        .get_element_by_id(id)
        .expect("application element")
}
fn field(id: &str) -> String {
    let Some(e) = document().get_element_by_id(id) else {
        return String::new();
    };
    if let Some(v) = e.dyn_ref::<HtmlInputElement>() {
        v.value()
    } else if let Some(v) = e.dyn_ref::<HtmlSelectElement>() {
        v.value()
    } else {
        String::new()
    }
}
fn notice(s: &str) {
    element("feedback").set_text_content(Some(s));
}
fn text(v: &Value, key: &str) -> String {
    escape(v[key].as_str().unwrap_or(""))
}
fn api_path(suffix: &str) -> String {
    STATE.with(|s| format!("/api/orgs/{}/{}", s.borrow().org, suffix))
}
fn fail<T>(_: T) -> String {
    "네트워크 연결을 확인해 주세요. 변경 결과가 불명확하면 목록을 새로 확인해 주세요.".into()
}
async fn request(method: &str, path: &str, body: Option<Value>) -> Result<Value> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_credentials(RequestCredentials::SameOrigin);
    if let Some(v) = body {
        init.set_body(&JsValue::from_str(&v.to_string()));
    }
    let req = Request::new_with_str_and_init(path, &init).map_err(fail)?;
    req.headers()
        .set("Content-Type", "application/json")
        .map_err(fail)?;
    let csrf = STATE.with(|s| s.borrow().csrf.clone());
    if !csrf.is_empty() && method != "GET" {
        req.headers().set("X-CSRF-Token", &csrf).map_err(fail)?;
    }
    let r = JsFuture::from(web_sys::window().unwrap().fetch_with_request(&req))
        .await
        .map_err(fail)?
        .dyn_into::<Response>()
        .map_err(fail)?;
    let status = r.status();
    let raw = JsFuture::from(r.text().map_err(fail)?)
        .await
        .map_err(fail)?
        .as_string()
        .unwrap_or_default();
    let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    if !r.ok() {
        if status == 401 {
            STATE.with(|s| *s.borrow_mut() = State::default());
            login_view();
        }
        let message = match status {
            401 => "세션이 만료됐어요. 다시 로그인해 주세요.",
            403 => "접근 권한이 없거나 인증 정보가 변경됐어요. 다시 로그인해 주세요.",
            409 => {
                "다른 변경이 있거나 파일 처리가 아직 끝나지 않았어요. 새로고침 후 확인해 주세요."
            }
            429 => "요청이 많아요. 잠시 후 다시 시도해 주세요.",
            501 => "제출 요건과 심사 기능 준비 중으로 최종 제출할 수 없어요.",
            503 => "서버 또는 파일 저장소 연결을 사용할 수 없어요.",
            _ => "요청을 처리하지 못했어요. 입력값과 연결 상태를 확인해 주세요.",
        };
        return Err(format!("{message} ({status})"));
    }
    Ok(value)
}
fn input(id: &str, label: &str, value: &str, ty: &str) -> String {
    format!(
        "<label>{label}<input id=\"{id}\" type=\"{ty}\" value=\"{}\" required></label>",
        escape(value)
    )
}
fn button(action: &str, label: &str) -> String {
    format!("<button type=\"button\" class=\"button\" data-action=\"{action}\">{label}</button>")
}
fn login_view() {
    element("workspace").set_inner_html(&format!("<section class=\"view\"><div class=\"view-title\"><h1>내 작업실에 로그인</h1></div><form data-form=\"login\" class=\"surface connected-form\">{}{}<button class=\"button\" type=\"submit\">로그인</button>{}<p class=\"small\">새 계정의 비밀번호는 12~128바이트로 입력해 주세요. 회원 등록은 배급 계약 체결이나 최종 제출을 의미하지 않아요.</p></form></section>", input("email","이메일","","email"), input("password","비밀번호","","password"),button("register","회원 등록")));
}
fn shell() {
    let orgs = STATE.with(|s| {
        s.borrow()
            .orgs
            .iter()
            .map(|o| {
                format!(
                    "<option value=\"{}\" {}>{}</option>",
                    text(o, "id"),
                    if o["id"].as_str() == Some(s.borrow().org.as_str()) {
                        "selected"
                    } else {
                        ""
                    },
                    text(o, "name")
                )
            })
            .collect::<String>()
    });
    element("workspace").set_inner_html(&format!("<div class=\"view-title\"><div><p class=\"eyebrow\">AUDENIQ / STUDIO</p><h1>내 작업실</h1></div>{}</div><label>작업 조직 <select id=\"org\">{orgs}</select></label>{}<nav class=\"connected-nav\" aria-label=\"작업실 메뉴\">{}{}{}{}{} </nav><div id=\"content\"></div>",button("logout","로그아웃"),button("switch","조직 선택"),button("releases","발매·곡 관리"),button("artists","아티스트"),button("labels","레이블"),button("sessions","로그인 세션"),button("gated","계약·정산·배급")));
}
async fn bootstrap() -> Result<()> {
    let csrf = request("POST", "/api/auth/csrf", Some(json!({}))).await?;
    STATE.with(|s| s.borrow_mut().csrf = csrf["csrf_token"].as_str().unwrap_or_default().into());
    let me = request("GET", "/api/me", None).await?;
    STATE.with(|s| s.borrow_mut().party = me["party_id"].as_str().unwrap_or_default().into());
    let orgs = request("GET", "/api/orgs", None).await?;
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.orgs = orgs["items"].as_array().cloned().unwrap_or_default();
        s.org = s
            .orgs
            .first()
            .and_then(|v| v["id"].as_str())
            .unwrap_or_default()
            .into();
    });
    shell();
    list("releases", None).await
}
async fn list(kind: &str, after: Option<String>) -> Result<()> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.kind = kind.into();
        s.detail = Value::Null;
    });
    let suffix = format!(
        "{kind}?limit=50{}",
        after.map(|v| format!("&after={v}")).unwrap_or_default()
    );
    let data = request("GET", &api_path(&suffix), None).await?;
    STATE.with(|s| s.borrow_mut().next = data["next_cursor"].as_str().map(str::to_owned));
    let mut html = format!(
        "<section class=\"surface\"><div class=\"section-top\"><h2>{}</h2>{}</div>",
        match kind {
            "artists" => "아티스트",
            "labels" => "레이블",
            _ => "발매 초안",
        },
        button("new", "새로 만들기")
    );
    for row in data["items"].as_array().into_iter().flatten() {
        let name = if kind == "releases" {
            text(row, "title")
        } else {
            text(row, "name")
        };
        html.push_str(&format!("<div class=\"connected-row\"><div><strong>{name}</strong><p class=\"small\">{}</p></div><button type=\"button\" class=\"link-btn\" data-action=\"detail\" data-id=\"{}\">열기 ↗</button></div>",text(row,"status"),text(row,"id")));
    }
    if data["items"].as_array().is_none_or(Vec::is_empty) {
        html.push_str("<p class=\"empty-note\">등록된 항목이 없어요.</p>");
    }
    html.push_str(&button("first", "처음부터"));
    if data["next_cursor"].is_string() {
        html.push_str(&button("next", "다음 페이지"));
    }
    html.push_str("</section>");
    element("content").set_inner_html(&html);
    Ok(())
}
fn editor() {
    let (kind, v) = STATE.with(|s| (s.borrow().kind.clone(), s.borrow().detail.clone()));
    let name = if kind == "releases" {
        v["title"].as_str()
    } else {
        v["name"].as_str()
    }
    .unwrap_or_default();
    let mut html = format!(
        "<form class=\"surface connected-form\" data-form=\"save\"><h2>{}</h2>{}",
        if v.is_null() {
            "새 항목"
        } else {
            "기본 정보 수정"
        },
        input("name", "표시명 / 발매 제목", name, "text")
    );
    if kind == "releases" {
        html.push_str("<label>발매 유형<select id=\"release-type\">");
        for ty in ["SINGLE", "EP", "ALBUM"] {
            html.push_str(&format!(
                "<option {}>{ty}</option>",
                if v["release_type"] == ty {
                    "selected"
                } else {
                    ""
                }
            ));
        }
        html.push_str("</select></label>");
    }
    if kind == "labels" {
        html.push_str(&format!(
            "<p class=\"small\">내 계정 당사자 ID: {}. 이 조직의 당사자만 연결할 수 있어요.</p>",
            STATE.with(|s| escape(&s.borrow().party))
        ));
        html.push_str(&input(
            "party",
            "계약 당사자 ID",
            v["party_id"].as_str().unwrap_or_default(),
            "text",
        ));
    }
    html.push_str("<button type=\"submit\" class=\"button\">저장</button></form>");
    element("content").set_inner_html(&html);
}
async fn detail(id: &str) -> Result<()> {
    let kind = STATE.with(|s| s.borrow().kind.clone());
    let v = request("GET", &api_path(&format!("{kind}/{id}")), None).await?;
    STATE.with(|s| s.borrow_mut().detail = v.clone());
    let name = if kind == "releases" {
        text(&v, "title")
    } else {
        text(&v, "name")
    };
    let mut html = format!(
        "<section class=\"surface\"><h2>{name}</h2><p class=\"small\">{}</p>{}{}{}",
        text(&v, "status"),
        button("edit", "수정"),
        button("archive", "보관하기"),
        button("first", "목록")
    );
    html.push_str(&format!(
        "<p class=\"small\">내부 ID: {}</p>",
        text(&v, "id")
    ));
    if kind == "releases" {
        html.push_str("<p class=\"notice\">초안을 저장할 수 있어요. 계약·동의 및 QC가 준비되지 않아 최종 제출과 배급은 비활성 상태예요.</p>");
        for t in v["tracks"].as_array().into_iter().flatten() {
            html.push_str(&format!("<div class=\"connected-row\"><div><strong>{}. {}</strong><p class=\"small\">파일: {}</p></div><button type=\"button\" class=\"link-btn\" data-action=\"remove-track\" data-id=\"{}\">트랙 보관</button></div>",t["track_number"],text(t,"title"),if t["asset_id"].is_string(){"등록됨 · QC 상태 별도 확인"}else{"미연결"},text(t,"id")));
        }
        html.push_str(&button("preflight", "제출 준비 상태 확인"));
        html.push_str(&format!("<form class=\"connected-form connected-detail\" data-form=\"track\"><h3>트랙 추가</h3>{}{}{}{}<label>원본 음원 (WAV / FLAC, 선택)<input id=\"audio\" type=\"file\" accept=\".wav,.flac\"></label><p class=\"small\">선택한 파일은 저장소로 직접 업로드돼요. 파일 등록은 음원 QC 통과를 뜻하지 않아요.</p><button class=\"button\" type=\"submit\">트랙 저장</button></form>",input("track-title","곡 제목","","text"),input("artist","아티스트 ID","","text"),input("disc","디스크 번호","1","number"),input("track-number","트랙 번호",&(v["tracks"].as_array().map_or(0,Vec::len)+1).to_string(),"number")));
    }
    html.push_str("</section>");
    element("content").set_inner_html(&html);
    Ok(())
}
async fn upload_audio() -> Result<Option<String>> {
    let input = element("audio")
        .dyn_into::<HtmlInputElement>()
        .map_err(fail)?;
    let Some(file) = input.files().and_then(|f| f.get(0)) else {
        return Ok(None);
    };
    let mime = audio_mime(&file.name()).ok_or("WAV 또는 FLAC 파일을 선택해 주세요.")?;
    if file.size() < 1.0 || file.size() > 512.0 * 1024.0 * 1024.0 {
        return Err("음원은 1바이트~512MiB여야 해요.".into());
    }
    let grant = request(
        "POST",
        &api_path("uploads"),
        Some(json!({"kind":"AUDIO","size_bytes":file.size() as i64,"content_type":mime})),
    )
    .await?;
    let id = grant["upload_session_id"]
        .as_str()
        .ok_or("업로드 응답 오류")?;
    let url = grant["grant"]["url"].as_str().ok_or("업로드 응답 오류")?;
    if !url.starts_with("https://") {
        return Err("안전하지 않은 업로드 주소를 거부했어요.".into());
    }
    notice("음원을 저장소에 직접 업로드하고 있어요. 이 창을 유지해 주세요.");
    let init = RequestInit::new();
    init.set_method("PUT");
    init.set_credentials(RequestCredentials::Omit);
    init.set_body(file.as_ref());
    let req = Request::new_with_str_and_init(url, &init).map_err(fail)?;
    for (key, value) in grant["grant"]["headers"]
        .as_object()
        .ok_or("업로드 헤더 오류")?
    {
        // Content-Length is a browser-controlled forbidden header; File supplies the exact length.
        if key.eq_ignore_ascii_case("content-length") {
            continue;
        }
        if !matches!(key.as_str(), "content-type" | "x-amz-meta-upload-nonce") {
            return Err("지원하지 않는 업로드 헤더".into());
        }
        req.headers()
            .set(key, value.as_str().ok_or("업로드 헤더 오류")?)
            .map_err(fail)?;
    }
    let response = JsFuture::from(web_sys::window().unwrap().fetch_with_request(&req))
        .await
        .map_err(fail)?
        .dyn_into::<Response>()
        .map_err(fail)?;
    if !response.ok() {
        let _ = request(
            "POST",
            &api_path(&format!("uploads/{id}/cancel")),
            Some(json!({})),
        )
        .await;
        return Err("음원 전송에 실패했어요. 파일을 다시 선택해 주세요.".into());
    }
    request(
        "POST",
        &api_path(&format!("uploads/{id}/complete")),
        Some(json!({"asset_id":grant["asset_id"],"expected_key":grant["expected_key"]})),
    )
    .await?;
    Ok(grant["asset_id"].as_str().map(str::to_owned))
}
async fn action(action: &str, id: &str) -> Result<()> {
    match action {
        "login" | "register" => {
            let body = json!({"email":field("email"),"password":field("password")});
            if action == "register" {
                request("POST", "/api/auth/register", Some(body.clone())).await?;
            }
            let v = request("POST", "/api/auth/login", Some(body)).await?;
            STATE.with(|s| {
                s.borrow_mut().csrf = v["csrf_token"].as_str().unwrap_or_default().into()
            });
            bootstrap().await?;
        }
        "logout" => {
            request("POST", "/api/auth/logout", Some(json!({}))).await?;
            STATE.with(|s| *s.borrow_mut() = State::default());
            login_view();
        }
        "switch" => {
            let org = field("org");
            STATE.with(|s| s.borrow_mut().org = org);
            list("releases", None).await?;
        }
        "artists" | "labels" | "releases" => list(action, None).await?,
        "next" | "first" => {
            let (kind, next) = STATE.with(|s| (s.borrow().kind.clone(), s.borrow().next.clone()));
            list(&kind, if action == "next" { next } else { None }).await?;
        }
        "detail" => detail(id).await?,
        "new" => {
            STATE.with(|s| s.borrow_mut().detail = Value::Null);
            editor();
        }
        "edit" => editor(),
        "save" => {
            let (kind, v) = STATE.with(|s| (s.borrow().kind.clone(), s.borrow().detail.clone()));
            let mut body = json!({"name":field("name"),"profile":if kind=="releases"{v["draft"].clone()}else{v["profile"].clone()},"party_id":v["party_id"],"label_id":v["label_id"],"row_version":v["row_version"]});
            if kind == "releases" {
                body["release_type"] = json!(field("release-type"));
            }
            if kind == "labels" {
                body["party_id"] = json!(field("party"));
            }
            let path = if v.is_null() {
                api_path(&kind)
            } else {
                api_path(&format!("{kind}/{}", v["id"].as_str().unwrap_or_default()))
            };
            let saved =
                request(if v.is_null() { "POST" } else { "PUT" }, &path, Some(body)).await?;
            let id = saved["id"]
                .as_str()
                .or(v["id"].as_str())
                .ok_or("저장 응답 오류")?;
            detail(id).await?;
        }
        "archive" => {
            if !web_sys::window()
                .unwrap()
                .confirm_with_message("이 항목을 보관할까요?")
                .map_err(fail)?
            {
                return Ok(());
            }
            let (kind, v) = STATE.with(|s| (s.borrow().kind.clone(), s.borrow().detail.clone()));
            request(
                "DELETE",
                &api_path(&format!("{kind}/{}", v["id"].as_str().unwrap_or_default())),
                Some(json!({"row_version":v["row_version"]})),
            )
            .await?;
            list(&kind, None).await?;
        }
        "track" => {
            let v = STATE.with(|s| s.borrow().detail.clone());
            let id = v["id"].as_str().ok_or("발매를 다시 열어 주세요.")?;
            let disc = field("disc")
                .parse::<i32>()
                .map_err(|_| "디스크 번호를 확인해 주세요.")?;
            let number = field("track-number")
                .parse::<i32>()
                .map_err(|_| "트랙 번호를 확인해 주세요.")?;
            let title = field("track-title");
            let artist = field("artist");
            let asset = upload_audio().await?;
            request("POST",&api_path(&format!("releases/{id}/tracks")),Some(json!({"title":title,"artist_id":artist,"disc_number":disc,"track_number":number,"asset_id":asset,"row_version":v["row_version"]}))).await?;
            detail(id).await?;
        }
        "remove-track" => {
            let v = STATE.with(|s| s.borrow().detail.clone());
            let release = v["id"].as_str().ok_or("발매를 다시 열어 주세요.")?;
            request(
                "DELETE",
                &api_path(&format!("releases/{release}/tracks/{id}")),
                Some(json!({"row_version":v["row_version"]})),
            )
            .await?;
            detail(release).await?;
        }
        "preflight" => {
            let id = STATE.with(|s| {
                s.borrow().detail["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            });
            let v = request("GET", &api_path(&format!("releases/{id}/preflight")), None).await?;
            let count = v["issues"].as_array().map_or(0, Vec::len);
            notice(&format!(
                "기본 정보 확인 항목 {count}개. 계약·동의·QC 기능 준비 중으로 최종 제출할 수 없어요."
            ));
            return Ok(());
        }
        "sessions" => {
            let v = request("GET", "/api/auth/sessions", None).await?;
            let mut html = String::from("<section class=\"surface\"><h2>로그인 세션</h2>");
            for row in v["items"].as_array().into_iter().flatten() {
                html.push_str(&format!("<div class=\"connected-row\"><span>{} · 만료 {}</span><button class=\"link-btn\" type=\"button\" data-action=\"revoke\" data-id=\"{}\">폐기</button></div>",if row["current"]==true{"현재 세션"}else{"다른 세션"},text(row,"expires_at"),text(row,"id")));
            }
            html.push_str("</section>");
            element("content").set_inner_html(&html);
        }
        "revoke" => {
            let v = request(
                "POST",
                &format!("/api/auth/sessions/{id}/revoke"),
                Some(json!({})),
            )
            .await?;
            if v["reauthentication_required"] == true {
                login_view();
                STATE.with(|s| *s.borrow_mut() = State::default());
            } else {
                element("content").set_text_content(Some(
                    "세션을 폐기했어요. 로그인 세션 메뉴에서 다시 확인해 주세요.",
                ));
            }
        }
        "gated" => {
            element("content").set_inner_html("<section class=\"surface\"><h2>준비 중인 기능</h2><p>계약·권리 심사, DSP 배급, 로열티 정산·지급은 아직 사용할 수 없어요. 현재 저장되는 발매는 초안이에요.</p></section>");
        }
        _ => return Ok(()),
    }
    notice("서버의 최신 정보를 반영했어요.");
    Ok(())
}
fn dispatch(action_name: String, id: String) {
    let busy = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.busy {
            true
        } else {
            s.busy = true;
            false
        }
    });
    if busy {
        return;
    }
    element("workspace").set_attribute("aria-busy", "true").ok();
    spawn_local(async move {
        notice("처리 중이에요.");
        if let Err(e) = action(&action_name, &id).await {
            notice(&e);
        }
        STATE.with(|s| s.borrow_mut().busy = false);
        element("workspace")
            .set_attribute("aria-busy", "false")
            .ok();
    });
}
#[wasm_bindgen(start)]
pub fn start() -> std::result::Result<(), JsValue> {
    let click = Closure::<dyn FnMut(web_sys::Event)>::new(|event: web_sys::Event| {
        let Some(target) = event.target().and_then(|v| v.dyn_into::<Element>().ok()) else {
            return;
        };
        if let Ok(Some(button)) = target.closest("[data-action]") {
            event.prevent_default();
            dispatch(
                button.get_attribute("data-action").unwrap_or_default(),
                button.get_attribute("data-id").unwrap_or_default(),
            );
        }
    });
    document().add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
    click.forget();
    let submit = Closure::<dyn FnMut(web_sys::Event)>::new(|event: web_sys::Event| {
        event.prevent_default();
        let Some(form) = event.target().and_then(|v| v.dyn_into::<Element>().ok()) else {
            return;
        };
        dispatch(
            form.get_attribute("data-form").unwrap_or_default(),
            String::new(),
        );
    });
    document().add_event_listener_with_callback("submit", submit.as_ref().unchecked_ref())?;
    submit.forget();
    spawn_local(async {
        if let Err(e) = bootstrap().await {
            login_view();
            notice(&e);
        } else {
            notice("서버에 연결됐어요.");
        }
    });
    Ok(())
}
