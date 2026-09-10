mod common;

use axum::http::StatusCode;
use common::{assert_error_envelope, delete, get, json_request, send_json, test_app};
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn configurations_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/configurations")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/configurations",
            &json!({
                "configId": "CFG-001",
                "name": "chrome-linux",
                "browser": "Chrome",
                "os": "Linux",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "chrome-linux.json");

    let (status, listing) = send_json(&app, get("/configurations")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["chrome-linux.json"]));

    let (status, stored) = send_json(&app, get("/configurations/chrome-linux.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["configId"], "CFG-001");
    assert_eq!(stored["browser"], "Chrome");

    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/configurations/chrome-linux.json",
            &json!({
                "configId": "CFG-001",
                "name": "chrome-linux",
                "browser": "Chrome",
                "os": "Linux",
                "resolution": "1920x1080",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, updated) = send_json(&app, get("/configurations/chrome-linux.json")).await;
    assert_eq!(updated["resolution"], "1920x1080");

    let (status, _) = send_json(&app, delete("/configurations/chrome-linux.json")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/configurations/chrome-linux.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_supports_case_insensitive_substring_filtering() {
    let (_directory, app) = test_app();

    for name in ["chrome-linux", "firefox-linux", "chrome-windows"] {
        let (status, _) = send_json(
            &app,
            json_request("POST", "/configurations", &json!({"name": name})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, body) = send_json(&app, get("/configurations?filter=CHROME")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(["chrome-linux.json", "chrome-windows.json"]));

    let (status, body) = send_json(&app, get("/configurations?filter=nonexistent")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn creating_a_configuration_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    for payload in [
        json!({"configId": "CFG-001", "browser": "Chrome"}),
        json!({"name": ""}),
    ] {
        let (status, body) =
            send_json(&app, json_request("POST", "/configurations", &payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{payload}");
        assert_error_envelope(&body, "invalid_request");
    }

    let (status, listed) = send_json(&app, get("/configurations")).await;
    assert_eq!(listed, json!([]), "a rejected body must not be stored");
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn duplicate_configurations_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    let payload = json!({"name": "chrome-linux"});

    let (status, _) = send_json(&app, json_request("POST", "/configurations", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(&app, json_request("POST", "/configurations", &payload)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
}

#[tokio::test]
async fn missing_configurations_return_a_stable_error_envelope() {
    let (_directory, app) = test_app();

    for request in [
        get("/configurations/missing.json"),
        delete("/configurations/missing.json"),
        json_request(
            "PUT",
            "/configurations/missing.json",
            &json!({"name": "missing"}),
        ),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_error_envelope(&body, "not_found");
    }
}

#[tokio::test]
async fn unknown_fields_are_rejected_before_anything_is_persisted() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/configurations",
            &json!({"name": "chrome-linux", "unknownField": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    let (_, listed) = send_json(&app, get("/configurations")).await;
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn stored_configurations_survive_a_repository_restart() {
    let directory = TempDir::new().expect("temp dir");

    {
        let app = common::app_at(directory.path());
        let (status, _) = send_json(
            &app,
            json_request(
                "POST",
                "/configurations",
                &json!({"configId": "CFG-001", "name": "chrome-linux", "browser": "Chrome"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let app = common::app_at(directory.path());
    let (status, stored) = send_json(&app, get("/configurations/chrome-linux.json")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["browser"], "Chrome");
}

#[tokio::test]
async fn configuration_markers_are_plain_json_under_the_data_root() {
    let (directory, app) = test_app();

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/configurations",
            &json!({"configId": "CFG-001", "name": "chrome-linux", "browser": "Chrome"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let marker = directory.path().join("configurations/chrome-linux.json");
    assert!(marker.is_file(), "marker missing at {}", marker.display());

    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&marker).expect("marker readable"))
            .expect("marker is valid JSON");
    assert_eq!(stored["configId"], "CFG-001");
}

#[tokio::test]
async fn hostile_configuration_identifiers_are_rejected_as_client_errors() {
    let (directory, app) = test_app();

    for (request, status) in [
        (
            get("/configurations/..%2Fescape.json"),
            StatusCode::BAD_REQUEST,
        ),
        (get("/configurations/missing.json"), StatusCode::NOT_FOUND),
    ] {
        let (actual, body) = send_json(&app, request).await;
        assert_eq!(actual, status);
        assert_error_envelope(
            &body,
            if status == StatusCode::BAD_REQUEST {
                "invalid_id"
            } else {
                "not_found"
            },
        );
    }

    // A traversal attempt inside the identifier must never reach the filesystem.
    let (status, body) = send_json(
        &app,
        json_request("POST", "/configurations", &json!({"name": "../escape"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
    assert!(!directory.path().join("escape.json").exists());
}

#[tokio::test]
async fn test_runs_can_reference_a_configuration() {
    let (_directory, app) = test_app();

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/configurations",
            &json!({"configId": "CFG-001", "name": "chrome-linux", "browser": "Chrome"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs",
            &json!({
                "testRunId": "R-001",
                "name": "nightly",
                "timestamp": "2026-09-09T00:00:00Z",
                "configurations": [{"configId": "CFG-001", "name": "chrome-linux"}],
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["configurations"][0]["configId"], "CFG-001");
}
