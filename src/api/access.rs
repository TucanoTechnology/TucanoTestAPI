//! Authorization: which projects a request touches, and what it takes to touch
//! them.
//!
//! [`crate::api::auth::authorize`] answers one question — does this caller hold
//! this role in this project — and is all a handler whose project is written in
//! its path needs. The rest of the API is harder: a suite names no project, a run
//! names several, and a report names none at all. This module is the place that
//! resolves those requests to the projects behind them and hands the answer to
//! the same single question, so the shape of the rule is written once.
//!
//! Two policies recur and are worth naming:
//!
//! - **Reads and writes reach a project, not a listing.** A caller with no role
//!   in a project never sees its suites, cases, runs, or milestones, and a
//!   listing is filtered to the projects the caller can reach rather than
//!   refused. A system administrator is not filtered: [`scope`] returns `None`,
//!   which every helper treats as "nothing to check".
//! - **A request that names no project is not a loophole.** A run carries the
//!   projects it covers in its own body, and the role is required in every one of
//!   them; a run naming none is readable by any authenticated caller. A milestone
//!   must name at least one project-bearing reference, so a milestone that names
//!   none is a bad request rather than an unguarded document.
//!
//! With `TUCANO_AUTH_REQUIRED` off every helper returns before it resolves
//! anything, so the trusted-network deployment reads no grant and answers no
//! extra 404 — it is the service it was before auth existed.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{
    AppState,
    auth::{AuthState, authorize},
};
use crate::{
    auth::{Principal, Role},
    domain::DomainError,
    storage::{Repository, Resource},
};

/// The projects `principal` may reach, or `None` when there is nothing to check.
///
/// `None` means unrestricted and covers both cases that need no filtering: the
/// deployment that does not enforce auth, and a system administrator. A caller
/// that is restricted but holds no grant at all gets `Some(empty)`, which filters
/// every listing down to `[]`.
pub(crate) fn scope(
    auth: &AuthState,
    principal: &Principal,
) -> Result<Option<BTreeSet<String>>, DomainError> {
    if !auth.config.required || principal.system_admin {
        return Ok(None);
    }
    Ok(Some(
        auth.store
            .projects_for_user(&principal.user_id)?
            .into_iter()
            .collect(),
    ))
}

/// Whether `project` falls inside `scope`. An absent scope reaches everything.
pub(crate) fn allowed(scope: Option<&BTreeSet<String>>, project: &str) -> bool {
    match scope {
        None => true,
        Some(scope) => scope.contains(project),
    }
}

/// Requires `required` in `project`, delegating to the one role check.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller is not entitled, and whatever
/// reading the grants failed with when the store cannot be read.
pub(crate) fn require<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    project: &str,
    required: Role,
) -> Result<(), DomainError> {
    authorize(state.auth(), principal, project, required)
}

/// Requires the system-administrator capability.
///
/// A system administrator is the only identity that may create a project,
/// because the grant that would scope creation cannot name a project yet.
///
/// # Errors
///
/// [`DomainError::Forbidden`] for every caller that is not a system
/// administrator, when authentication is enforced.
pub(crate) fn require_admin(auth: &AuthState, principal: &Principal) -> Result<(), DomainError> {
    if !auth.config.required || principal.system_admin {
        return Ok(());
    }
    Err(DomainError::forbidden(
        "This account needs the system administrator role",
    ))
}

/// The project ids a flat `projects` array carries.
///
/// Entries that are not objects with a string `projectId` are ignored: a
/// malformed reference no more grants access than an absent one does.
fn project_ids(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("projectId").and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

/// The string entries of an array field, in the order they appear.
fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The array `field` of a partial body, falling back to the stored document.
///
/// An update that omits the field keeps the reference set the document had; one
/// that supplies it — even as `null` — replaces it, so clearing the last
/// reference is caught rather than ignored.
fn effective_ids(document: &Value, body: &Value, field: &str) -> Vec<String> {
    string_array(body.get(field).or_else(|| document.get(field)))
}

/// The projects a run's `projects` array names, from a body or a document.
fn effective_run_projects(document: &Value, body: &Value) -> Vec<String> {
    project_ids(body.get("projects").or_else(|| document.get("projects")))
}

/// The projects a milestone's references reach.
///
/// A reference the store cannot resolve is skipped, matching how progress
/// tolerates a deleted suite or run: a dangling reference must not fail a
/// request that the resource itself can still answer.
fn milestone_projects<R: Repository>(
    state: &AppState<R>,
    suites: &[String],
    runs: &[String],
) -> Vec<String> {
    let mut projects = BTreeSet::new();
    for suite in suites {
        if let Ok(project) = state.project_of(Resource::Suites, suite, missing(Resource::Suites)) {
            projects.insert(project);
        }
    }
    for run in runs {
        if let Ok(document) = state.document(Resource::Runs, run) {
            projects.extend(project_ids(document.get("projects")));
        }
    }
    projects.into_iter().collect()
}

/// The projects a resource stored under `id` reaches.
///
/// This is the resolution the guards run before the handler reads the document
/// the request will read next. A flat resource names its own project; a
/// hierarchical one is resolved through [`TestService::project_of`]; a run or a
/// milestone is followed through its references.
fn projects_of<R: Repository>(
    state: &AppState<R>,
    resource: Resource,
    id: &str,
) -> Result<Vec<String>, DomainError> {
    match resource {
        Resource::Projects => Ok(vec![id.to_owned()]),
        Resource::Suites | Resource::Cases => {
            Ok(vec![state.project_of(resource, id, missing(resource))?])
        }
        Resource::Runs => {
            let document = state.document(Resource::Runs, id)?;
            Ok(project_ids(document.get("projects")))
        }
        Resource::Milestones => {
            let document = state.document(Resource::Milestones, id)?;
            Ok(milestone_projects(
                state,
                &string_array(document.get("testSuiteIds")),
                &string_array(document.get("testRunIds")),
            ))
        }
        Resource::Configurations => Ok(Vec::new()),
    }
}

/// Requires `required` in every project of `projects`.
///
/// An empty set is vacuously satisfied: it is the authenticated-only fallback a
/// run that names no project gets.
fn require_every(
    auth: &AuthState,
    principal: &Principal,
    projects: &[String],
    required: Role,
) -> Result<(), DomainError> {
    if !auth.config.required {
        return Ok(());
    }
    for project in projects {
        authorize(auth, principal, project, required)?;
    }
    Ok(())
}

/// Requires `required` in every project a milestone reaches.
///
/// A milestone that reaches none is refused rather than left open, because it
/// names nothing the role could be required in and would otherwise be readable
/// and writable by any authenticated caller.
fn require_milestone(
    auth: &AuthState,
    principal: &Principal,
    projects: &[String],
    required: Role,
) -> Result<(), DomainError> {
    if !auth.config.required || principal.system_admin {
        return Ok(());
    }
    if projects.is_empty() {
        return Err(DomainError::forbidden(
            "This milestone is not linked to any project",
        ));
    }
    require_every(auth, principal, projects, required)
}

/// The role a write to `resource` needs.
fn write_role(resource: Resource) -> Role {
    match resource {
        Resource::Projects | Resource::Milestones => Role::Owner,
        Resource::Suites | Resource::Cases | Resource::Runs | Resource::Configurations => {
            Role::Editor
        }
    }
}

/// The message a missing document of `resource` is reported under.
fn missing(resource: Resource) -> &'static str {
    match resource {
        Resource::Projects => "Project not found",
        Resource::Suites => "Test suite not found",
        Resource::Cases => "Test case not found",
        Resource::Runs => "Test run not found",
        Resource::Milestones => "Milestone not found",
        Resource::Configurations => "Test configuration not found",
    }
}

/// The bad request a milestone that names no project is refused with.
fn milestone_needs_project() -> DomainError {
    DomainError::invalid_request(
        "A milestone must reference at least one project: link it to a test suite or a test run",
    )
}

/// The field a placement body names its source entity by.
fn source_field(resource: Resource) -> Option<&'static str> {
    match resource {
        Resource::Suites => Some("suiteId"),
        Resource::Cases => Some("testCaseId"),
        _ => None,
    }
}

/// Authorizes reading `id` of `resource`.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach the resource, and
/// [`DomainError::NotFound`] when the resource the guard resolves does not
/// exist.
pub(crate) fn guard_get<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
) -> Result<(), DomainError> {
    if !state.auth().config.required {
        return Ok(());
    }
    if resource == Resource::Configurations {
        return Ok(());
    }
    let projects = projects_of(state, resource, id)?;
    if resource == Resource::Milestones {
        return require_milestone(state.auth(), principal, &projects, Role::Viewer);
    }
    require_every(state.auth(), principal, &projects, Role::Viewer)
}

/// Authorizes creating `resource` from `body`.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach every project the
/// body names, and the bad-request answer for a milestone that names none.
pub(crate) fn guard_create<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    body: &Value,
) -> Result<(), DomainError> {
    let auth = state.auth();
    if !auth.config.required {
        return Ok(());
    }
    match resource {
        Resource::Projects => require_admin(auth, principal),
        Resource::Runs => require_every(
            auth,
            principal,
            &project_ids(body.get("projects")),
            Role::Editor,
        ),
        Resource::Milestones => {
            let projects = milestone_projects(
                state,
                &string_array(body.get("testSuiteIds")),
                &string_array(body.get("testRunIds")),
            );
            if projects.is_empty() {
                return Err(milestone_needs_project());
            }
            require_every(auth, principal, &projects, Role::Owner)
        }
        _ => Ok(()),
    }
}

/// Authorizes updating `id` of `resource` with `body`.
///
/// The projects a run or a milestone reaches come from the body when it names
/// them and from the stored document otherwise, so a body that adds a foreign
/// project needs the role there, and one that clears the last reference is
/// refused where a reference is required.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach every project the
/// resource would carry, and the bad-request answer for a milestone stripped of
/// its last reference.
pub(crate) fn guard_update<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
    body: &Value,
) -> Result<(), DomainError> {
    let auth = state.auth();
    if !auth.config.required || resource == Resource::Configurations {
        return Ok(());
    }
    match resource {
        Resource::Runs => {
            let document = state.document(Resource::Runs, id)?;
            let projects = effective_run_projects(&document, body);
            require_every(auth, principal, &projects, Role::Editor)
        }
        Resource::Milestones => {
            let document = state.document(Resource::Milestones, id)?;
            let projects = milestone_projects(
                state,
                &effective_ids(&document, body, "testSuiteIds"),
                &effective_ids(&document, body, "testRunIds"),
            );
            if projects.is_empty() {
                return Err(milestone_needs_project());
            }
            require_every(auth, principal, &projects, Role::Owner)
        }
        _ => {
            let projects = projects_of(state, resource, id)?;
            require_every(auth, principal, &projects, write_role(resource))
        }
    }
}

/// Authorizes deleting `id` of `resource`.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach the resource, and
/// [`DomainError::NotFound`] when it does not exist.
pub(crate) fn guard_delete<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
) -> Result<(), DomainError> {
    let auth = state.auth();
    if !auth.config.required || resource == Resource::Configurations {
        return Ok(());
    }
    let projects = projects_of(state, resource, id)?;
    if resource == Resource::Milestones {
        return require_milestone(auth, principal, &projects, Role::Owner);
    }
    require_every(auth, principal, &projects, write_role(resource))
}

/// Authorizes duplicating `id` of `resource`.
///
/// The copy lands in the source's own project, so the caller needs the source
/// project's write role — or, for a project, the system-administrator
/// capability, because a copy is a new project.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach the source, and
/// [`DomainError::NotFound`] when it does not exist.
pub(crate) fn guard_duplicate<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
) -> Result<(), DomainError> {
    let auth = state.auth();
    if !auth.config.required {
        return Ok(());
    }
    match resource {
        Resource::Projects => require_admin(auth, principal),
        Resource::Milestones => {
            let projects = projects_of(state, resource, id)?;
            require_milestone(auth, principal, &projects, Role::Owner)
        }
        _ => {
            let projects = projects_of(state, resource, id)?;
            require_every(auth, principal, &projects, write_role(resource))
        }
    }
}

/// Authorizes a composition request that names `target` as the parent project.
///
/// A body that places an existing suite or case names it by `suiteId` or
/// `testCaseId`; the caller must hold `required` in the source's project too,
/// because a move removes it from there. A body that names an identifier the
/// store does not hold is asking for a creation — the identifier is the new
/// entity's own — so there is no source project to check.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach the target or an
/// existing source.
pub(crate) fn guard_composition<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    target: &str,
    body: &Value,
    required: Role,
) -> Result<(), DomainError> {
    if !state.auth().config.required {
        return Ok(());
    }
    authorize(state.auth(), principal, target, required)?;
    if let Some(field) = source_field(resource)
        && let Some(id) = body.get(field).and_then(Value::as_str)
    {
        match state.project_of(resource, id, missing(resource)) {
            Ok(project) => authorize(state.auth(), principal, &project, required)?,
            Err(DomainError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Authorizes a removal that names a suite or a case by id rather than by
/// project.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach the item, and
/// [`DomainError::NotFound`] when it does not exist.
pub(crate) fn guard_removal<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
    required: Role,
) -> Result<(), DomainError> {
    if !state.auth().config.required {
        return Ok(());
    }
    authorize_item(state, principal, resource, id, required)
}

/// Authorizes access to a suite or a case named by id.
///
/// Only suites and cases live in the project tree, so this is the one path that
/// can resolve an id to its project. It is used where a route names a suite or a
/// case without naming its project.
fn authorize_item<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
    required: Role,
) -> Result<(), DomainError> {
    let project = state.project_of(resource, id, missing(resource))?;
    authorize(state.auth(), principal, &project, required)
}

/// Authorizes an operation on a run.
///
/// The caller must reach every project the run names; a run that names none is
/// readable and writable by any authenticated caller, which is the decided
/// fallback for a run that covers no project.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach the run, and
/// [`DomainError::NotFound`] when it does not exist.
pub(crate) fn require_run<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    run_id: &str,
    required: Role,
) -> Result<(), DomainError> {
    if !state.auth().config.required {
        return Ok(());
    }
    let document = state.document(Resource::Runs, run_id)?;
    require_every(
        state.auth(),
        principal,
        &project_ids(document.get("projects")),
        required,
    )
}

/// Authorizes a run operation that also names a suite or a case.
///
/// The caller must reach the run's projects and the source's project, so a run
/// in one project cannot be used to pull content out of another. A body that
/// omits the source entirely names nothing to resolve; the service reports the
/// missing field, so the source check is skipped.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when either side is out of reach, and
/// [`DomainError::NotFound`] when the run or the source does not exist.
pub(crate) fn require_run_source<R: Repository>(
    state: &AppState<R>,
    principal: &Principal,
    run_id: &str,
    resource: Resource,
    source_id: Option<&str>,
    required: Role,
) -> Result<(), DomainError> {
    if !state.auth().config.required {
        return Ok(());
    }
    require_run(state, principal, run_id, required)?;
    let Some(source_id) = source_id else {
        return Ok(());
    };
    let project = state.project_of(resource, source_id, missing(resource))?;
    authorize(state.auth(), principal, &project, required)
}

/// Filters a listing to the projects a restricted caller can reach.
///
/// An unrestricted caller (`scope` is `None`) sees the listing unchanged. A
/// restricted one keeps a project by membership, a suite or a case by the
/// project it belongs to, a run whose embedded projects are all reachable and
/// non-empty, and a milestone whose references all resolve to reachable
/// projects. A `configurations` listing is global and is never filtered. An
/// unresolvable entry is dropped rather than failing the listing.
///
/// # Errors
///
/// Whatever reading the entries the filter inspects failed with.
pub(crate) fn filter_list<R: Repository>(
    state: &AppState<R>,
    resource: Resource,
    items: Vec<String>,
    scope: Option<&BTreeSet<String>>,
) -> Result<Vec<String>, DomainError> {
    let Some(reachable) = scope else {
        return Ok(items);
    };
    let mut kept = Vec::with_capacity(items.len());
    for item in items {
        let keep = match resource {
            Resource::Configurations => true,
            Resource::Projects => reachable.contains(&item),
            Resource::Suites | Resource::Cases => state
                .project_of(resource, &item, missing(resource))
                .map(|project| allowed(Some(reachable), &project))
                .unwrap_or(false),
            Resource::Runs => state
                .document(Resource::Runs, &item)
                .map(|document| run_within(&project_ids(document.get("projects")), reachable))
                .unwrap_or(false),
            Resource::Milestones => state
                .document(Resource::Milestones, &item)
                .map(|document| {
                    let projects = milestone_projects(
                        state,
                        &string_array(document.get("testSuiteIds")),
                        &string_array(document.get("testRunIds")),
                    );
                    !projects.is_empty()
                        && projects.iter().all(|project| reachable.contains(project))
                })
                .unwrap_or(false),
        };
        if keep {
            kept.push(item);
        }
    }
    Ok(kept)
}

/// Whether a run's projects all fall inside `reachable`, and it names one.
fn run_within(projects: &[String], reachable: &BTreeSet<String>) -> bool {
    !projects.is_empty() && projects.iter().all(|project| reachable.contains(project))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_scope_reaches_every_project() {
        assert!(allowed(None, "checkout.json"));
    }

    #[test]
    fn a_scope_reaches_only_its_members() {
        let scope: BTreeSet<String> = ["checkout.json".to_owned()].into_iter().collect();
        assert!(allowed(Some(&scope), "checkout.json"));
        assert!(!allowed(Some(&scope), "payments.json"));
    }

    #[test]
    fn an_empty_scope_reaches_nothing() {
        let scope = BTreeSet::new();
        assert!(!allowed(Some(&scope), "checkout.json"));
    }

    #[test]
    fn a_run_within_scope_must_name_a_reachable_project() {
        let reachable: BTreeSet<String> = ["checkout.json".to_owned()].into_iter().collect();
        assert!(run_within(&["checkout.json".to_owned()], &reachable));
        assert!(!run_within(&[], &reachable));
        assert!(!run_within(&["payments.json".to_owned()], &reachable));
        assert!(!run_within(
            &["checkout.json".to_owned(), "payments.json".to_owned()],
            &reachable
        ));
    }

    #[test]
    fn project_ids_ignore_malformed_entries() {
        let value = serde_json::json!([
            { "projectId": "checkout.json" },
            { "projectId": "checkout.json" },
            { "name": "no id" },
            "not an object",
            { "projectId": 7 }
        ]);
        assert_eq!(project_ids(Some(&value)), vec!["checkout.json".to_owned()]);
    }

    #[test]
    fn effective_ids_prefer_the_body_and_accept_a_clear() {
        let document = serde_json::json!({ "testRunIds": ["R-1.json"] });
        let keep = serde_json::json!({});
        assert_eq!(
            effective_ids(&document, &keep, "testRunIds"),
            vec!["R-1.json"]
        );
        let clear = serde_json::json!({ "testRunIds": null });
        assert!(effective_ids(&document, &clear, "testRunIds").is_empty());
        let replace = serde_json::json!({ "testRunIds": ["R-2.json"] });
        assert_eq!(
            effective_ids(&document, &replace, "testRunIds"),
            vec!["R-2.json"]
        );
    }

    #[test]
    fn a_milestone_that_names_no_project_is_a_bad_request() {
        assert!(matches!(
            milestone_needs_project(),
            DomainError::InvalidRequest { .. }
        ));
    }
}
