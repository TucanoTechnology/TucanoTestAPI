//! The request id: minted or honoured, echoed, carried in the error envelope,
//! and stamped on the request span.

mod common;

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::Value;
use tracing::{
    Metadata,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    subscriber::Subscriber,
};

const HEADER: &str = "x-request-id";

fn with_id(uri: &str, id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(HEADER, id)
        .body(Body::empty())
        .expect("request")
}

fn response_id(headers: &HeaderMap) -> String {
    headers
        .get(HEADER)
        .expect("x-request-id response header")
        .to_str()
        .expect("visible ascii")
        .to_owned()
}

#[tokio::test]
async fn a_request_without_the_header_gets_one() {
    let (_directory, app) = common::test_app();

    let (status, headers, _) = common::send_full(&app, common::get("/health")).await;

    assert_eq!(status, StatusCode::OK);
    let id = response_id(&headers);
    assert_eq!(id.len(), 16, "unexpected id shape: {id}");
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "not hex: {id}");
}

#[tokio::test]
async fn a_request_with_the_header_echoes_it() {
    let (_directory, app) = common::test_app();

    let (status, headers, _) =
        common::send_full(&app, with_id("/health", "client-supplied-42")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response_id(&headers), "client-supplied-42");
}

#[tokio::test]
async fn an_empty_header_is_replaced() {
    let (_directory, app) = common::test_app();

    let (status, headers, _) = common::send_full(&app, with_id("/health", "")).await;

    assert_eq!(status, StatusCode::OK);
    let id = response_id(&headers);
    assert!(
        !id.is_empty(),
        "the empty inbound id should have been replaced"
    );
}

#[tokio::test]
async fn two_requests_get_different_ids() {
    let (_directory, app) = common::test_app();

    let (_, first, _) = common::send_full(&app, common::get("/health")).await;
    let (_, second, _) = common::send_full(&app, common::get("/health")).await;

    assert_ne!(response_id(&first), response_id(&second));
}

#[tokio::test]
async fn an_error_envelope_carries_the_request_id() {
    let (_directory, app) = common::test_app();

    let (status, headers, bytes) =
        common::send_full(&app, common::get("/projects/missing.json")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    let body: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(
        body["error"]["requestId"].as_str().expect("requestId"),
        response_id(&headers),
        "the envelope and the header must agree: {body}"
    );
}

#[tokio::test]
async fn a_plain_text_rejection_carries_the_header() {
    let (_directory, app) = common::test_app();

    let request = common::raw_json_request("POST", "/test_cases/TC-001/attachments", "{}");
    let (status, headers, bytes) = common::send_full(&app, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(common::content_type(&headers).is_some_and(|value| value.starts_with("text/plain")));
    assert!(!bytes.is_empty());
    assert!(!response_id(&headers).is_empty());
}

/// A subscriber that records the `request_id` field of every span created.
#[derive(Clone, Default)]
struct SpanRecorder {
    ids: Arc<Mutex<Vec<String>>>,
}

impl SpanRecorder {
    fn recorded(&self) -> Vec<String> {
        self.ids.lock().expect("lock").clone()
    }
}

#[derive(Default)]
struct IdVisitor {
    id: Option<String>,
}

impl Visit for IdVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "request_id" {
            self.id = Some(format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "request_id" {
            self.id = Some(value.to_owned());
        }
    }
}

impl Subscriber for SpanRecorder {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        static NEXT: AtomicU64 = AtomicU64::new(1);

        let mut visitor = IdVisitor::default();
        span.record(&mut visitor);
        if let Some(id) = visitor.id {
            self.ids.lock().expect("lock").push(id);
        }
        Id::from_u64(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &tracing::Event<'_>) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

/// Installs the recorder as the process-wide subscriber.
///
/// `set_default` is thread-local and the span macro consults a callsite cache
/// that another suite in this binary can already have fixed at `never`, so a
/// process-wide subscriber is the only version that does not depend on test
/// order.
fn span_recorder() -> &'static SpanRecorder {
    static RECORDER: OnceLock<SpanRecorder> = OnceLock::new();

    RECORDER.get_or_init(|| {
        let recorder = SpanRecorder::default();
        tracing::subscriber::set_global_default(recorder.clone())
            .expect("installing the span recorder");
        recorder
    })
}

#[tokio::test]
async fn the_span_carries_the_request_id() {
    let (_directory, app) = common::test_app();
    let recorder = span_recorder();

    let (status, headers, _) = common::send_full(&app, with_id("/health", "span-abc-123")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response_id(&headers), "span-abc-123");
    assert!(
        recorder.recorded().iter().any(|id| id == "span-abc-123"),
        "the request span did not carry the id: {:?}",
        recorder.recorded()
    );
}
