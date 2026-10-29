//! 솔라피 문자 발송 요청 모양 (인증 헤더·본문) against a local stand-in server.
//! Own test binary: it sets process-wide SMS_* variables.
use audeniq_core::{error::Error, sms::SmsSender};
use axum::{Json, Router, http::HeaderMap, http::StatusCode, routing::post};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn solapi_send() {
    let seen: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let log = seen.clone();
    let app = Router::new().route(
        "/messages/v4/send",
        post(move |h: HeaderMap, Json(body): Json<Value>| {
            let log = log.clone();
            async move {
                let auth = h["authorization"].to_str().unwrap().to_string();
                let refuse = body["message"]["to"] == "01000000000";
                log.lock().unwrap().push((auth, body));
                if refuse {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"errorCode": "ValidationError"})),
                    )
                } else {
                    (
                        StatusCode::OK,
                        Json(json!({"statusCode": "2000", "messageId": "M1"})),
                    )
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // SAFETY: single test in this binary.
    unsafe {
        std::env::remove_var("APP_ENV");
        std::env::set_var("SMS_PROVIDER", "solapi");
        std::env::set_var("SMS_FIXTURE_BASE", format!("http://127.0.0.1:{port}"));
        std::env::remove_var("SOLAPI_API_KEY");
    }
    // Incomplete credentials: not ready, nothing sent.
    assert!(!SmsSender::from_env().ready());
    unsafe {
        std::env::set_var("SOLAPI_API_KEY", "TESTKEY");
        std::env::set_var("SOLAPI_API_SECRET", "test-secret");
        std::env::set_var("SMS_FROM", "02-1234-5678");
    }
    let sender = SmsSender::from_env();
    assert!(sender.ready());
    sender
        .send("01012345678", "[AUDENIQ] 인증번호 123456")
        .await
        .unwrap();
    {
        let seen = seen.lock().unwrap();
        let (auth, body) = &seen[0];
        assert_eq!(
            body,
            &json!({"message": {"to": "01012345678", "from": "0212345678", "text": "[AUDENIQ] 인증번호 123456"}})
        );
        assert!(
            auth.starts_with("HMAC-SHA256 apiKey=TESTKEY, date="),
            "{auth}"
        );
        assert!(!auth.contains("test-secret"), "secret never sent");
        let sig = auth.rsplit_once("signature=").unwrap().1;
        assert_eq!(sig.len(), 64);
    }
    // The provider refuses: a fixed error code, no detail.
    let err = sender.send("01000000000", "x").await.unwrap_err();
    assert!(
        matches!(err, Error::PolicyGate("SMS_SEND_FAILED")),
        "{err:?}"
    );
}
