//! Concurrency stress tests for the advisory lock and OCC.
//!
//! These tests exercise the server under concurrent load: lost-update
//! prevention via ETag/If-Match (OCC, #263), concurrent creates to
//! different documents, concurrent creates of the *same* document,
//! mixed read/write safety, cross-resource lock contention, and
//! sustained lock-contention latency.
//!
//! Every test uses [`tokio::spawn`] so the async router processes
//! requests concurrently through the shared advisory lock.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{assert_error_envelope, get, json_request, send_full, send_json, test_app};
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

/// Creates to *one* identifier race for the same location: the existence
/// check and the write happen under one lock, so exactly one request answers
/// `201` and every other answers `409` rather than overwriting the winner.
///
/// Regression test for the TOCTOU in issue #322: with the check performed
/// before the lock, two creates could both observe an empty location and both
/// answer `201`, silently discarding every title but one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_identical_creates_return_409() {
    let (_dir, app) = test_app();
    let app = Arc::new(app);

    let project = common::create_named(&app, "/projects", "race").await;
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
                "testCaseId": "RACE-1",
                "title": format!("case from thread {i}"),
                "expectedResult": "pass"
            });
            send_json(&app, json_request("POST", &uri, &body)).await
        }));
    }

    let mut created = 0usize;
    let mut conflicts = 0usize;
    for handle in handles {
        let (status, body) = handle.await.expect("task");
        match status {
            StatusCode::CREATED => {
                created += 1;
                assert_eq!(body["id"], "RACE-1", "body: {body}");
            }
            StatusCode::CONFLICT => {
                conflicts += 1;
                assert_error_envelope(&body, "conflict");
            }
            other => panic!("unexpected status {other}: {body}"),
        }
    }

    assert_eq!(created, 1, "exactly one create may win the race");
    assert_eq!(conflicts, n - 1, "every other create must answer 409");

    // The winner's document is the one on disk: the loser's bodies were
    // refused, not written and then quietly forgotten.
    let (status, body) = send_json(&app, get("/test_cases/RACE-1")).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        body["testCaseId"], "RACE-1",
        "the stored case must be the one that won"
    );
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

/// #406: concurrent result records for *different* cases of one run must all
/// survive. Before the transaction fix, each mutation read the run document
/// outside the lock and wrote it back inside a fresh one, so the slower writer
/// clobbered the faster one's acknowledged result — two 2xx responses, one
/// lost fact. `mutate_run` makes read+mutate+write one lock acquisition.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_results_for_different_cases_all_survive() {
    let (_dir, app) = test_app();
    let app = Arc::new(app);

    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "race-project"})),
    )
    .await;
    for id in ["TC-race-1", "TC-race-2"] {
        send_json(
            &app,
            json_request(
                "POST",
                "/projects/race-project.json/test_cases",
                &json!({"testCaseId": id, "title": id, "expectedResult": "ok"}),
            ),
        )
        .await;
    }
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/projects/race-project.json/test_runs",
            &json!({
                "name": "race-run",
                "testCases": [
                    {"testCaseId": "TC-race-1", "title": "TC-race-1", "expectedResult": "ok"},
                    {"testCaseId": "TC-race-2", "title": "TC-race-2", "expectedResult": "ok"}
                ]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let mut handles = Vec::new();
    for round in 0..4 {
        let app = Arc::clone(&app);
        let (case, outcome) = match round % 2 {
            0 => ("TC-race-1", "Passed"),
            _ => ("TC-race-2", "Failed"),
        };
        handles.push(tokio::spawn(async move {
            for _ in 0..12 {
                let (code, body) = send_json(
                    &app,
                    json_request(
                        "POST",
                        "/test_runs/race-run.json/results",
                        &json!({"testCaseId": case, "status": outcome}),
                    ),
                )
                .await;
                if code == StatusCode::CREATED || code == StatusCode::OK {
                    return;
                }
                assert_eq!(
                    body.pointer("/error/code").and_then(Value::as_str),
                    Some("lock_timeout"),
                    "only lock contention may refuse this POST: {code} {body}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("result for {case} never succeeded under contention");
        }));
    }
    for handle in handles {
        handle.await.expect("task");
    }

    let (_status, run) = send_json(&app, get("/test_runs/race-run.json")).await;
    let results = run["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2, "both cases must hold a result: {run}");
    for case in ["TC-race-1", "TC-race-2"] {
        assert!(
            results
                .iter()
                .any(|result| result["testCaseId"] == case && result["status"] != "Untested"),
            "{case} lost its concurrently recorded result: {run}"
        );
    }
}

/// #406: two concurrent duplicates of the same source under the same `newId`
/// used to race an unlocked `exists_at` against an unconditional overwrite —
/// both answered 201 and one document silently replaced the other. `create_at`
/// makes check-and-write one acquisition: exactly one winner, one conflict.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_duplicates_of_one_new_id_have_exactly_one_winner() {
    let (_dir, app) = test_app();
    let app = Arc::new(app);
    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "dupe-source"})),
    )
    .await;

    let mut handles = Vec::new();
    for _ in 0..8 {
        let app = Arc::clone(&app);
        handles.push(tokio::spawn(async move {
            let (code, body) = send_json(
                &app,
                json_request(
                    "POST",
                    "/projects/dupe-source.json/duplicate",
                    &json!({"newId": "dupe-copy.json", "name": "dupe-copy"}),
                ),
            )
            .await;
            (code, body)
        }));
    }

    let mut created = 0;
    let mut conflicts = 0;
    for handle in handles {
        let (code, body) = handle.await.expect("task");
        match code.as_u16() {
            201 => created += 1,
            409 => {
                assert_eq!(
                    body.pointer("/error/code").and_then(Value::as_str),
                    Some("conflict")
                );
                conflicts += 1;
            }
            other => panic!("unexpected {other} {body} from racing duplicate"),
        }
    }
    assert_eq!(created, 1, "exactly one duplicate may create the target");
    assert_eq!(conflicts, 7, "the losers must all conflict");

    // The single copy is a complete document, readable exactly once.
    let (_status, listing) = send_json(&app, get("/projects")).await;
    let names: Vec<_> = listing
        .as_array()
        .expect("list")
        .iter()
        .map(Value::as_str)
        .collect();
    assert!(names.contains(&Some("dupe-copy.json")), "{names:?}");
}

/// The regression lock for #406's transactional revision: eight writers
/// racing a qualifying update (no `If-Match` — the legacy path every
/// deployment still uses) must produce eight acknowledged updates, eight
/// gapless snapshot versions, and every writer's body must survive in
/// exactly one revision. Before #406 the middle of the read-modify-write
/// ran unlocked and silently DISCARDED snapshots (S2 pass entry 24:
/// 20 acknowledged, 11 kept). #421 pins the failure direction directly.
#[tokio::test]
async fn racing_qualifying_updates_leave_a_gapless_revision_history() {
    let (_directory, app) = test_app();
    let project = common::create_project(&app, "race-history").await;
    common::create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-race").await;
    let app = Arc::new(app);
    let barrier = Arc::new(tokio::sync::Barrier::new(8));

    let mut handles = Vec::new();
    for i in 0..8 {
        let app = Arc::clone(&app);
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            send_json(
                &app,
                json_request(
                    "PUT",
                    "/test_cases/TC-race",
                    &json!({"title": format!("W{i}")}),
                ),
            )
            .await
        }));
    }
    for handle in handles {
        let (status, body) = handle.await.expect("writer");
        assert_eq!(status, StatusCode::OK, "acknowledged: {body}");
    }

    let (_, document) = send_json(&app, get("/test_cases/TC-race")).await;
    assert_eq!(
        document["version"], 9,
        "eight qualifying updates on top of the created v1: {document}"
    );

    let (_, history) = send_json(&app, get("/test_cases/TC-race/history")).await;
    let mut versions: Vec<u64> = history
        .as_array()
        .expect("history array")
        .iter()
        .filter_map(|entry| entry["version"].as_u64())
        .collect();
    versions.sort_unstable();
    assert_eq!(
        versions,
        (1_u64..=8).collect::<Vec<_>>(),
        "gapless snapshot run: {history}"
    );

    // Every writer's body reads back from exactly one revision — the
    // surviving chain is v1(create) → … → v9, so all eight titles appear
    // across the snapshots plus the live document.
    let mut surviving = vec![document["title"].as_str().expect("live title").to_owned()];
    for version in 1..=8_u64 {
        let (status, snapshot) =
            send_json(&app, get(&format!("/test_cases/TC-race/history/{version}"))).await;
        assert_eq!(status, StatusCode::OK, "revision {version} reads back");
        surviving.push(
            snapshot["title"]
                .as_str()
                .expect("snapshot title")
                .to_owned(),
        );
    }
    let writers: Vec<_> = surviving
        .iter()
        .filter(|t| t.starts_with('W'))
        .cloned()
        .collect();
    let distinct: std::collections::BTreeSet<_> = writers.iter().cloned().collect();
    assert_eq!(
        distinct.len(),
        8,
        "every acknowledged writer survives in exactly one revision, saw {writers:?}"
    );
    assert_eq!(writers.len(), 8, "and appears exactly once: {writers:?}");
}
