//! `/reports` — read-only aggregations over the stored tree.

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::{domain::DomainError, models::CoverageReport, storage::Repository};

use super::AppState;

/// Query parameters of the coverage report. `projectId` restricts the report to
/// one project; omitted, the report covers every project.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CoverageQuery {
    project_id: Option<String>,
}

async fn get_coverage<R: Repository>(
    State(service): State<AppState<R>>,
    Query(query): Query<CoverageQuery>,
) -> Result<Json<CoverageReport>, DomainError> {
    Ok(Json(service.coverage_report(query.project_id.as_deref())?))
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new().route("/reports/coverage", get(get_coverage::<R>))
}
