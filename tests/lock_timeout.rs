//! The two refusals a contended or stale write can draw, kept apart (#407).
//!
//! Before the typed split, `WouldBlock` meant both "the advisory lock timed
//! out" and "the If-Match digest was stale", and the PUT path rendered the
//! lock timeout as 412 with the io message echoed into the `ETag` header —
//! telling an optimistic client someone had edited the document when nobody
//! had. These tests pin the two answers apart: contention answers 503
//! `lock_timeout`; a genuine mismatch answers 412 carrying the real current
//! digest.

mod common;

use std::{fs::OpenOptions, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use common::{app_at_with_lock_timeout, json_request, send_full, send_json};
use fs2::FileExt;
use serde_json::json;

fn put_with_etag(uri: &str, body: &serde_json::Value, etag: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::IF_MATCH, etag)
        .body(Body::from(body.to_string()))
        .expect("request")
}

fn app(data: &std::path::Path) -> Router {
    // A short lock deadline keeps the contention test honest without making it
    // slow: the held lock below guarantees the timeout path either way.
    app_at_with_lock_timeout(data, Duration::from_millis(300))
}

async fn seed(app: &Router) {
    let (status, body) = send_json(
        app,
        json_request("POST", "/projects", &json!({"name": "contended"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_lock_held_by_another_process_times_out_as_503_not_412() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app(directory.path());
    seed(&app).await;

    // A second opener of the advisory lock, held across the request: exactly
    // the busy-volume shape `TUCANO_LOCK_TIMEOUT_MS` exists to answer.
    let guard = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.path().join(".tucano.lock"))
        .expect("lock file");
    guard.lock_exclusive().expect("hold the lock");

    let blocking_app = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(send_json(
                &blocking_app,
                json_request(
                    "PUT",
                    "/projects/contended.json",
                    &json!({"description": "should not apply"}),
                ),
            ))
    })
    .await
    .expect("task");
    guard.unlock().expect("release");

    let (status, body) = &result;
    assert_eq!(*status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["code"], "lock_timeout");
    // The old defect rendered this as 412; assert it cannot be one.
    assert_ne!(*status, StatusCode::PRECONDITION_FAILED);

    // Release and retry: the refused write left nothing behind.
    let (_status, stored) = send_json(&app, common::get("/projects/contended.json")).await;
    assert!(
        stored["description"].as_str().unwrap_or("").is_empty(),
        "the refused write left nothing behind: {stored}"
    );
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/projects/contended.json",
            &json!({"description": "applied after retry"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn a_genuine_precondition_failure_answers_412_with_the_real_digest() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app(directory.path());
    seed(&app).await;

    let (status, headers, raw) = send_full(
        &app,
        put_with_etag(
            "/projects/contended.json",
            &json!({"description": "stale"}),
            "\"deadbeefdeadbeefdeadbeefdeadbeef\"",
        ),
    )
    .await;
    let body: serde_json::Value = serde_json::from_slice(&raw).expect("envelope");
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
    let etag = headers
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .trim_matches('"')
        .to_owned();
    assert!(
        etag.len() >= 16 && etag.bytes().all(|c| c.is_ascii_hexdigit()),
        "the 412 must offer the actual current digest, not an internal string: {etag}"
    );
    // The published contract renders a 412 under the "conflict" code; what
    // #407 fixes is that the digest now always names the stored document
    // instead of the stringified lock error.
    assert_eq!(body["error"]["code"], "conflict");
}

/// #410: a write parked on a held advisory lock must not starve the
/// executor. Before the blocking-pool change the handler slept on an async
/// worker thread; with a single-worker runtime that sleep blocked every
/// other task — including `/health`, which by design touches no disk.
/// After the change the wait happens on the blocking pool and the executor
/// keeps serving.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn a_parked_writer_leaves_the_executor_serving_health() {
    use std::fs::OpenOptions;
    use std::time::Duration;

    let directory = tempfile::TempDir::new().expect("temp dir");
    let app = app(directory.path());
    let (status, body) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "starved"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let guard = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.path().join(".tucano.lock"))
        .expect("lock file");
    guard.lock_exclusive().expect("hold the lock");

    let writer = {
        let app = app.clone();
        tokio::spawn(async move {
            send_json(
                &app,
                json_request(
                    "PUT",
                    "/projects/starved.json",
                    &json!({"description": "queued"}),
                ),
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let health = tokio::time::timeout(Duration::from_millis(120), async {
        let (status, _) = send_json(&app, common::get("/health")).await;
        status
    })
    .await
    .expect("/health must answer while a writer is parked: executor starvation (#410)");
    assert_eq!(health, StatusCode::OK);

    guard.unlock().expect("release");
    let (status, body) = writer.await.expect("task");
    assert_eq!(status, StatusCode::OK, "{body}");
}
