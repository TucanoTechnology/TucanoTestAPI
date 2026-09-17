//! Concurrency stress tests for the advisory lock and OCC.
//!
//! These tests exercise the server under concurrent load: lost-update
//! prevention via ETag/If-Match (OCC, #263), concurrent creates to
//! different documents, mixed read/write safety, cross-resource lock
//! contention, and sustained lock-contention latency.
//!
//! Every test uses [`tokio::spawn`] so the async router processes
//! requests concurrently through the shared advisory lock.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{get, json_request, send_full, send_json, test_app};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Builds a PUT request with a JSON body and an `If-Match` ETag header.
fn put_with_etag(uri: &str, body: &Value, etag: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::IF_MATCH, etag)
        .body(Body::from(body.to_string()))
        .expect("request")
}

/// Reads an ETag header value from a response, stripping surrounding quotes.
fn extract_etag(headers: &axum::http::HeaderMap) -> String {
    headers
        .get(header::ETAG)
        .expect("GET must return ETag header")
        .to_str()
        .expect("ETag must be valid UTF-8")
        .trim_matches('"')
        .to_owned()
}

/// Sorts a mutable slice and returns (p50, p95, p99, max) in milliseconds.
fn percentiles_ms(latencies: &mut [Duration]) -> (f64, f64, f64, f64) {
    latencies.sort();
    let n = latencies.len();
    let p = |k: usize| latencies[n * k / 100].as_secs_f64() * 1000.0;
    (p(50), p(95), p(99), latencies[n - 1].as_secs_f64() * 1000.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// With OCC active, 32 concurrent updaters that all read the same ETag
/// before writing should see exactly one succeed and 31 receive 412.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_updates_return_412_on_etag_mismatch() {
    let (_dir, app) = test_app();
    let app = Arc::new(app);

    let project = common::create_named(&app, "/projects", "concurrency").await;
    let case_uri = format!("/projects/{project}/test_cases");
    let (_, created) = send_json(
        &app,
        json_request(
            "POST",
            &case_uri,
            &json!({
                "testCaseId": "CC-1",
                "title": "concurrent test",
                "expectedResult": "pass"
            }),
        ),
    )
    .await;
    let case_id = created["id"].as_str().expect("created id");
    let case_path = format!("/test_cases/{case_id}");

    // All threads read the same ETag before any writes start.
    let (status, headers, _) = send_full(&app, get(&case_path)).await;
    assert_eq!(status, StatusCode::OK);
    let etag = extract_etag(&headers);

    let n = 32usize;
    let barrier = Arc::new(tokio::sync::Barrier::new(n));
    let mut handles = Vec::with_capacity(n);

    for i in 0..n {
        let app = Arc::clone(&app);
        let barrier = Arc::clone(&barrier);
        let path = case_path.clone();
        let etag = etag.clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let body = json!({"title": format!("update from thread {i}")});
            send_full(&app, put_with_etag(&path, &body, &etag)).await
        }));
    }

    let mut ok_count = 0usize;
    let mut conflict_count = 0usize;
    for handle in handles {
        let (status, response_headers, _) = handle.await.expect("task");
        match status {
            StatusCode::OK => {
                ok_count += 1;
                assert!(
                    response_headers.get(header::ETAG).is_none(),
                    "a successful PUT should not return an ETag"
                );
            }
            StatusCode::PRECONDITION_FAILED => {
                conflict_count += 1;
                assert!(
                    response_headers.get(header::ETAG).is_some(),
                    "a 412 must carry the current ETag for the client to re-read"
                );
            }
            other => panic!("unexpected status: {other}"),
        }
    }

    assert_eq!(ok_count, 1, "exactly one update should succeed");
    assert_eq!(conflict_count, n - 1, "all others should receive 412");
}

/// Creates to different documents share the global lock but never
/// interfere — all 16 should succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_creates_all_succeed() {
    let (_dir, app) = test_app();
    let app = Arc::new(app);

    let project = common::create_named(&app, "/projects", "batch").await;
    let case_uri = format!("/projects/{project}/test_cases");

    let n = 16usize;
    let barrier = Arc::new(tokio::sync::Barrier::new(n));
    let mut handles = Vec::with_capacity(n);

    for i in 0..n {
        let app = Arc::clone(&app);
        let barrier = Arc::clone(&barrier);
        let uri = case_uri.clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let body = json!({
                "testCaseId": format!("TC-{i:03}"),
                "title": format!("case {i}"),
                "expectedResult": "pass"
            });
            send_json(&app, json_request("POST", &uri, &body)).await
        }));
    }

    let mut ids = Vec::with_capacity(n);
    for handle in handles {
        let (status, body) = handle.await.expect("task");
        assert_eq!(status, StatusCode::CREATED, "body: {body}");
        ids.push(body["id"].as_str().expect("created id").to_owned());
    }

    // Every document is retrievable.
    for id in &ids {
        let (status, _) = send_json(&app, get(&format!("/test_cases/{id}"))).await;
        assert_eq!(status, StatusCode::OK, "case {id} should be retrievable");
    }
}

/// Readers never see malformed JSON from an in-flight atomic write, and
/// writers never deadlock against concurrent readers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_mixed_read_write() {
    let (_dir, app) = test_app();
    let app = Arc::new(app);

    let project = common::create_named(&app, "/projects", "mixed").await;
    let case_uri = format!("/projects/{project}/test_cases");
    let (_, created) = send_json(
        &app,
        json_request(
            "POST",
            &case_uri,
            &json!({
                "testCaseId": "MX-1",
                "title": "mixed test",
                "expectedResult": "pass"
            }),
        ),
    )
    .await;
    let case_id = created["id"].as_str().expect("created id");
    let case_path = format!("/test_cases/{case_id}");

    let readers = 16usize;
    let writers = 4usize;
    let total = readers + writers;
    let barrier = Arc::new(tokio::sync::Barrier::new(total));
    let read_ok = Arc::new(AtomicUsize::new(0));
    let write_ok = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::with_capacity(total);

    // Readers: 20 iterations each, assert every response is valid JSON.
    for _ in 0..readers {
        let app = Arc::clone(&app);
        let barrier = Arc::clone(&barrier);
        let path = case_path.clone();
        let read_ok = Arc::clone(&read_ok);

        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            for _ in 0..20 {
                let (status, _, body) = send_full(&app, get(&path)).await;
                assert_eq!(status, StatusCode::OK);
                let parsed: Result<Value, _> = serde_json::from_slice(&body);
                assert!(parsed.is_ok(), "reader received malformed JSON");
                read_ok.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    // Writers: 5 iterations each, update without If-Match (last-writer-wins).
    for w in 0..writers {
        let app = Arc::clone(&app);
        let barrier = Arc::clone(&barrier);
        let path = case_path.clone();
        let write_ok = Arc::clone(&write_ok);

        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            for i in 0..5 {
                let body = json!({"title": format!("writer {w} iteration {i}")});
                let (status, _) = send_json(&app, json_request("PUT", &path, &body)).await;
                assert_eq!(status, StatusCode::OK);
                write_ok.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for handle in handles {
        handle.await.expect("task");
    }

    assert_eq!(read_ok.load(Ordering::Relaxed), readers * 20);
    assert_eq!(write_ok.load(Ordering::Relaxed), writers * 5);
}

/// Writes to different resources all succeed, but the global lock makes
/// parallel execution slower than serial — the ratio quantifies the
/// contention overhead.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_cross_resource_lock_contention() {
    let (_dir, app) = test_app();
    let app = Arc::new(app);

    let project_a = common::create_named(&app, "/projects", "alpha").await;
    let project_b = common::create_named(&app, "/projects", "beta").await;
    let project_c = common::create_named(&app, "/projects", "gamma").await;

    // Create one case, one run, and one suite to update.
    let (_, case_created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project_a}/test_cases"),
            &json!({
                "testCaseId": "CR-1",
                "title": "cross resource",
                "expectedResult": "pass"
            }),
        ),
    )
    .await;
    let case_id = case_created["id"].as_str().expect("case id");

    let (_, run_created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project_b}/test_runs"),
            &json!({"name": "cross-run"}),
        ),
    )
    .await;
    let run_id = run_created["id"].as_str().expect("run id");

    let (_, suite_created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project_c}/test_suites"),
            &json!({"name": "cross-suite"}),
        ),
    )
    .await;
    let suite_id = suite_created["id"].as_str().expect("suite id");

    let iterations = 10usize;

    // Serial baseline.
    let serial_start = Instant::now();
    for i in 0..iterations {
        let _ = send_json(
            &app,
            json_request(
                "PUT",
                &format!("/test_cases/{case_id}"),
                &json!({"title": format!("serial {i}")}),
            ),
        )
        .await;
        let _ = send_json(
            &app,
            json_request(
                "PUT",
                &format!("/projects/{project_b}/test_runs/{run_id}"),
                &json!({"name": format!("serial-run {i}")}),
            ),
        )
        .await;
        let _ = send_json(
            &app,
            json_request(
                "PUT",
                &format!("/projects/{project_c}/test_suites/{suite_id}"),
                &json!({"name": format!("serial-suite {i}")}),
            ),
        )
        .await;
    }
    let serial_elapsed = serial_start.elapsed();

    // Parallel: three concurrent writers, each doing `iterations` writes.
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let parallel_start = Instant::now();

    let app1 = Arc::new(app.as_ref().clone());
    let b1 = Arc::clone(&barrier);
    let case_path = format!("/test_cases/{case_id}");
    let h1 = tokio::spawn(async move {
        b1.wait().await;
        for i in 0..iterations {
            let _ = send_json(
                &app1,
                json_request(
                    "PUT",
                    &case_path,
                    &json!({"title": format!("parallel {i}")}),
                ),
            )
            .await;
        }
    });

    let app2 = Arc::new(app.as_ref().clone());
    let b2 = Arc::clone(&barrier);
    let run_path = format!("/projects/{project_b}/test_runs/{run_id}");
    let h2 = tokio::spawn(async move {
        b2.wait().await;
        for i in 0..iterations {
            let _ = send_json(
                &app2,
                json_request(
                    "PUT",
                    &run_path,
                    &json!({"name": format!("parallel-run {i}")}),
                ),
            )
            .await;
        }
    });

    let app3 = Arc::new(app.as_ref().clone());
    let b3 = Arc::clone(&barrier);
    let suite_path = format!("/projects/{project_c}/test_suites/{suite_id}");
    let h3 = tokio::spawn(async move {
        b3.wait().await;
        for i in 0..iterations {
            let _ = send_json(
                &app3,
                json_request(
                    "PUT",
                    &suite_path,
                    &json!({"name": format!("parallel-suite {i}")}),
                ),
            )
            .await;
        }
    });

    h1.await.expect("case writer");
    h2.await.expect("run writer");
    h3.await.expect("suite writer");
    let parallel_elapsed = parallel_start.elapsed();

    let ratio = parallel_elapsed.as_secs_f64() / serial_elapsed.as_secs_f64();
    eprintln!(
        "cross-resource contention: serial={serial_elapsed:?} parallel={parallel_elapsed:?} ratio={ratio:.2}x"
    );

    // Parallel should be within a reasonable range of serial — the global
    // lock serialises writes, so parallel won't be dramatically faster, but
    // it should never be more than 3× slower (generous CI tolerance).
    assert!(
        ratio < 3.0,
        "parallel was {ratio:.2}x slower than serial — possible lock starvation"
    );
}

/// Sustained contention: 8 writers hammering the same document for 5
/// seconds. Reports throughput and latency percentiles as a baseline.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_lock_contention_latency() {
    let (_dir, app) = test_app();
    let app = Arc::new(app);

    let project = common::create_named(&app, "/projects", "latency").await;
    let case_uri = format!("/projects/{project}/test_cases");
    let (_, created) = send_json(
        &app,
        json_request(
            "POST",
            &case_uri,
            &json!({
                "testCaseId": "LT-1",
                "title": "latency test",
                "expectedResult": "pass"
            }),
        ),
    )
    .await;
    let case_id = created["id"].as_str().expect("created id");
    let case_path = format!("/test_cases/{case_id}");

    let threads = 8usize;
    let duration = Duration::from_secs(5);
    let barrier = Arc::new(tokio::sync::Barrier::new(threads));
    let ops = Arc::new(AtomicUsize::new(0));
    let all_latencies: Arc<std::sync::Mutex<Vec<Duration>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut handles = Vec::with_capacity(threads);

    for t in 0..threads {
        let app = Arc::clone(&app);
        let barrier = Arc::clone(&barrier);
        let path = case_path.clone();
        let ops = Arc::clone(&ops);
        let latencies = Arc::clone(&all_latencies);

        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let deadline = Instant::now() + duration;
            let mut local_latencies = Vec::new();

            while Instant::now() < deadline {
                let body = json!({"title": format!("thread {t} at {:?}", Instant::now())});
                let start = Instant::now();
                let (status, _) = send_json(&app, json_request("PUT", &path, &body)).await;
                let elapsed = start.elapsed();

                assert_eq!(status, StatusCode::OK);
                ops.fetch_add(1, Ordering::Relaxed);
                local_latencies.push(elapsed);
            }

            latencies.lock().expect("mutex").extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.await.expect("task");
    }

    let total_ops = ops.load(Ordering::Relaxed);
    let mut latencies = all_latencies.lock().expect("mutex").clone();

    assert!(!latencies.is_empty(), "no operations completed");
    let (p50, p95, p99, max) = percentiles_ms(&mut latencies);
    let ops_per_sec = total_ops as f64 / duration.as_secs_f64();

    eprintln!(
        "lock contention baseline ({threads} writers, {duration:?}): \
         {total_ops} ops, {ops_per_sec:.1} ops/s, \
         p50={p50:.1}ms p95={p95:.1}ms p99={p99:.1}ms max={max:.1}ms"
    );

    // Sanity: at least some ops completed, and max latency is bounded.
    assert!(total_ops > 10, "too few operations: {total_ops}");
    assert!(
        max < 10_000.0,
        "max latency {max:.0}ms exceeds 10s — possible deadlock"
    );
}
