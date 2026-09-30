//! `/reports` — read-only aggregations over the stored tree.

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    auth::{Principal, Role},
    domain::{
        DomainError,
        reports::{self, SummaryFilters},
    },
    models::{CoverageReport, LastResultsReport, SummaryReport},
    storage::Repository,
};

use super::{AppState, access};

/// Query parameters of the coverage report. `projectId` restricts the report to
/// one project; omitted, the report covers every project the caller can reach.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CoverageQuery {
    project_id: Option<String>,
}

/// Query parameters of the summary report. Every filter is optional and the
/// ones supplied combine; an empty query summarises every recorded result.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SummaryQuery {
    project_id: Option<String>,
    milestone_id: Option<String>,
    configuration_id: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

/// Query parameters of the last-results report. `projectId` restricts the
/// report to one project exactly as it does for coverage: supplied, the answer
/// covers that project alone and echoes the identifier back; omitted, the
/// report covers every project the caller can reach.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LastResultsQuery {
    project_id: Option<String>,
}

async fn get_coverage<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Query(query): Query<CoverageQuery>,
) -> Result<Json<CoverageReport>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<CoverageReport>, DomainError> {
        let reachable = access::scope(service.auth(), &principal)?;
        let scope = match (query.project_id, reachable) {
            (Some(id), None) => reports::Scope::Project(id),
            (Some(id), Some(_)) => {
                access::require(&service, &principal, &id, Role::Viewer)?;
                reports::Scope::Project(id)
            }
            (None, None) => reports::Scope::All,
            (None, Some(reachable)) => reports::Scope::Projects(reachable.into_iter().collect()),
        };
        Ok(Json(service.coverage_report(scope)?))
    })
    .await
}

async fn get_summary<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Query(query): Query<SummaryQuery>,
) -> Result<Json<SummaryReport>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<SummaryReport>, DomainError> {
        let reachable = access::scope(service.auth(), &principal)?;
        if let Some(id) = query.project_id.as_deref()
            && reachable.is_some()
        {
            access::require(&service, &principal, id, Role::Viewer)?;
        }
        let filters = SummaryFilters {
            project_id: query.project_id,
            milestone_id: query.milestone_id,
            configuration_id: query.configuration_id,
            from: query.from,
            to: query.to,
        };
        let reachable: Option<Vec<String>> = reachable.map(|set| set.into_iter().collect());
        Ok(Json(
            service.summary_report(&filters, reachable.as_deref())?,
        ))
    })
    .await
}

async fn get_last_results<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Query(query): Query<LastResultsQuery>,
) -> Result<Json<LastResultsReport>, DomainError> {
    // The walk reads every run in scope, so it is parked on the blocking
    // pool like the other reports (#410).
    super::on_blocking(move || -> Result<Json<LastResultsReport>, DomainError> {
        let reachable = access::scope(service.auth(), &principal)?;
        // The scope resolves exactly as coverage resolves it — a named
        // project must carry the caller's Viewer grant before its results are
        // read — and the reachable set travels on as the run-level filter:
        // a home the caller can reach can still hold runs whose `projects`
        // snapshot reaches beyond it (#399).
        let scope = match query.project_id {
            Some(id) => {
                if reachable.is_some() {
                    access::require(&service, &principal, &id, Role::Viewer)?;
                }
                reports::Scope::Project(id)
            }
            None => match reachable.as_ref() {
                None => reports::Scope::All,
                Some(reachable) => reports::Scope::Projects(reachable.iter().cloned().collect()),
            },
        };
        let reachable = reachable.map(|set| set.into_iter().collect::<Vec<String>>());
        Ok(Json(
            service.last_results_report(scope, reachable.as_deref())?,
        ))
    })
    .await
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        .route("/reports/coverage", get(get_coverage::<R>))
        .route("/reports/last-results", get(get_last_results::<R>))
        .route("/reports/summary", get(get_summary::<R>))
}
