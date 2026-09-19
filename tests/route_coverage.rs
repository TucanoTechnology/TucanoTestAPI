//! Proves, from what the router actually served, that every operation the
//! contract documents was driven to a successful answer.
//!
//! `tests/service.rs` pairs each documented operation with a covering test, but
//! a source scanner can only prove that a test *named* in that table exists — not
//! that its body ever reaches the route, nor that it asserts a success. This
//! suite closes that gap by reading what the router recorded while the other
//! suites ran: the middleware in `tests/common` appends the registered template
//! of every request that reached a route, and the check below demands a
//! successful answer for each documented operation.
//!
//! What it does not say is *which* test produced the success — only that one in
//! the run did, so a failure here means the whole suite never reached the
//! operation. The table in `tests/service.rs` is what pins an operation to a
//! named test, and the two halves are meant to be read together.
//!
//! libtest gives no order between test binaries, so recording and checking are
//! two passes of one command — `scripts/coverage-check.sh` names the recording
//! with `TUCANO_ROUTE_LOG` for the suites and turns the check on with
//! `TUCANO_ROUTE_LOG_ASSERT` for this one. A run that only records says so and
//! asserts nothing.

mod common;

use axum::http::StatusCode;
use common::{documented_operations, get, send_json, test_app};
use std::collections::BTreeMap;
use std::fs;

/// What the wire showed for one operation.
#[derive(Default)]
struct Observed {
    attempts: usize,
    successes: usize,
}

#[tokio::test]
async fn every_documented_operation_was_served_successfully() {
    let recording = std::env::var("TUCANO_ROUTE_LOG").ok();
    if std::env::var_os("TUCANO_ROUTE_LOG_ASSERT").is_none() {
        eprintln!(
            "route coverage: recording pass only — the suites are still writing \
             {}; run scripts/coverage-check.sh to record and check in one go",
            recording.as_deref().unwrap_or("(no TUCANO_ROUTE_LOG)")
        );
        return;
    }
    let recording = recording.expect("TUCANO_ROUTE_LOG names the recording this check reads");

    let observed = read_recording(&recording);

    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);
    let documented = documented_operations(&document);

    let unreached: Vec<&String> = documented
        .iter()
        .filter(|label| !observed.contains_key(*label))
        .collect();
    let failed: Vec<&String> = documented
        .iter()
        .filter(|label| observed.get(*label).is_some_and(|seen| seen.successes == 0))
        .collect();

    eprintln!(
        "route coverage: {} of {} documented operations answered successfully \
         ({} attempts recorded in total)",
        documented.len() - unreached.len() - failed.len(),
        documented.len(),
        observed.values().map(|seen| seen.attempts).sum::<usize>(),
    );

    assert!(
        unreached.is_empty(),
        "no test reached these documented operations: {unreached:#?}"
    );
    assert!(
        failed.is_empty(),
        "no test drove these documented operations to a successful answer: {failed:#?}"
    );
}

/// The operations the suites drove, keyed by the `method template` label both
/// sides agree on.
///
/// A line names the method, the registered template, and the status the router
/// answered with. Anything else is a malformed recording and fails loudly
/// rather than being skipped, which would quietly shrink the check.
fn read_recording(recording: &str) -> BTreeMap<String, Observed> {
    let recorded = fs::read_to_string(recording)
        .unwrap_or_else(|error| panic!("reading the recording at {recording}: {error}"));

    let mut observed: BTreeMap<String, Observed> = BTreeMap::new();
    for line in recorded.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split(' ');
        let (Some(method), Some(template), Some(status)) =
            (fields.next(), fields.next(), fields.next())
        else {
            panic!("malformed recording line: {line}");
        };
        let status: u16 = status
            .parse()
            .unwrap_or_else(|_| panic!("malformed status in recording line: {line}"));

        let entry = observed.entry(format!("{method} {template}")).or_default();
        entry.attempts += 1;
        if (200..400).contains(&status) {
            entry.successes += 1;
        }
    }
    observed
}
