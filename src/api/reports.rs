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
    models::{CoverageReport, SummaryReport},
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

async fn get_coverage<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Query(query): Query<CoverageQuery>,
) -> Result<Json<CoverageReport>, DomainError> {
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
}

async fn get_summary<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Query(query): Query<SummaryQuery>,
) -> Result<Json<SummaryReport>, DomainError> {
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
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        .route("/reports/coverage", get(get_coverage::<R>))
        .route("/reports/summary", get(get_summary::<R>))
}
