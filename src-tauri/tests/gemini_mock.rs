//! Integration tests: the Gemini client and tool loop against a local mock
//! server (section 10.4). No real network, no real key.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use wisp_lib::agent::tool_loop::{self, LoopEnd, LoopRequest, ToolHost};
use wisp_lib::gemini::client::{ClientEvent, GemError, GeminiClient};
use wisp_lib::secrets::ApiKey;

const MODEL: &str = "gemini-test";
const GEN_PATH: &str = "/v1beta/models/gemini-test:generateContent";

fn key() -> ApiKey {
    ApiKey::new("test-key-0123456789abcdef")
}

fn client(server: &MockServer) -> GeminiClient {
    GeminiClient::new(format!("{}/v1beta", server.uri())).with_retry_base(Duration::from_millis(5))
}

fn text_response(t: &str) -> Value {
    json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": t }] }, "finishReason": "STOP" }] })
}

fn call_response(id: &str, name: &str, args: Value) -> Value {
    json!({ "candidates": [{ "content": { "role": "model", "parts": [
        { "functionCall": { "id": id, "name": name, "args": args }, "thoughtSignature": "sig-abc" }
    ] }, "finishReason": "STOP" }] })
}

async fn generate(c: &GeminiClient) -> Result<Value, GemError> {
    c.generate_content(
        &key(),
        MODEL,
        &json!({ "contents": [] }),
        Duration::from_secs(5),
        &CancellationToken::new(),
    )
    .await
}

#[tokio::test]
async fn success_sends_key_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .and(header("x-goog-api-key", "test-key-0123456789abcdef"))
        .respond_with(ResponseTemplate::new(200).set_body_json(text_response("hello")))
        .expect(1)
        .mount(&server)
        .await;
    let v = generate(&client(&server)).await.unwrap();
    assert_eq!(wisp_lib::gemini::response_text(&v), "hello");
}

/// Responds with a fixed sequence, then repeats the last entry.
struct Sequence {
    responses: Vec<ResponseTemplate>,
    n: AtomicUsize,
}

impl Respond for Sequence {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let i = self
            .n
            .fetch_add(1, Ordering::SeqCst)
            .min(self.responses.len() - 1);
        self.responses[i].clone()
    }
}

#[tokio::test]
async fn retries_429_honouring_retry_after_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(Sequence {
            responses: vec![
                ResponseTemplate::new(429).insert_header("Retry-After", "0"),
                ResponseTemplate::new(200).set_body_json(text_response("ok")),
            ],
            n: AtomicUsize::new(0),
        })
        .expect(2)
        .mount(&server)
        .await;
    let events = Arc::new(Mutex::new(vec![]));
    let ev2 = events.clone();
    let c = client(&server).with_observer(Arc::new(move |e| ev2.lock().unwrap().push(e)));
    let v = generate(&c).await.unwrap();
    assert_eq!(wisp_lib::gemini::response_text(&v), "ok");
    let ev = events.lock().unwrap();
    assert_eq!(
        ev.iter()
            .filter(|e| **e == ClientEvent::RateLimited)
            .count(),
        1
    );
    assert_eq!(ev.iter().filter(|e| **e == ClientEvent::Request).count(), 2);
}

#[tokio::test]
async fn gives_up_after_three_retries_on_429() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(ResponseTemplate::new(429))
        .expect(4)
        .mount(&server)
        .await;
    let err = generate(&client(&server)).await.unwrap_err();
    assert!(matches!(err, GemError::RateLimit));
    assert_eq!(err.code().as_str(), "RATE_LIMIT");
}

#[tokio::test]
async fn retries_5xx() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(Sequence {
            responses: vec![
                ResponseTemplate::new(503),
                ResponseTemplate::new(200).set_body_json(text_response("back")),
            ],
            n: AtomicUsize::new(0),
        })
        .expect(2)
        .mount(&server)
        .await;
    assert!(generate(&client(&server)).await.is_ok());
}

#[tokio::test]
async fn never_retries_other_4xx() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            json!({ "error": { "message": "API key not valid. Please pass a valid API key." } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let err = generate(&client(&server)).await.unwrap_err();
    assert!(matches!(err, GemError::KeyInvalid(400)));
}

#[tokio::test]
async fn missing_model_maps_to_model_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    assert!(matches!(
        generate(&client(&server)).await.unwrap_err(),
        GemError::ModelNotFound
    ));
}

#[tokio::test]
async fn malformed_json_is_an_error_not_a_crash() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string("{not json"))
        .mount(&server)
        .await;
    assert!(matches!(
        generate(&client(&server)).await.unwrap_err(),
        GemError::Malformed
    ));
}

#[tokio::test]
async fn safety_block_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "promptFeedback": { "blockReason": "SAFETY" } })),
        )
        .mount(&server)
        .await;
    let err = generate(&client(&server)).await.unwrap_err();
    assert!(matches!(err, GemError::SafetyBlocked));
    assert_eq!(err.code().island_message(), "Gemini declined this request");
}

#[tokio::test]
async fn timeout_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(text_response("late"))
                .set_delay(Duration::from_secs(3)),
        )
        .mount(&server)
        .await;
    let c = client(&server);
    let err = c
        .generate_content(
            &key(),
            MODEL,
            &json!({}),
            Duration::from_millis(300),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, GemError::Timeout));
}

#[tokio::test]
async fn cancellation_stops_in_flight_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(text_response("late"))
                .set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;
    let c = client(&server);
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        c2.cancel();
    });
    let started = std::time::Instant::now();
    let err = c
        .generate_content(&key(), MODEL, &json!({}), Duration::from_secs(30), &cancel)
        .await
        .unwrap_err();
    assert!(matches!(err, GemError::Cancelled));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "emergency stop must end within 1 s"
    );
}

#[tokio::test]
async fn list_models_follows_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/models"))
        .respond_with(Sequence {
            responses: vec![
                ResponseTemplate::new(200).set_body_json(json!({
                    "models": [{ "name": "models/gemini-a", "displayName": "A", "supportedGenerationMethods": ["generateContent"] }],
                    "nextPageToken": "p2"
                })),
                ResponseTemplate::new(200).set_body_json(json!({ "models": [{ "name": "models/gemini-b" }] })),
            ],
            n: AtomicUsize::new(0),
        })
        .expect(2)
        .mount(&server)
        .await;
    let models = client(&server).list_models(&key()).await.unwrap();
    assert_eq!(
        models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["gemini-a", "gemini-b"]
    );
}

/// Scripted tool host that records calls and fails selected tools.
struct FakeTools {
    calls: Mutex<Vec<(String, Value)>>,
    fail: Vec<&'static str>,
}

impl ToolHost for FakeTools {
    async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        self.calls.lock().unwrap().push((name.to_string(), args));
        if self.fail.contains(&name) {
            Err(format!("{name} failed"))
        } else {
            Ok(json!({ "ok": true }))
        }
    }
}

fn loop_request() -> LoopRequest<'static> {
    LoopRequest {
        model: MODEL,
        system: "test system",
        user_parts: vec![json!({ "text": "open spotify" })],
        tools: wisp_lib::gemini::schema::tools_field(&[wisp_lib::tools::ToolName::OpenApp]),
        timeout: Duration::from_secs(5),
    }
}

/// Records request bodies so the test can check history handling.
struct Scripted {
    responses: Vec<Value>,
    n: AtomicUsize,
    bodies: Arc<Mutex<Vec<Value>>>,
}

impl Respond for Scripted {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        self.bodies
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&req.body).unwrap());
        let i = self
            .n
            .fetch_add(1, Ordering::SeqCst)
            .min(self.responses.len() - 1);
        ResponseTemplate::new(200).set_body_json(self.responses[i].clone())
    }
}

#[tokio::test]
async fn tool_loop_runs_calls_and_returns_text() {
    let server = MockServer::start().await;
    let bodies = Arc::new(Mutex::new(vec![]));
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(Scripted {
            responses: vec![
                call_response("c1", "open_app", json!({ "name": "Spotify" })),
                text_response("Opened Spotify."),
            ],
            n: AtomicUsize::new(0),
            bodies: bodies.clone(),
        })
        .mount(&server)
        .await;
    let host = FakeTools {
        calls: Mutex::new(vec![]),
        fail: vec![],
    };
    let out = tool_loop::run(
        &client(&server),
        &key(),
        loop_request(),
        &host,
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(&out.end, LoopEnd::Text(t) if t == "Opened Spotify."));
    assert_eq!(host.calls.lock().unwrap()[0].1["name"], "Spotify");
    assert_eq!(out.calls.len(), 1);

    // Second request: the model turn is echoed unchanged (thought signature kept)
    // and the function response carries the call id.
    let b = bodies.lock().unwrap();
    let contents = b[1]["contents"].as_array().unwrap();
    assert_eq!(contents[1]["parts"][0]["thoughtSignature"], "sig-abc");
    assert_eq!(contents[2]["parts"][0]["functionResponse"]["id"], "c1");
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["name"],
        "open_app"
    );
    assert_eq!(
        b[0]["tools"][0]["functionDeclarations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(b[0]["systemInstruction"]["parts"][0]["text"], "test system");
}

#[tokio::test]
async fn tool_loop_stops_after_two_consecutive_failures() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(call_response(
            "c",
            "open_app",
            json!({ "name": "Nope" }),
        )))
        .mount(&server)
        .await;
    let host = FakeTools {
        calls: Mutex::new(vec![]),
        fail: vec!["open_app"],
    };
    let out = tool_loop::run(
        &client(&server),
        &key(),
        loop_request(),
        &host,
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(out.end, LoopEnd::ToolFailures(_)));
    assert_eq!(host.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn tool_loop_caps_at_eight_rounds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(call_response(
            "c",
            "open_app",
            json!({ "name": "x" }),
        )))
        .expect(8)
        .mount(&server)
        .await;
    let host = FakeTools {
        calls: Mutex::new(vec![]),
        fail: vec![],
    };
    let out = tool_loop::run(
        &client(&server),
        &key(),
        loop_request(),
        &host,
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(out.end, LoopEnd::RoundLimit));
}

#[tokio::test]
async fn computer_use_interaction_round_trip() {
    use wisp_lib::gemini::computer_use as cu;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/interactions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "int_7", "status": "requires_action",
            "steps": [{ "type": "function_call", "id": "call_1", "name": "click", "arguments": { "x": 500, "y": 999, "intent": "Click Start" } }]
        })))
        .mount(&server)
        .await;
    let body = cu::first_request("gemini-3.8-flash", "Open Notepad", "image/png", b"fake");
    let v = client(&server)
        .create_interaction(
            &key(),
            &body,
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let turn = cu::parse_turn(&v).unwrap();
    assert_eq!(turn.interaction_id, "int_7");
    let action = cu::parse_action(&turn.calls[0].name, &turn.calls[0].args).unwrap();
    assert_eq!(action, cu::CuAction::Click { x: 500, y: 999 });
}
