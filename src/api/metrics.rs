//! Request counters in the Prometheus text exposition format.
//!
//! The counters are hand-rolled rather than pulled from an SDK: three labels
//! and one number do not need a metrics crate, and the exposition format is a
//! handful of lines to write. The store is a plain [`Mutex`] around a
//! [`BTreeMap`], which keeps the rendering deterministic — a test can assert
//! the exact text — and costs one uncontended lock per request.
//!
//! Nothing here reads a request body, a header or a stored document. The only
//! labels are the method, the first segment of the *matched* route template,
//! and the status class, so a caller cannot inject a label value through a
//! path, a query string or a body.

use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use axum::{
    extract::{MatchedPath, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use tracing::Span;

use super::AppState;
use crate::storage::Repository;

/// One counter's identity: method, resource and status class.
type Key = (String, String, &'static str);

/// Counts of served requests, split by method, resource and status class.
#[derive(Debug, Default)]
pub struct HttpMetrics {
    counts: Mutex<BTreeMap<Key, u64>>,
}

impl HttpMetrics {
    /// A fresh, empty set of counters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one served request.
    pub fn record(&self, method: &str, resource: &str, status: StatusCode) {
        let key = (method.to_owned(), resource.to_owned(), status_class(status));
        *self.counts().entry(key).or_insert(0) += 1;
    }

    /// Renders every counter in the Prometheus text exposition format.
    ///
    /// Label values are never escaped: they come from the route template and
    /// the status line, never from a request, so they hold no quote, newline or
    /// backslash a Prometheus parser would read as syntax.
    pub fn render(&self) -> String {
        let mut rendered = String::from(
            "# HELP tucano_http_requests_total Requests served, by method, resource and status class.\n\
             # TYPE tucano_http_requests_total counter\n",
        );
        for ((method, resource, class), count) in self.counts().iter() {
            rendered.push_str(&format!(
                "tucano_http_requests_total{{method=\"{method}\",resource=\"{resource}\",status=\"{class}\"}} {count}\n"
            ));
        }
        rendered
    }

    /// Borrows the counters, tolerating a poisoned lock.
    ///
    /// A panic while the lock was held can only have interleaved nothing: the
    /// guard is taken for a single map update that cannot observe a partial
    /// state. Losing the whole metric surface to one panicking request would be
    /// worse than reporting counts an operator can still trust.
    fn counts(&self) -> MutexGuard<'_, BTreeMap<Key, u64>> {
        self.counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The status class a status code belongs to, as Prometheus sees it.
fn status_class(status: StatusCode) -> &'static str {
    match status.as_u16() / 100 {
        1 => "1xx",
        2 => "2xx",
        3 => "3xx",
        4 => "4xx",
        _ => "5xx",
    }
}

/// Counts a request under the resource its matched route belongs to.
///
/// The layer sits outside the routes, so axum has already resolved the route by
/// the time this runs and [`MatchedPath`] is in the request extensions — the
/// same reason `tests/common` can record a route template from an outer layer.
/// A request that matched nothing is counted as `unmatched`, which is how a
/// client that wanders off the published surface shows up without every
/// unknown path minting its own label.
pub async fn track<R>(State(state): State<AppState<R>>, request: Request, next: Next) -> Response
where
    R: Repository + 'static,
{
    let method = request.method().as_str().to_owned();
    let resource = resource_label(
        request
            .extensions()
            .get::<MatchedPath>()
            .map(MatchedPath::as_str),
    );
    let response = next.run(request).await;
    state
        .metrics()
        .record(&method, &resource, response.status());
    response
}

/// The resource label for a matched route template: its first path segment.
fn resource_label(matched: Option<&str>) -> String {
    matched
        .and_then(|matched| matched.trim_start_matches('/').split('/').next())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("unmatched")
        .to_owned()
}

/// Records the outcome on the span [`super::request_id::request_span`] opened.
///
/// A span is opened before the response exists, so status and latency are
/// declared empty there and filled in here. Recording on the span rather than
/// emitting a second event keeps one log line per request.
pub fn record_outcome(response: &Response, latency: Duration, span: &Span) {
    span.record("status", response.status().as_u16());
    span.record("duration_ms", latency.as_millis() as u64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_split_by_method_resource_and_status_class() {
        let metrics = HttpMetrics::new();
        metrics.record("GET", "projects", StatusCode::OK);
        metrics.record("GET", "projects", StatusCode::OK);
        metrics.record("POST", "projects", StatusCode::CREATED);
        metrics.record("GET", "projects", StatusCode::NOT_FOUND);
        metrics.record("GET", "unmatched", StatusCode::BAD_REQUEST);

        let rendered = metrics.render();
        assert!(rendered.starts_with("# HELP tucano_http_requests_total Requests served"));
        assert!(rendered.contains(
            "tucano_http_requests_total{method=\"GET\",resource=\"projects\",status=\"2xx\"} 2\n"
        ));
        assert!(rendered.contains(
            "tucano_http_requests_total{method=\"POST\",resource=\"projects\",status=\"2xx\"} 1\n"
        ));
        assert!(rendered.contains(
            "tucano_http_requests_total{method=\"GET\",resource=\"projects\",status=\"4xx\"} 1\n"
        ));
        assert!(rendered.contains(
            "tucano_http_requests_total{method=\"GET\",resource=\"unmatched\",status=\"4xx\"} 1\n"
        ));
    }

    #[test]
    fn an_empty_store_renders_the_help_and_type_lines_only() {
        let rendered = HttpMetrics::new().render();
        assert_eq!(rendered.lines().count(), 2);
        assert!(rendered.contains("# TYPE tucano_http_requests_total counter"));
    }

    #[test]
    fn the_resource_is_the_first_segment_of_the_matched_template() {
        assert_eq!(resource_label(Some("/health")), "health");
        assert_eq!(resource_label(Some("/openapi.json")), "openapi.json");
        assert_eq!(
            resource_label(Some("/projects/{id}/test_cases")),
            "projects"
        );
        assert_eq!(resource_label(Some("/")), "unmatched");
        assert_eq!(resource_label(None), "unmatched");
    }

    #[test]
    fn a_status_class_covers_every_hundred() {
        assert_eq!(status_class(StatusCode::CONTINUE), "1xx");
        assert_eq!(status_class(StatusCode::OK), "2xx");
        assert_eq!(status_class(StatusCode::NOT_MODIFIED), "3xx");
        assert_eq!(status_class(StatusCode::IM_A_TEAPOT), "4xx");
        assert_eq!(status_class(StatusCode::INTERNAL_SERVER_ERROR), "5xx");
    }
}
