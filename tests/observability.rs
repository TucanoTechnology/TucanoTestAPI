//! Observability: the request span, the request counters, and the audit trail.
//!
//! These assertions are made against a whole-process capture of every span and
//! event the router emits, installed once for this test binary. That is a
//! stronger claim than asserting on a single log line: the API logs from
//! exactly two places — the `http.request` span and the `tucano.audit` event —
//! so "the capture never holds this secret" is a statement about everything the
//! deployment can write, not about one line of it.

mod common;

use std::{
    collections::BTreeMap,
    fmt::Debug,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use tucano_test::{api::redact::REDACTED, domain::AUDIT_TARGET};

/// The span every request opens, named by `TraceLayer`'s configuration.
const REQUEST_SPAN: &str = "http.request";

/// Which of the three record kinds a captured line is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// A span was created.
    Span,
    /// A field was recorded on an existing span.
    SpanField,
    /// An event was emitted.
    Event,
}

/// One captured record, flattened to the name/value pairs it carried.
#[derive(Clone, Debug)]
struct Line {
    kind: Kind,
    /// The span the record belongs to; `None` for a free-standing event.
    span_id: Option<u64>,
    /// The span or event name.
    name: String,
    /// The target the record was emitted under.
    target: String,
    /// Every field the record carried, rendered as text.
    fields: BTreeMap<String, String>,
}

impl Line {
    /// The value of `name` where the record carried it at creation time.
    fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }
}

/// Every span and event this binary's router emitted.
///
/// A shared subscriber is the only version that does not depend on test order:
/// `set_default` is thread-local, and a callsite another suite already cached
/// would stay silent.
#[derive(Clone, Default)]
struct Capture {
    lines: Arc<Mutex<Vec<Line>>>,
}

impl Capture {
    fn push(&self, line: Line) {
        self.lines.lock().expect("lock").push(line);
    }

    fn snapshot(&self) -> Vec<Line> {
        self.lines.lock().expect("lock").clone()
    }

    /// The span named `name` whose `field` reads `value`.
    fn span(&self, name: &str, field: &str, value: &str) -> Line {
        let found = self.snapshot().into_iter().find(|line| {
            line.kind == Kind::Span && line.name == name && line.field(field) == Some(value)
        });
        found.unwrap_or_else(|| {
            panic!(
                "no {name} span with {field}={value:?} in\n{}",
                self.rendered()
            )
        })
    }

    /// The most recent value recorded for `field` on `span`.
    ///
    /// A field the span did not declare at creation — `status`, `duration_ms`,
    /// and `query` when there is one — arrives as a later record, so it is not
    /// visible on the span's own line.
    fn recorded(&self, span: &Line, field: &str) -> String {
        let recorded = self
            .snapshot()
            .into_iter()
            .filter(|line| line.span_id == span.span_id)
            .filter_map(|line| line.field(field).map(str::to_owned))
            .next_back();
        recorded.unwrap_or_else(|| {
            panic!(
                "{field} was never recorded on {} in\n{}",
                span.name,
                self.rendered()
            )
        })
    }

    /// The audit lines about `id`, which is the only way a mutation names what
    /// it touched.
    fn audits(&self, id: &str) -> Vec<Line> {
        self.snapshot()
            .into_iter()
            .filter(|line| line.target == AUDIT_TARGET && line.field("id") == Some(id))
            .collect()
    }

    /// The whole capture, so a failed assertion can show what was written and a
    /// passing one can prove what was not.
    fn rendered(&self) -> String {
        self.snapshot()
            .into_iter()
            .map(|line| {
                let fields = line
                    .fields
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{:?} {} {} {}", line.kind, line.target, line.name, fields)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Collects a record's fields as text.
///
/// Every typed record — an integer, a boolean — reaches `record_debug` by
/// default, and a `%value` field reaches it as a `format_args!` whose `Debug`
/// rendering is its `Display`, so the two methods here capture every field
/// cleanly whether it was declared as a string or as a number.
#[derive(Default)]
struct Fields {
    fields: BTreeMap<String, String>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_owned(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

impl Subscriber for Capture {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut fields = Fields::default();
        span.record(&mut fields);
        let id = mint_id();
        self.push(Line {
            kind: Kind::Span,
            span_id: Some(id),
            name: span.metadata().name().to_owned(),
            target: span.metadata().target().to_owned(),
            fields: fields.fields,
        });
        Id::from_u64(id)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        self.push(Line {
            kind: Kind::SpanField,
            span_id: Some(span.into_u64()),
            name: String::new(),
            target: String::new(),
            fields: fields.fields,
        });
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.push(Line {
            kind: Kind::Event,
            span_id: None,
            name: event.metadata().name().to_owned(),
            target: event.metadata().target().to_owned(),
            fields: fields.fields,
        });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

/// A span id that is unique within the process, as `Id` requires.
fn mint_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Installs the capture as the process-wide subscriber and returns it.
///
/// Rebuilding the interest cache is what makes the installation independent of
/// whatever the first callsite reached: a callsite cached at `never` before the
/// subscriber existed would never call back into it.
fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();

    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        tracing::subscriber::set_global_default(capture.clone())
            .expect("installing the capture subscriber");
        tracing::callsite::rebuild_interest_cache();
        capture
    })
}

/// A request the capture can find again by the id it carries.
fn with_id(uri: &str, request_id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("x-request-id", request_id)
        .body(Body::empty())
        .expect("request")
}

#[tokio::test]
async fn metrics_report_what_the_deployment_served() {
    let (_directory, app) = common::test_app();

    // The counters are recorded once a response exists, so the request that
    // asks for them cannot be in the rendering it receives.
    let (status, _, _) = common::send_full(&app, common::get("/health")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, headers, bytes) = common::send_full(&app, common::get("/metrics")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        common::content_type(&headers),
        Some("text/plain; version=0.0.4; charset=utf-8")
    );

    let body = String::from_utf8(bytes).expect("the exposition format is text");
    assert!(body.contains("# HELP tucano_http_requests_total Requests served"));
    assert!(body.contains("# TYPE tucano_http_requests_total counter"));
    assert!(
        body.contains(
            "tucano_http_requests_total{method=\"GET\",resource=\"health\",status=\"2xx\"} 1\n"
        ),
        "the served request is missing from the counters: {body}"
    );
    assert!(
        !body.contains("resource=\"metrics\""),
        "a rendering cannot hold the counter it is about to increment: {body}"
    );
}

#[tokio::test]
async fn the_request_span_records_the_outcome_of_the_request() {
    let (_directory, app) = common::test_app();
    let capture = capture();

    let (status, _, _) = common::send_full(&app, with_id("/health", "observability-span")).await;
    assert_eq!(status, StatusCode::OK);

    let span = capture.span(REQUEST_SPAN, "request_id", "observability-span");
    assert_eq!(span.field("method"), Some("GET"));
    assert_eq!(span.field("path"), Some("/health"));
    assert_eq!(capture.recorded(&span, "status"), "200");

    let duration = capture
        .recorded(&span, "duration_ms")
        .parse::<u64>()
        .expect("duration_ms is a number");
    assert!(duration < 60_000, "implausible latency: {duration}ms");
}

#[tokio::test]
async fn a_credential_in_a_query_string_is_redacted_before_it_is_logged() {
    let (_directory, app) = common::test_app();
    let capture = capture();

    // The header is never read: authentication is switched off in this app, so
    // the only way the credential could be logged is the query string.
    let request = Request::builder()
        .uri("/projects?filter=smoke&token=SUPERSECRET-4f2a")
        .header("x-request-id", "observability-redaction")
        .header("authorization", "Bearer SUPERSECRET-4f2a")
        .body(Body::empty())
        .expect("request");
    let (status, _, _) = common::send_full(&app, request).await;
    assert_eq!(status, StatusCode::OK);

    let span = capture.span(REQUEST_SPAN, "request_id", "observability-redaction");
    assert_eq!(span.field("path"), Some("/projects"));
    assert_eq!(
        capture.recorded(&span, "query"),
        format!("filter=smoke&token={REDACTED}")
    );
    assert!(
        !capture.rendered().contains("SUPERSECRET-4f2a"),
        "a credential reached the log capture"
    );
}

#[tokio::test]
async fn a_mutation_is_audited_without_its_body() {
    let (_directory, app) = common::test_app();
    let capture = capture();

    let (status, created) = common::send_json(
        &app,
        common::json_request(
            "POST",
            "/projects",
            &json!({"name": "observability-audit", "tags": ["BODY-ONLY-9c1d"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["id"], "observability-audit.json");

    let audits = capture.audits("observability-audit.json");
    assert_eq!(audits.len(), 1, "unexpected audit lines: {audits:?}");
    let audit = &audits[0];
    assert_eq!(audit.field("action"), Some("create"));
    assert_eq!(audit.field("resource"), Some("project"));
    assert_eq!(audit.field("outcome"), Some("success"));
    assert_eq!(audit.field("code"), None, "a success carries no code");
    assert!(
        !capture.rendered().contains("BODY-ONLY-9c1d"),
        "the request body reached the log capture"
    );
}

#[tokio::test]
async fn a_refused_mutation_is_audited_with_the_code_the_client_saw() {
    let (_directory, app) = common::test_app();
    let capture = capture();

    let (status, body) =
        common::send_json(&app, common::delete("/projects/observability-missing.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "not_found");

    let audits = capture.audits("observability-missing.json");
    assert_eq!(audits.len(), 1, "unexpected audit lines: {audits:?}");
    let audit = &audits[0];
    assert_eq!(audit.field("action"), Some("delete"));
    assert_eq!(audit.field("resource"), Some("project"));
    assert_eq!(audit.field("outcome"), Some("failure"));
    assert_eq!(audit.field("code"), Some("not_found"));
}

#[tokio::test]
async fn an_attachment_is_audited_without_its_contents() {
    let (_directory, app) = common::test_app();
    let capture = capture();

    common::create_test_case(&app, "TC-AUDIT").await;

    let (status, uploaded) = common::send_json(
        &app,
        common::multipart_request(
            "/test_cases/TC-AUDIT/attachments",
            "evidence.txt",
            b"ATTACHMENT-BODY-7e3f",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let filename = uploaded["filename"].as_str().expect("filename");

    let audits = capture.audits(&format!("TC-AUDIT/{filename}"));
    assert_eq!(audits.len(), 1, "unexpected audit lines: {audits:?}");
    let audit = &audits[0];
    assert_eq!(audit.field("action"), Some("attach"));
    assert_eq!(audit.field("resource"), Some("attachment"));
    assert_eq!(audit.field("outcome"), Some("success"));
    assert!(
        !capture.rendered().contains("ATTACHMENT-BODY-7e3f"),
        "an attachment's contents reached the log capture"
    );
}

// --- #417: the 500 answers stay redacted; the CAUSE reaches the log ------

#[tokio::test]
#[cfg(unix)]
async fn a_storage_failure_logs_its_cause_and_answers_the_fixed_envelope() {
    use std::os::unix::fs::PermissionsExt as _;
    // The forced failure is a MODE refusal, and root is above the mode
    // bits: inside the container test jobs (uid 0) the chmod would not
    // stop the write, and this test would assert a 500 that cannot
    // happen there. Honest skip, loudly reported — not a silent pass.
    if rustix::process::getuid().as_raw() == 0 {
        eprintln!("skipping: the test process is root, which ignores mode 0500");
        return;
    }
    let (directory, app) = common::test_app();
    let capture = capture();

    // Make the store unwritable, the way a full or remounted volume does:
    // the next create must fail deep in the storage layer.
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o500))
        .expect("seal the directory");
    let (status, headers, body) = common::send_full(
        &app,
        common::json_request(
            "POST",
            "/projects",
            &json!({"name": "against-a-dead-volume"}),
        ),
    )
    .await;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("reopen the directory");

    let text = String::from_utf8_lossy(&body).into_owned();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{text}");
    assert!(
        text.contains("\"storage_error\"") && text.contains("Storage operation failed"),
        "the published contract is the fixed envelope: {text}"
    );
    assert!(
        !text.contains("Permission") && !text.contains("denied"),
        "the OS-level cause must not reach the client: {text}"
    );

    // The server-side line names the cause and joins the request by id.
    let causes: Vec<_> = capture
        .snapshot()
        .into_iter()
        .filter(|line| line.field("cause").is_some() && line.field("code") == Some("storage_error"))
        .collect();
    assert!(!causes.is_empty(), "the 500 must be logged with its cause");
    let cause = causes
        .last()
        .expect("just asserted")
        .field("cause")
        .expect("field");
    assert!(
        cause.contains("PermissionDenied"),
        "the surviving io kind names the failure: {cause}"
    );
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .expect("the answer carries a request id");
    assert_eq!(
        causes.last().expect("just asserted").field("request_id"),
        Some(request_id),
        "the error line and the client's id join"
    );
}
// --- #416: attributable audit lines and session events --------------------

const OBS_SECRET: &[u8] = b"observability-actor-secret";
const OBS_USERNAME: &str = "obs-actor";
const OBS_PASSWORD: &str = "correct-horse-obs";
const OBS_WRONG: &str = "wrong-horse-obs";
const OBS_USER_ID: &str = "obs-user-1";

/// A router that enforces authentication over an isolated directory, with
/// one known account.
fn enforcing_app() -> (tempfile::TempDir, axum::Router) {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let repository = tucano_test::repository::FileRepository::new(directory.path()).expect("repo");
    let store = tucano_test::auth::AuthStore::new(directory.path()).expect("auth store");
    store
        .insert_user(&tucano_test::auth::User {
            id: OBS_USER_ID.to_owned(),
            username: OBS_USERNAME.to_owned(),
            password_hash: tucano_test::auth::hash_password(OBS_PASSWORD).expect("hash"),
            system_admin: true,
            created_at: 1_700_000_000,
            refresh_tokens: Vec::new(),
        })
        .expect("seed account");
    let config = tucano_test::auth::AuthConfig {
        required: true,
        jwt_secret: Some(OBS_SECRET.to_vec()),
        access_ttl: std::time::Duration::from_secs(900),
        refresh_ttl: std::time::Duration::from_secs(1_209_600),
        bootstrap_username: None,
        bootstrap_password: None,
    };
    (
        directory,
        tucano_test::api::router(
            repository,
            tucano_test::api::auth::AuthState::new(store, config),
        ),
    )
}

async fn sign_in(app: &axum::Router) -> (String, String) {
    let (status, session) = common::send_json(
        app,
        common::json_request(
            "POST",
            "/auth/login",
            &json!({"username": OBS_USERNAME, "password": OBS_PASSWORD}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    (
        session["accessToken"].as_str().expect("token").to_owned(),
        session["refreshToken"]
            .as_str()
            .expect("refresh")
            .to_owned(),
    )
}

fn authorized(method: &str, uri: &str, token: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

#[tokio::test]
async fn audit_lines_and_the_request_span_name_the_acting_subject() {
    let (_directory, app) = enforcing_app();
    let capture = capture();
    let (token, _refresh) = sign_in(&app).await;

    let (status, created) = common::send_json(&app, {
        let mut request = authorized("POST", "/projects", &token, json!({"name": "attributed"}));
        request
            .headers_mut()
            .insert("x-request-id", "actor-span-1".parse().expect("header"));
        request
    })
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let audit = &capture.audits("attributed.json")[0];
    assert_eq!(
        audit.field("user"),
        Some(OBS_USER_ID),
        "the trail must say who wrote: {audit:?}"
    );
    let span = capture.span(REQUEST_SPAN, "request_id", "actor-span-1");
    assert_eq!(
        capture.recorded(&span, "user").as_str(),
        OBS_USER_ID,
        "the request span must carry the subject too"
    );

    // A trusted deployment has no subject; the honest label is "-", not an
    // omission a redaction could hide behind.
    let (_directory, trusted) = common::test_app();
    let (status, created) = common::send_json(
        &trusted,
        common::json_request("POST", "/projects", &json!({"name": "trusted-writer"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        capture.audits("trusted-writer.json")[0].field("user"),
        Some("-"),
        "trusted writes are attributed anonymously, explicitly"
    );
}

#[tokio::test]
async fn session_events_are_audited_and_never_name_a_secret() {
    let (_directory, app) = enforcing_app();
    let capture = capture();

    // A failed sign-in is an event — and the one place the attempted
    // username is recorded, which is what an operator asks after a burst.
    let (status, _) = common::send_json(
        &app,
        common::json_request(
            "POST",
            "/auth/login",
            &json!({"username": OBS_USERNAME, "password": OBS_WRONG}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let failures: Vec<_> = capture
        .audits(OBS_USERNAME)
        .into_iter()
        .filter(|line| line.field("outcome") == Some("failure"))
        .collect();
    assert_eq!(
        failures.len(),
        1,
        "the attempt is audited once: {failures:?}"
    );
    assert_eq!(failures[0].field("action"), Some("login"));
    assert_eq!(failures[0].field("code"), Some("invalid_credentials"));

    let (access, refresh) = sign_in(&app).await;
    let successes: Vec<_> = capture
        .audits(OBS_USERNAME)
        .into_iter()
        .filter(|line| line.field("outcome") == Some("success"))
        .collect();
    assert!(!successes.is_empty(), "a real sign-in is audited");

    // Rotation succeeds once; replaying the spent token is THE theft
    // signal and is audited as `replay`.
    let (status, rotated) = common::send_json(
        &app,
        common::json_request("POST", "/auth/refresh", &json!({"refreshToken": refresh})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let (status, _) = common::send_json(
        &app,
        common::json_request("POST", "/auth/refresh", &json!({"refreshToken": refresh})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "spent tokens die once");
    let refreshes: Vec<_> = capture
        .audits("-")
        .into_iter()
        .filter(|line| line.field("action") == Some("refresh"))
        .collect();
    assert_eq!(
        refreshes.len(),
        2,
        "one rotation and one replay: {refreshes:?}"
    );
    assert_eq!(refreshes[0].field("outcome"), Some("success"));
    assert_eq!(refreshes[1].field("outcome"), Some("replay"));
    assert_eq!(refreshes[1].field("code"), Some("invalid_refresh_token"));

    // Logout names the account and records what really happened.
    let (status, body) = common::send_json(
        &app,
        authorized(
            "POST",
            "/auth/logout",
            &access,
            json!({"refreshToken": rotated["refreshToken"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let logouts = capture
        .audits(OBS_USER_ID)
        .into_iter()
        .filter(|line| line.field("action") == Some("logout"))
        .collect::<Vec<_>>();
    assert_eq!(logouts.len(), 1, "{logouts:?}");
    assert_eq!(logouts[0].field("revoked"), Some("true"));

    // The redaction half: none of the material that WAS in play appears.
    let rendered = capture.rendered();
    for secret in [
        OBS_PASSWORD,
        OBS_WRONG,
        &access,
        &refresh,
        rotated["accessToken"].as_str().expect("token"),
        rotated["refreshToken"].as_str().expect("refresh"),
    ] {
        assert!(
            !rendered.contains(secret),
            "the capture leaked a credential: {secret}"
        );
    }
}
