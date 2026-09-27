//! Reporting: milestone progress, coverage and the summary report.

use super::*;

impl<R: Repository + 'static> TestService<R> {
    // --- reporting -----------------------------------------------------

    /// Reports a milestone's progress from the runs it references.
    ///
    /// A reference means the milestone's own project first, so a run identifier
    /// another project happens to use too still names the intended run. One
    /// that no project holds is skipped and progress recomputes over the runs
    /// that remain; one that two or more hold outside the home is refused,
    /// because an arbitrary pick would report a wrong number.
    pub fn milestone_progress(&self, id: &str) -> Result<MilestoneProgress, DomainError> {
        // Polling endpoint of milestone dashboards, O(referenced runs) per
        // answer (#415): the progress for one milestone is per-caller
        // identical — authorisation happens in the route's guard before this
        // runs — so one key serves it, retired by any local write and aged
        // out across replicas.
        let key = format!("progress|{}", id);
        let value = self.cached_derivations(&key, || {
            serde_json::to_value(self.milestone_progress_uncached(id)?)
                .map_err(|error| DomainError::Internal(error.to_string()))
        })?;
        serde_json::from_value(value).map_err(|error| DomainError::Internal(error.to_string()))
    }

    fn milestone_progress_uncached(&self, id: &str) -> Result<MilestoneProgress, DomainError> {
        let home = self.resolve(Resource::Milestones, id, None, "Milestone not found")?;
        let value = self
            .repository
            .read_at(Resource::Milestones, Some(&home), id)
            .map_err(error::milestone_error)?;
        let milestone: Milestone = serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored milestone JSON is invalid".to_owned()))?;

        let mut runs = Vec::new();
        for run_id in milestone.test_run_ids.as_deref().unwrap_or_default() {
            let run_home =
                match self.resolve(Resource::Runs, run_id, Some(&home), "Test run not found") {
                    Ok(run_home) => run_home,
                    Err(DomainError::NotFound(_)) => continue,
                    Err(error) => return Err(error),
                };
            let Ok(run_value) = self.read_document(
                Resource::Runs,
                Some(&run_home),
                run_id,
                "Test run not found",
            ) else {
                continue;
            };
            // A run that cannot be decoded is skipped rather than failing the
            // whole report; the format-version reader check is #98's.
            let Ok(run) = serde_json::from_value::<TestRun>(run_value) else {
                continue;
            };
            runs.push(run);
        }

        Ok(progress::compute(&milestone, &runs))
    }

    /// Reports how many cases the tree holds, per suite and in total, for the
    /// projects the caller asked for.
    ///
    /// The scope decides which projects are walked: [`reports::Scope::All`]
    /// walks every project, [`reports::Scope::Project`] verifies that one
    /// project exists and walks it, and [`reports::Scope::Projects`] walks the
    /// identifiers as given — its caller already filtered them by reachability,
    /// so a project that has since been deleted contributes nothing rather than
    /// failing the report.
    pub fn coverage_report(&self, scope: reports::Scope) -> Result<CoverageReport, DomainError> {
        let (echo, projects): (Option<String>, Vec<String>) = match scope {
            reports::Scope::All => (
                None,
                self.repository
                    .list(Resource::Projects)
                    .map_err(error::read_error)?,
            ),
            reports::Scope::Project(id) => {
                self.require_parent(&Parent::Project(id.clone()))?;
                (Some(id.clone()), vec![id])
            }
            reports::Scope::Projects(ids) => (None, ids),
        };

        let mut project_cases = Vec::with_capacity(projects.len());
        for project in projects {
            let parent = Parent::Project(project);
            let direct = self
                .repository
                .list_children(&parent, Resource::Cases)
                .map_err(error::read_error)?
                .len();
            let suite_ids = self
                .repository
                .list_children(&parent, Resource::Suites)
                .map_err(error::read_error)?;
            let mut suites = Vec::with_capacity(suite_ids.len());
            for suite_id in suite_ids {
                let document = self
                    .repository
                    .read_at(Resource::Suites, Some(&parent), &suite_id)
                    .map_err(error::read_error)?;
                // A suite's marker records its own name; fall back to the
                // identifier when a legacy document omits it.
                let name = document
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&suite_id)
                    .to_owned();
                let suite = Parent::Suite {
                    project: parent.project().to_owned(),
                    suite: suite_id.clone(),
                };
                let case_count = self
                    .repository
                    .list_children(&suite, Resource::Cases)
                    .map_err(error::read_error)?
                    .len();
                suites.push(reports::SuiteCases {
                    suite_id,
                    name,
                    case_count,
                });
            }
            project_cases.push(reports::ProjectCases { direct, suites });
        }

        Ok(reports::coverage(echo.as_deref(), project_cases))
    }

    /// Reports how the results recorded across the runs in scope split by
    /// status, together with their pass rate and total duration.
    ///
    /// Every filter is optional and the ones supplied combine: a run is only
    /// counted when it satisfies all of them. Runs that cannot be read or
    /// decoded are skipped rather than failing the whole report, matching how
    /// [`Self::milestone_progress`] treats its references.
    ///
    /// `reachable`, when set, is the authorisation filter: a run that names a
    /// project outside it is skipped even though it would otherwise be in scope.
    /// A trusted caller passes `None` and sees every run.
    pub fn summary_report(
        &self,
        filters: &reports::SummaryFilters,
        reachable: Option<&[String]>,
    ) -> Result<SummaryReport, DomainError> {
        // Reads and deserializes EVERY run in scope per call (#415): keyed
        // by scope and filters — two callers never share an answer their
        // authorisation would differ on. Validation failures (an unknown
        // `project_id` filter answers 404) are not cached either way:
        // `compute` runs them before anything is stored.
        let key = format!(
            "summary|{}|{}|{}|{}|{}|{}",
            super::cache::scope_key(reachable),
            super::cache::optional_key(filters.project_id.as_deref()),
            super::cache::optional_key(filters.milestone_id.as_deref()),
            super::cache::optional_key(filters.configuration_id.as_deref()),
            super::cache::optional_key(filters.from.as_deref()),
            super::cache::optional_key(filters.to.as_deref()),
        );
        let value = self.cached_derivations(&key, || {
            serde_json::to_value(self.summary_report_uncached(filters, reachable)?)
                .map_err(|error| DomainError::Internal(error.to_string()))
        })?;
        serde_json::from_value(value).map_err(|error| DomainError::Internal(error.to_string()))
    }

    /// Whether an identifier-filter resolution failure must be answered as
    /// the empty report rather than echoed to the caller (#422).
    ///
    /// The scope-filtered report is the one endpoint a restricted caller may
    /// probe with identifiers it does not own; until now a 404 meant "no
    /// such milestone anywhere" and a 409 meant "several" — across EVERY
    /// tenant's data, an existence oracle smuggled inside a properly scoped
    /// answer. Errors that are not NotFound/Conflict (a real storage
    /// failure) are never masked, and a caller that CAN see one of the
    /// homes keeps receiving the documented 409 for genuine ambiguity: what
    /// disappears is only the cross-tenant signal, never a stated contract.
    fn filter_out_of_reach(
        &self,
        resource: Resource,
        id: &str,
        reachable: Option<&[String]>,
        error: &DomainError,
    ) -> bool {
        if !matches!(error, DomainError::NotFound(_) | DomainError::Conflict(_)) {
            return false;
        }
        let Some(reachable) = reachable else {
            return false;
        };
        !self
            .repository
            .locate(resource, id)
            .unwrap_or_default()
            .iter()
            .any(|home| reachable.contains(&home.project().to_owned()))
    }

    fn summary_report_uncached(
        &self,
        filters: &reports::SummaryFilters,
        reachable: Option<&[String]>,
    ) -> Result<SummaryReport, DomainError> {
        let mut filters = filters.clone();
        if let Some(project_id) = filters.project_id.as_deref() {
            self.require_parent(&Parent::Project(project_id.to_owned()))?;
        }

        let milestone_runs = match filters.milestone_id.as_deref() {
            Some(id) => {
                // A filter names no home to prefer, so the milestone resolves
                // globally exactly as the route that reads it does — but the
                // ANSWER a restricted caller gets for an identifier outside
                // its scope is the empty report, never the 404 or 409 that
                // would betray whether it exists at all (#422).
                match self.resolve(Resource::Milestones, id, None, "Milestone not found") {
                    Ok(home) => {
                        let value = self
                            .repository
                            .read_at(Resource::Milestones, Some(&home), id)
                            .map_err(error::milestone_error)?;
                        let milestone: Milestone = serde_json::from_value(value).map_err(|_| {
                            DomainError::Internal("Stored milestone JSON is invalid".to_owned())
                        })?;
                        Some(milestone.test_run_ids.unwrap_or_default())
                    }
                    Err(error) => {
                        if self.filter_out_of_reach(Resource::Milestones, id, reachable, &error) {
                            return Ok(reports::summary(&[]));
                        }
                        return Err(error);
                    }
                }
            }
            None => None,
        };

        if let Some(config_id) = filters.configuration_id.as_deref() {
            // A filter value is not a dereference, so an identifier two projects
            // hold is not a conflict here; one none holds is absent — and for a
            // restricted caller, absent and invisible answer identically: the
            // empty report, like any unmatched filter (#422).
            match self.repository.locate(Resource::Configurations, config_id) {
                Ok(homes) if !homes.is_empty() => {}
                Ok(_) => {
                    if reachable.is_some() {
                        return Ok(reports::summary(&[]));
                    }
                    return Err(DomainError::NotFound(
                        entity_missing_message(Resource::Configurations).to_owned(),
                    ));
                }
                Err(error) => return Err(error::delete_error(error)),
            }
        }

        if let Some(raw) = std::mem::take(&mut filters.from) {
            filters.from = Some(reports::parse_date_filter(&raw)?);
        }
        if let Some(raw) = std::mem::take(&mut filters.to) {
            filters.to = Some(reports::parse_date_filter(&raw)?);
        }

        let mut results = Vec::new();
        // A global listing de-duplicates, so two runs sharing an identifier
        // would be counted once and the report would under-report. The walk is
        // per project instead, reading each run from the home that owns it.
        for project in self
            .repository
            .list(Resource::Projects)
            .map_err(error::read_error)?
        {
            let home = Parent::Project(project);
            let run_ids = self
                .repository
                .list_children(&home, Resource::Runs)
                .map_err(error::read_error)?;
            for run_id in run_ids {
                let Ok(value) = self
                    .repository
                    .read_at(Resource::Runs, Some(&home), &run_id)
                else {
                    continue;
                };
                let Ok(run) = serde_json::from_value::<TestRun>(value) else {
                    continue;
                };
                if let Some(reachable) = reachable
                    && !reports::run_reachable(&run, home.project(), reachable)
                {
                    continue;
                }
                if !reports::run_is_in_scope(
                    &run,
                    home.project(),
                    &run_id,
                    &filters,
                    milestone_runs.as_deref(),
                ) {
                    continue;
                }
                results.extend(run.results.unwrap_or_default());
            }
        }

        Ok(reports::summary(&results))
    }
}
