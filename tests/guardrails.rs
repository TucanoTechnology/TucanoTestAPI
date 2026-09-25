//! The request guardrails (#103): timeout, concurrency cap, and the
//! environment-configured body limit, driven through the real router.
//!
//! Every refusal here is asserted as a contract answer, not just a status:
//! the error envelope names the code, the request id is carried like on any
//! other response, and the saturated answer says when to come back.

mod common;

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    http::{Request, StatusCode, header},
};
use tower::ServiceExt;

use common::{app_at_with_guardrails, send_full, send_json};
use tucano_test::api::{MAX_BODY_BYTES, guardrails::Guardrails};

/// A body that delivers nothing until its sender is dropped, then ends: a
/// client that stalls, and (in the releasing test) a client whose bytes do
/// arrive and whose request is genuinely answered by the handler.
struct EosBody {
    rx: tokio::sync::oneshot::Receiver<Bytes>,
}

impl futures_core::Stream for EosBody {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.rx).poll(cx) {
            Poll::Ready(Ok(bytes)) => Poll::Ready(Some(Ok(bytes))),
            // Sender dropped: the body ends empty, which the JSON extractor
            // takes as the failure it is.
            Poll::Ready(Err(_)) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A POST that stalls in the body until the returned sender is dropped.
fn stalling_post(uri: &str) -> (Request<Body>, tokio::sync::oneshot::Sender<Bytes>) {
    let (tx, rx) = tokio::sync::oneshot::channel::<Bytes>();
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from_stream(EosBody { rx }))
        .expect("stalling request");
    (request, tx)
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).expect("get request")
}

fn post_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .expect("write request")
}

fn guardrails(
    max_body_bytes: usize,
    request_timeout: Option<Duration>,
    max_concurrency: Option<usize>,
) -> Guardrails {
    Guardrails {
        max_body_bytes,
        request_timeout,
        max_concurrency,
    }
}

#[tokio::test]
async fn a_request_past_its_deadline_is_cut_off_with_the_error_envelope() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app_at_with_guardrails(
        directory.path(),
        guardrails(MAX_BODY_BYTES, Some(Duration::from_millis(60)), None),
    );
    let (request, _sender) = stalling_post("/projects");

    let (status, headers, bytes) = send_full(&app, request).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    let document: serde_json::Value = serde_json::from_slice(&bytes).expect("envelope");
    assert_eq!(document["error"]["code"], "request_timeout");
    assert!(
        headers.get("x-request-id").is_some(),
        "the 504 is an ordinary answer with an ordinary id"
    );

    // The guard cut the request off; it did not break the server.
    let (status, health) = send_json(&app, get("/health")).await;
    assert_eq!(status, StatusCode::OK, "{health}");
}

#[tokio::test]
async fn a_saturated_server_refuses_with_503_and_retry_after() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app_at_with_guardrails(directory.path(), guardrails(MAX_BODY_BYTES, None, Some(1)));

    // Occupy the single permit with a stalled write.
    let holder = {
        let app = app.clone();
        tokio::spawn(async move {
            let (request, _sender) = stalling_post("/projects");
            let _ = app.oneshot(request).await;
        })
    };
    tokio::time::sleep(Duration::from_millis(40)).await;

    let (status, headers, bytes) = send_full(&app, get("/health")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        headers.get(header::RETRY_AFTER).map(|v| v.as_bytes()),
        Some(b"1".as_ref()),
        "the refusal says when to come back"
    );
    let document: serde_json::Value = serde_json::from_slice(&bytes).expect("envelope");
    assert_eq!(document["error"]["code"], "service_unavailable");
    assert!(
        headers.get("x-request-id").is_some(),
        "the refusal carries the request id like any other answer"
    );

    // Cancelling the holder returns the permit — the next request is served.
    holder.abort();
    let _ = holder.await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let (status, health) = send_json(&app, get("/health")).await;
    assert_eq!(status, StatusCode::OK, "{health}");
}

#[tokio::test]
async fn a_request_completed_after_a_refusal_releases_its_permit_on_answer() {
    // The same saturation, but this time the stalled body *does* arrive and
    // the request completes normally: the permit must return on the ordinary
    // answer path, not only on cancellation — and the completed request must
    // be genuinely handled (it is answered 400 for the empty body the drop
    // delivers, which only the real handler produces).
    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app_at_with_guardrails(directory.path(), guardrails(MAX_BODY_BYTES, None, Some(1)));

    let (request, sender) = stalling_post("/projects");
    let holder = {
        let app = app.clone();
        tokio::spawn(async move { app.oneshot(request).await.expect("answered") })
    };

    // Saturate: keep asking until the guard refuses, proving the permit is
    // held by the stalled write.
    let mut refused = false;
    for _ in 0..100 {
        let (status, _, _) = send_full(&app, get("/health")).await;
        if status == StatusCode::SERVICE_UNAVAILABLE {
            refused = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(refused, "the stalled write must occupy the only permit");

    // Let the stalled client finish: the body ends, the handler answers, and
    // the request completes — release by completion, not abort.
    drop(sender);
    let response = tokio::time::timeout(Duration::from_secs(5), holder)
        .await
        .expect("the stalled request must come back")
        .expect("task");
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "the answer came from the handler (empty body is invalid JSON), which proves it ran"
    );

    tokio::time::sleep(Duration::from_millis(20)).await;
    let (status, health) = send_json(&app, get("/health")).await;
    assert_eq!(status, StatusCode::OK, "{health}");
}

#[tokio::test]
async fn a_cancelled_request_leaves_the_store_clean_and_writable() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app_at_with_guardrails(
        directory.path(),
        guardrails(MAX_BODY_BYTES, Some(Duration::from_millis(60)), Some(4)),
    );

    let holder = {
        let app = app.clone();
        tokio::spawn(async move {
            let (request, _sender) = stalling_post("/projects");
            let _ = app.oneshot(request).await;
        })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    holder.abort();
    let _ = holder.await;

    // The timeout fired mid-write-path and the client hung up; either way the
    // volume shows no half-written document and takes new work.
    let leftovers: Vec<_> = std::fs::read_dir(directory.path())
        .expect("read data dir")
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.starts_with(".tucano-") && name.ends_with(".tmp")
        })
        .collect();
    assert!(leftovers.is_empty(), "partial write left behind");

    let (status, created) = send_json(
        &app,
        post_json("/projects", r#"{"name":"After the Cancellation"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let (status, listing) = send_json(&app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing,
        serde_json::json!(["After the Cancellation.json"]),
        "the refused write created nothing"
    );

    let (status, ready) = send_json(&app, get("/ready")).await;
    assert_eq!(status, StatusCode::OK, "{ready}");
}

#[tokio::test]
async fn the_body_cap_is_the_configured_one_not_the_compiled_default() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app_at_with_guardrails(directory.path(), guardrails(64, None, None));

    let fat = serde_json::json!({"name": format!("padded-{}", "x".repeat(200))})
        .to_string()
        .into_bytes();
    // A real client declares its length, and the router can then refuse the
    // request before a byte of body is read — the contract's plain answer.
    let request = Request::builder()
        .method("POST")
        .uri("/projects")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, fat.len())
        .body(Body::from(fat))
        .expect("fat request");
    let (status, bytes) = common::send(&app, request).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "length limit exceeded",
        "the plain-text answer the published contract records"
    );

    // The same body under the default cap is accepted — the refusal came from
    // the configured number, not from a handler limit.
    let roomy_directory = tempfile::TempDir::new().expect("temp dir");
    let roomy = app_at_with_guardrails(
        roomy_directory.path(),
        guardrails(MAX_BODY_BYTES, None, None),
    );
    let (status, document) = send_json(
        &roomy,
        post_json(
            "/projects",
            &String::from_utf8_lossy(
                &serde_json::to_vec(
                    &serde_json::json!({"name": format!("padded-{}", "x".repeat(200))}),
                )
                .expect("json"),
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{document}");
}

#[tokio::test]
async fn an_uncapped_deployment_never_refuses_on_concurrency() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app_at_with_guardrails(directory.path(), guardrails(MAX_BODY_BYTES, None, None));

    let mut holders = Vec::new();
    for _ in 0..64 {
        let app = app.clone();
        holders.push(tokio::spawn(async move {
            let (request, _sender) = stalling_post("/projects");
            let _ = app.oneshot(request).await;
        }));
    }
    tokio::time::sleep(Duration::from_millis(60)).await;
    let (status, health) = send_json(&app, get("/health")).await;
    assert_eq!(status, StatusCode::OK, "{health}");
    for holder in holders {
        holder.abort();
    }
}
