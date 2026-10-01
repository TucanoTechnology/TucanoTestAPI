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
//!   in a project never sees its suites, cases, runs, milestones, or
//!   configurations, and a listing is filtered to the projects the caller can
//!   reach rather than refused. A system administrator is not filtered: [`scope`]
//!   returns `None`, which every helper treats as "nothing to check".
//! - **A resource is governed by where it lives, and by what it names.** Every
//!   resource has a home project, and a run or a milestone also reaches the
//!   projects its own references name. The role is required in the home *and* in
//!   every project a reference reaches, so a run stored in one project can never
//!   be used to read the embedded snapshots of another. A document that names no
//!   project is governed by its home alone — it is not thereby open to every
//!   caller.
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
    domain::{ChildExpansion, DomainError},
    storage::{Parent, Repository, Resource},
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
pub(crate) fn require<R: Repository + 'static>(
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

/// The projects a milestone's references reach, resolved with the milestone's
/// own project preferred.
///
/// A reference the store cannot resolve is skipped, matching how progress
/// tolerates a deleted suite or run: a dangling reference must not fail a
/// request that the resource itself can still answer. An identifier two projects
/// hold is not dangling, so it is read from `home` when the home holds it and
/// only resolved globally otherwise — a milestone in one project means its own
/// project's `nightly`, not a conflict. Skipping it instead would drop every
/// project that run covers out of the authorization, leaving the milestone
/// governed by its home alone.
fn milestone_projects<R: Repository + 'static>(
    state: &AppState<R>,
    home: &str,
    suites: &[String],
    runs: &[String],
) -> Vec<String> {
    let parent = Parent::Project(home.to_owned());
    let mut projects = BTreeSet::new();
    for suite in suites {
        // A suite the home holds lives in the home, which the caller requires
        // anyway; one it does not hold is resolved globally, as before.
        if state
            .document_in(Resource::Suites, &parent, suite, missing(Resource::Suites))
            .is_ok()
        {
            projects.insert(home.to_owned());
            continue;
        }
        if let Ok(project) = state.project_of(Resource::Suites, suite, missing(Resource::Suites)) {
            projects.insert(project);
        }
    }
    for run in runs {
        let document = state
            .document_in(Resource::Runs, &parent, run, missing(Resource::Runs))
            .or_else(|_| state.document(Resource::Runs, run));
        if let Ok(document) = document {
            projects.extend(project_ids(document.get("projects")));
        }
    }
    projects.into_iter().collect()
}

/// Every project a run or a milestone reaches: the project that stores it, and
/// the projects its own references name.
///
/// The home anchors a document whose references name no project at all, so a run
/// that covers nothing is still governed by someone rather than open to every
/// caller.
///
/// # Errors
///
/// [`DomainError::NotFound`] when nothing holds `id`, and
/// [`DomainError::Conflict`] when two projects do.
fn reachable_projects<R: Repository + 'static>(
    state: &AppState<R>,
    resource: Resource,
    id: &str,
) -> Result<Vec<String>, DomainError> {
    let home = state.project_of(resource, id, missing(resource))?;
    let document = state.document(resource, id)?;
    let references = match resource {
        Resource::Runs => project_ids(document.get("projects")),
        _ => milestone_projects(
            state,
            &home,
            &string_array(document.get("testSuiteIds")),
            &string_array(document.get("testRunIds")),
        ),
    };
    Ok(with_home(home, references))
}

/// Whether every project in `projects` is inside `reachable`.
fn all_within(projects: &[String], reachable: &BTreeSet<String>) -> bool {
    projects.iter().all(|project| reachable.contains(project))
}

/// `projects` plus `home`, the project the document is stored in.
fn with_home(home: String, mut projects: Vec<String>) -> Vec<String> {
    if !projects.contains(&home) {
        projects.push(home);
    }
    projects
}

/// The projects a resource stored under `id` reaches.
///
/// This is the resolution the guards run before the handler reads the document
/// the request will read next. A project names itself; a suite, a case and a
/// configuration are governed by the project that holds them; a run and a
/// milestone additionally by every project their references reach.
fn projects_of<R: Repository + 'static>(
    state: &AppState<R>,
    resource: Resource,
    id: &str,
) -> Result<Vec<String>, DomainError> {
    match resource {
        Resource::Projects => Ok(vec![id.to_owned()]),
        Resource::Suites | Resource::Cases | Resource::Configurations | Resource::Workflows => {
            Ok(vec![state.project_of(resource, id, missing(resource))?])
        }
        Resource::Runs | Resource::Milestones => reachable_projects(state, resource, id),
    }
}

/// Requires `required` in every project of `projects`.
///
/// An empty set is vacuously satisfied, which no longer happens: every resource
/// has a home project, so the set a guard builds always names at least one.
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

/// The role a write to `resource` needs.
fn write_role(resource: Resource) -> Role {
    match resource {
        Resource::Projects | Resource::Milestones => Role::Owner,
        Resource::Suites | Resource::Cases | Resource::Runs | Resource::Configurations | Resource::Workflows => {
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
        Resource::Workflows => "Workflow not found",
    }
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
/// [`DomainError::Forbidden`] when the caller cannot reach the resource,
/// [`DomainError::NotFound`] when the resource the guard resolves does not
/// exist, and [`DomainError::Conflict`] when two projects hold the identifier.
pub(crate) fn guard_get<R: Repository + 'static>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
    expansion: ChildExpansion,
) -> Result<ReadOutcome, DomainError> {
    // The read guard IS the read (#412): resolving the home, reading the
    // document and digesting its bytes used to happen once per concern —
    // three whole-tree scans and two file reads per GET. One resolution now
    // serves authz, the body, and the ETag, and the handler consumes the
    // result instead of reading again. `not_found` preserves the exact 404
    // text each path published before: the resource-named message when the
    // authz resolution is what 404s, the generic one when the read answers
    // for itself (a project, addressed without any resolve, or any resource
    // on a deployment that authorises nothing).
    let not_found = if state.auth().config.required && resource != Resource::Projects {
        missing(resource)
    } else {
        "Resource not found"
    };
    // A project is addressed by its own name, so its guard never needed a
    // resolve to authorise: the OLD order was authorise, then read, and both
    // an existing and a nonexistent project answered 403 to a caller with no
    // grant there. Reading first would turn that into 404-vs-403 — an
    // existence oracle. Projects therefore keep authorise-before-read.
    if state.auth().config.required && resource == Resource::Projects {
        require_every(state.auth(), principal, &[id.to_owned()], Role::Viewer)?;
    }
    let home = state.resolve_home(resource, id, not_found)?;
    let (document, etag) = state.get_and_etag(resource, id, home.as_ref(), not_found, expansion)?;
    if !state.auth().config.required {
        return Ok(ReadOutcome { document, etag });
    }
    let projects = match resource {
        Resource::Projects => vec![id.to_owned()],
        // A run or a milestone reaches several projects; their reference
        // resolution keeps its own path for now (#415 generalises it).
        Resource::Runs | Resource::Milestones => projects_of(state, resource, id)?,
        _ => vec![
            home.as_ref()
                .expect("every stored resource but a project has a home")
                .project()
                .to_owned(),
        ],
    };
    require_every(state.auth(), principal, &projects, Role::Viewer)?;
    Ok(ReadOutcome { document, etag })
}

/// The document a guard already read, digested from the very bytes served.
pub(crate) struct ReadOutcome {
    pub document: Value,
    pub etag: String,
}

/// Authorizes creating `resource` from `body`.
///
/// Only a project is created without a parent, and only a system administrator
/// may create one, because the grant that would scope the creation cannot name
/// a project yet. Every other resource is created inside a parent, so its flat
/// route has nothing but an explanation to give and [`guard_project_create`]
/// authorizes the real creation.
///
/// # Errors
///
/// [`DomainError::Forbidden`] for a caller that is not a system administrator
/// when authentication is enforced.
pub(crate) fn guard_create<R: Repository + 'static>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    _body: &Value,
) -> Result<(), DomainError> {
    let auth = state.auth();
    if !auth.config.required {
        return Ok(());
    }
    match resource {
        Resource::Projects => require_admin(auth, principal),
        _ => Ok(()),
    }
}

/// Authorizes creating `resource` inside the project `project` names.
///
/// The caller needs the resource's write role in that project. A run embeds the
/// projects it covered and a milestone reaches the projects its references live
/// in, so the role is required in every one of those too — a creation may not
/// name a project the caller cannot reach.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach the target project or
/// any project the body names, and whatever resolving a reference failed with.
pub(crate) fn guard_project_create<R: Repository + 'static>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    project: &str,
    body: &Value,
) -> Result<(), DomainError> {
    let auth = state.auth();
    if !auth.config.required {
        return Ok(());
    }
    let required = write_role(resource);
    authorize(auth, principal, project, required)?;
    let named = match resource {
        Resource::Runs => project_ids(body.get("projects")),
        // The project being created in is the milestone's home, so its
        // references resolve with that home preferred.
        Resource::Milestones => milestone_projects(
            state,
            project,
            &string_array(body.get("testSuiteIds")),
            &string_array(body.get("testRunIds")),
        ),
        _ => Vec::new(),
    };
    require_every(auth, principal, &named, required)
}

/// Authorizes updating `id` of `resource` with `body`.
///
/// The projects a run or a milestone reaches come from the body when it names
/// them and from the stored document otherwise, so a body that adds a foreign
/// project needs the role there. The home project is always required, whatever
/// the body says, because it is where the document lives.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach every project the
/// resource would carry, [`DomainError::NotFound`] when it does not exist, and
/// [`DomainError::Conflict`] when two projects hold the identifier.
pub(crate) fn guard_update<R: Repository + 'static>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
    body: &Value,
) -> Result<(), DomainError> {
    let auth = state.auth();
    if !auth.config.required {
        return Ok(());
    }
    match resource {
        Resource::Runs => {
            let home = state.project_of(resource, id, missing(resource))?;
            let document = state.document(resource, id)?;
            let projects = with_home(home, effective_run_projects(&document, body));
            require_every(auth, principal, &projects, Role::Editor)
        }
        Resource::Milestones => {
            let home = state.project_of(resource, id, missing(resource))?;
            let document = state.document(resource, id)?;
            let references = milestone_projects(
                state,
                &home,
                &effective_ids(&document, body, "testSuiteIds"),
                &effective_ids(&document, body, "testRunIds"),
            );
            let projects = with_home(home, references);
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
/// [`DomainError::Forbidden`] when the caller cannot reach the resource,
/// [`DomainError::NotFound`] when it does not exist, and
/// [`DomainError::Conflict`] when two projects hold the identifier.
pub(crate) fn guard_delete<R: Repository + 'static>(
    state: &AppState<R>,
    principal: &Principal,
    resource: Resource,
    id: &str,
) -> Result<(), DomainError> {
    let auth = state.auth();
    if !auth.config.required {
        return Ok(());
    }
    let projects = projects_of(state, resource, id)?;
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
pub(crate) fn guard_duplicate<R: Repository + 'static>(
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
pub(crate) fn guard_composition<R: Repository + 'static>(
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
pub(crate) fn guard_removal<R: Repository + 'static>(
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
fn authorize_item<R: Repository + 'static>(
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
/// The caller must reach the project the run is stored in and every project it
/// names, so a run covering several projects is only reachable by a caller who
/// holds the role in all of them, and a run covering none is still governed by
/// its home.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller cannot reach the run,
/// [`DomainError::NotFound`] when it does not exist, and
/// [`DomainError::Conflict`] when two projects hold the identifier.
pub(crate) fn require_run<R: Repository + 'static>(
    state: &AppState<R>,
    principal: &Principal,
    run_id: &str,
    required: Role,
) -> Result<(), DomainError> {
    if !state.auth().config.required {
        return Ok(());
    }
    let projects = reachable_projects(state, Resource::Runs, run_id)?;
    require_every(state.auth(), principal, &projects, required)
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
pub(crate) fn require_run_source<R: Repository + 'static>(
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

/// Authorizes a run operation that also names a configuration.
///
/// A configuration is not a suite or a case, so it is not resolved like an
/// unnamed member: it is read from the run's own project when that project holds
/// it, and only resolved globally otherwise. An identifier two projects hold is
/// therefore the run's home's copy rather than a conflict, matching how the
/// service itself resolves the reference.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the run or the configuration is out of reach,
/// and [`DomainError::NotFound`] when either does not exist.
pub(crate) fn require_run_configuration<R: Repository + 'static>(
    state: &AppState<R>,
    principal: &Principal,
    run_id: &str,
    config_id: Option<&str>,
    required: Role,
) -> Result<(), DomainError> {
    if !state.auth().config.required {
        return Ok(());
    }
    require_run(state, principal, run_id, required)?;
    let Some(config_id) = config_id else {
        return Ok(());
    };
    let home = state.project_of(Resource::Runs, run_id, missing(Resource::Runs))?;
    let parent = Parent::Project(home.clone());
    let project = if state
        .document_in(
            Resource::Configurations,
            &parent,
            config_id,
            missing(Resource::Configurations),
        )
        .is_ok()
    {
        home
    } else {
        state.project_of(
            Resource::Configurations,
            config_id,
            missing(Resource::Configurations),
        )?
    };
    authorize(state.auth(), principal, &project, required)
}

/// Filters a listing to the projects a restricted caller can reach.
///
/// An unrestricted caller (`scope` is `None`) sees the listing unchanged. A
/// restricted one keeps an entry when every project it reaches is in scope: a
/// project by membership, a suite, a case or a configuration by the project that
/// holds it, a run by its home and the projects it covers, and a milestone by
/// its home and the projects its references reach. An entry that does not
/// resolve — including an identifier two projects hold — is dropped rather than
/// failing the listing.
///
/// # Errors
///
/// Keeps the identifiers a listing may show to `scope`, resolved from the
/// HOMES the listing walk already produced (#414).
///
/// The old filter re-resolved every candidate through a fresh `locate` scan
/// (and runs and milestones read their document twice more), making a
/// restricted listing O(candidates x store). Now the homes ride along with
/// the ids: an identifier is kept when its single home is reachable — and
/// for a run or a milestone, when that home plus every project its
/// references reach is. Multiple distinct homes still drop the identifier
/// as ambiguous, exactly as the per-id resolve did; an unbounded scope sees
/// everything, unchanged.
pub(crate) fn filter_list<R: Repository + 'static>(
    state: &AppState<R>,
    resource: Resource,
    mut items: Vec<(String, Option<Parent>)>,
    scope: Option<&BTreeSet<String>>,
) -> Result<Vec<String>, DomainError> {
    // The walk yields homes in folder order; grouping by identifier needs
    // them beside each other, and callers expect a sorted listing.
    items.sort_by(|(a_id, _), (b_id, _)| a_id.cmp(b_id));
    let Some(reachable) = scope else {
        let mut ids: Vec<String> = items.into_iter().map(|(id, _)| id).collect();
        ids.sort();
        ids.dedup();
        return Ok(ids);
    };
    // Collapse to identifier -> distinct homes, keeping the sorted order of
    // the previous repository listing.
    let mut homes_of: Vec<(String, Vec<Option<Parent>>)> = Vec::new();
    for (id, home) in items {
        match homes_of.last_mut() {
            Some((last, homes)) if *last == id => {
                if !homes.contains(&home) {
                    homes.push(home);
                }
            }
            _ => homes_of.push((id, vec![home])),
        }
    }
    let mut kept = Vec::new();
    for (id, homes) in homes_of {
        let keep = match resource {
            Resource::Projects => reachable.contains(&id),
            // A suite, a case and a configuration answer to the one project
            // holding it; an identifier two homes claim stays as ambiguous
            // as it has always been listed.
            Resource::Suites | Resource::Cases | Resource::Configurations | Resource::Workflows => match &homes[..] {
                [Some(home)] => allowed(Some(reachable), home.project()),
                _ => false,
            },
            // A run or a milestone is governed by its home AND every project
            // its references reach; the document is read once, AT THE HOME,
            // instead of being resolved and read again as before.
            Resource::Runs | Resource::Milestones => match &homes[..] {
                [Some(home)] => {
                    let home_project = home.project().to_owned();
                    match state.document_at(resource, &id, Some(home)) {
                        Ok(document) => {
                            let projects = match resource {
                                Resource::Runs => project_ids(document.get("projects")),
                                _ => milestone_projects(
                                    state,
                                    &home_project,
                                    &string_array(document.get("testSuiteIds")),
                                    &string_array(document.get("testRunIds")),
                                ),
                            };
                            let projects = with_home(home_project, projects);
                            all_within(&projects, reachable)
                        }
                        Err(_) => false,
                    }
                }
                _ => false,
            },
        };
        if keep {
            kept.push(id);
        }
    }
    Ok(kept)
}

/// Whether every project a document reaches falls inside `reachable`.
///
/// Forget every grant for a project being deleted (#408).
///
/// A grant file that outlives its project is a silent access resurrection:
/// authorisation resolves roles from the file, so re-creating the same
/// identifier would restore former members with no grant event anywhere.
/// Runs BEFORE the folder delete so a refused revocation (a busy lock, an
/// unwritable volume) leaves the project untouched and the whole delete
/// retryable, rather than leaving a resurrection behind. Independent of
/// whether auth is enforced: a non-enforcing deployment may store grants the
/// day its operator turns the flag on. Audited like every other mutation.
pub(crate) fn revoke_project_grants<R: Repository + 'static>(
    state: &AppState<R>,
    project_id: &str,
) -> Result<(), DomainError> {
    let outcome = state.auth().store.remove_project_grants(project_id);
    match &outcome {
        // A project that never had a grant file is a no-op, and no-ops do not
        // audit: an operator reading the trail should see revocations, not
        // every delete of a grantless project (#408, observability contract).
        Ok(false) => {}
        Ok(true) => tracing::info!(
            target: "tucano.audit",
            action = "revoke_grants",
            resource = "project",
            id = project_id,
            outcome = "success"
        ),
        Err(_) => tracing::info!(
            target: "tucano.audit",
            action = "revoke_grants",
            resource = "project",
            id = project_id,
            outcome = "failure",
            code = "storage_error"
        ),
    }
    outcome.map(|_deleted| ()).map_err(DomainError::from)
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

    /// The same truth table `reports::run_reachable` pins on the domain side:
    /// the home anchors a run that covers nothing, and one unreachable project
    /// still hides it.
    #[test]
    fn a_run_is_in_scope_when_its_home_and_every_project_it_names_are() {
        let reachable: BTreeSet<String> = ["checkout.json".to_owned()].into_iter().collect();
        let home = || "checkout.json".to_owned();

        // A run that covers no project is governed by its home alone.
        assert!(all_within(&[home()], &reachable));
        // Its home plus a project the caller also reaches.
        assert!(all_within(
            &[home(), "billing.json".to_owned()],
            &["checkout.json".to_owned(), "billing.json".to_owned()]
                .into_iter()
                .collect::<BTreeSet<_>>()
        ));
        // One covered project the caller cannot reach hides the run.
        assert!(!all_within(
            &[home(), "payments.json".to_owned()],
            &reachable
        ));
        // An unreachable home hides it whatever else it covers.
        assert!(!all_within(&["payments.json".to_owned()], &reachable));
        assert!(!all_within(
            &["payments.json".to_owned(), home()],
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
    fn every_project_resource_has_a_write_role() {
        // A milestone is the one content resource an owner must write, and a
        // configuration is now governed by its project like any other content.
        assert_eq!(write_role(Resource::Projects), Role::Owner);
        assert_eq!(write_role(Resource::Milestones), Role::Owner);
        for resource in [
            Resource::Suites,
            Resource::Cases,
            Resource::Runs,
            Resource::Configurations,
        ] {
            assert_eq!(write_role(resource), Role::Editor, "{resource:?}");
        }
    }
}
