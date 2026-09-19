mod common;

use axum::http::StatusCode;
use common::{
    app_at, assert_error_envelope, create_project, delete, fixture_home, get, json_request,
    project_folder, send_json, test_app,
};
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn configurations_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/configurations")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let home = fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
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
    // The supplied `configId` is ignored: the identity is the derived id, so it
    // cannot name a document other than the one it is stored in.
    assert_eq!(stored["configId"], "chrome-linux.json");
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

    let home = fixture_home(&app).await;
    for name in ["chrome-linux", "firefox-linux", "chrome-windows"] {
        let (status, _) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{home}/configurations"),
                &json!({"name": name}),
            ),
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
async fn a_configuration_created_from_a_name_alone_reads_back_as_its_model() {
    let (_directory, app) = test_app();

    // The body names the configuration and nothing else: no `configId`.
    let home = fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
            &json!({"name": "C1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");
    assert_eq!(created["id"], "C1.json");

    let (status, stored) = send_json(&app, get("/configurations/C1.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["configId"], "C1.json");
    assert_eq!(stored["name"], "C1");
}

#[tokio::test]
async fn a_supplied_config_id_is_ignored_in_favour_of_the_derived_id() {
    let (directory, app) = test_app();

    // Issue #288: a `configId` the body carries is not a second name. It is
    // ignored, so the identity always equals the key the document is listed
    // under and no traversal-shaped value is ever persisted.
    let home = fixture_home(&app).await;
    for (name, supplied) in [("probe", "EXPLICIT"), ("docprobe", "../../etc/passwd")] {
        let (status, created) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{home}/configurations"),
                &json!({"name": name, "configId": supplied}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "creating {name}: {created}");
        assert_eq!(created["id"], format!("{name}.json"));

        let (status, stored) = send_json(&app, get(&format!("/configurations/{name}.json"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(stored["configId"], format!("{name}.json"), "{supplied}");
        assert_eq!(stored["name"], name);

        let marker = directory.path().join(format!(
            "projects/{}/configurations/{name}.json",
            project_folder(&home)
        ));
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&marker).expect("marker readable"))
                .expect("marker is valid JSON");
        assert_eq!(on_disk["configId"], format!("{name}.json"), "{supplied}");
    }

    let (status, listing) = send_json(&app, get("/configurations")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["docprobe.json", "probe.json"]));
}

#[tokio::test]
async fn an_update_cannot_move_a_configuration_identity_away_from_its_id() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
            &json!({"name": "chrome-linux"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/configurations/chrome-linux.json",
            &json!({"configId": "elsewhere.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "updating: {body}");

    let (_, stored) = send_json(&app, get("/configurations/chrome-linux.json")).await;
    assert_eq!(stored["configId"], "chrome-linux.json");

    // The id the body named is still not a document.
    let (status, body) = send_json(&app, get("/configurations/elsewhere.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn creating_a_configuration_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    for payload in [
        json!({"configId": "CFG-001", "browser": "Chrome"}),
        json!({"name": ""}),
    ] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{home}/configurations"),
                &payload,
            ),
        )
        .await;
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

    let home = fixture_home(&app).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
            &payload,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
            &payload,
        ),
    )
    .await;
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

    let home = fixture_home(&app).await;
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
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
        let app = app_at(directory.path());
        let home = fixture_home(&app).await;
        let (status, _) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{home}/configurations"),
                &json!({"configId": "CFG-001", "name": "chrome-linux", "browser": "Chrome"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let app = app_at(directory.path());
    let (status, stored) = send_json(&app, get("/configurations/chrome-linux.json")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["browser"], "Chrome");
}

#[tokio::test]
async fn configuration_markers_are_plain_json_under_the_data_root() {
    let (directory, app) = test_app();

    let home = fixture_home(&app).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
            &json!({"configId": "CFG-001", "name": "chrome-linux", "browser": "Chrome"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let marker = directory.path().join(format!(
        "projects/{}/configurations/chrome-linux.json",
        project_folder(&home)
    ));
    assert!(marker.is_file(), "marker missing at {}", marker.display());

    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&marker).expect("marker readable"))
            .expect("marker is valid JSON");
    assert_eq!(stored["configId"], "chrome-linux.json");
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

    let home = fixture_home(&app).await;

    // A traversal attempt inside the identifier must never reach the filesystem.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
            &json!({"name": "../escape"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
    assert!(
        !directory.path().join("escape.json").exists(),
        "a traversal attempt must never write beside the data root"
    );
    assert!(
        !directory
            .path()
            .join(format!("projects/{}/escape.json", project_folder(&home)))
            .exists(),
        "a traversal attempt must never write inside the project"
    );
}

#[tokio::test]
async fn test_runs_can_reference_a_configuration() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
            &json!({"configId": "CFG-001", "name": "chrome-linux", "browser": "Chrome"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
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

#[tokio::test]
async fn a_partial_update_keeps_the_fields_the_body_leaves_out() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/configurations"),
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

    let (status, body) = send_json(
        &app,
        json_request("PUT", "/configurations/chrome-linux.json", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "updating: {body}");

    let (status, stored) = send_json(&app, get("/configurations/chrome-linux.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["configId"], "chrome-linux.json");
    assert_eq!(stored["name"], "chrome-linux");
    assert_eq!(stored["browser"], "Chrome");
    assert_eq!(stored["os"], "Linux");

    // A field the body carries is replaced; the others survive it.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/configurations/chrome-linux.json",
            &json!({"resolution": "1920x1080"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, stored) = send_json(&app, get("/configurations/chrome-linux.json")).await;
    assert_eq!(stored["resolution"], "1920x1080");
    assert_eq!(stored["browser"], "Chrome");
    assert_eq!(stored["configId"], "chrome-linux.json");
}

/// Deleting a configuration through a project resolves the project the route
/// names, so an identifier two projects hold is removed one home at a time and
/// the other home is left alone.
#[tokio::test]
async fn deleting_a_configuration_through_a_project_resolves_that_project() {
    let (_directory, app) = test_app();

    let alpha = create_project(&app, "alpha").await;
    let beta = create_project(&app, "beta").await;
    for project in [&alpha, &beta] {
        let (status, created) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{project}/configurations"),
                &json!({"name": "chrome-linux", "browser": "Chrome", "os": "Linux"}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "creating in {project}: {created}"
        );
        assert_eq!(created["id"], "chrome-linux.json");
    }

    // While two projects hold the identifier, the global route refuses it: a
    // bare identifier cannot say which occurrence was meant.
    let (status, body) = send_json(&app, delete("/configurations/chrome-linux.json")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_error_envelope(&body, "conflict");

    let (status, body) = send_json(
        &app,
        delete(&format!(
            "/projects/{alpha}/configurations/chrome-linux.json"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Test configuration deleted");

    let (status, owned) = send_json(&app, get(&format!("/projects/{alpha}/configurations"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owned, json!([]), "the named project lost the occurrence");
    let (status, owned) = send_json(&app, get(&format!("/projects/{beta}/configurations"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        owned,
        json!(["chrome-linux.json"]),
        "the other home is untouched"
    );
    let (status, stored) = send_json(&app, get("/configurations/chrome-linux.json")).await;
    assert_eq!(status, StatusCode::OK, "one home resolves again: {stored}");
    assert_eq!(stored["browser"], "Chrome");

    // The occurrence this project owned is gone, so a second delete has nothing
    // left to remove.
    let (status, body) = send_json(
        &app,
        delete(&format!(
            "/projects/{alpha}/configurations/chrome-linux.json"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_error_envelope(&body, "not_found");
}
