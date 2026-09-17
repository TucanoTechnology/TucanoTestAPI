//! Creating or placing entities inside a parent, and the run sub-resources.

use super::super::composition;
use super::*;

impl<R: Repository> TestService<R> {
    // --- composition ---------------------------------------------------

    /// Creates a new entity in `target`, or places an existing one there.
    ///
    /// The body's grammar decides which: a body carrying the fields a creation
    /// needs creates, a body naming only an existing identifier places. A body
    /// that mixes the two, or that asks for an unknown `mode`, is rejected.
    pub fn compose(
        &self,
        resource: Resource,
        target: &Parent,
        body: &Value,
    ) -> Result<Composed, DomainError> {
        if creates(resource, body)? {
            return self
                .create_in(resource, target, body)
                .map(Composed::Created);
        }
        self.place_into(resource, target, body)
    }

    /// The parent a suite's own cases belong to, resolved from a bare suite id.
    pub fn suite_parent(&self, suite_id: &str) -> Result<Parent, DomainError> {
        let home = self.parent_of(Resource::Suites, suite_id, "Test suite not found")?;
        Ok(Parent::Suite {
            project: home.project().to_owned(),
            suite: suite_id.to_owned(),
        })
    }

    /// Adds the case named in `body` to a suite: a creation when the body
    /// carries a title and an expected result, otherwise a copy or a move.
    pub fn add_case_to_suite(&self, suite_id: &str, body: &Value) -> Result<(), DomainError> {
        let parent = self.suite_parent(suite_id)?;
        self.compose(Resource::Cases, &parent, body)?;
        Ok(())
    }

    /// Removes one occurrence of a case from a suite.
    pub fn remove_case_from_suite(&self, suite_id: &str, case_id: &str) -> Result<(), DomainError> {
        let parent = self.suite_parent(suite_id)?;
        self.delete_in(Resource::Cases, &parent, case_id)
    }

    /// Adds the suite named in `body` to a run, embedding a snapshot copy. Runs
    /// are never a home for a suite: the source keeps its project.
    pub fn add_suite_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let suite_id = required_string(body, "suiteId")
            .ok_or_else(|| DomainError::invalid_request("Required field suiteId is missing"))?;

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        let suite = self.load_entity::<TestSuite>(Resource::Suites, &suite_id)?;
        composition::attach_suite_to_run(&mut run, &suite, &suite_id)?;
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Adds the case named in `body` to a run, embedding a snapshot copy.
    pub fn add_case_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let test_case_id = required_string(body, "testCaseId")
            .ok_or_else(|| DomainError::invalid_request("Required field testCaseId is missing"))?;

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        let test_case = self.load_entity::<TestCase>(Resource::Cases, &test_case_id)?;
        composition::attach_case_to_run(&mut run, &test_case, &test_case_id)?;
        composition::capture_case_version(&mut run, &test_case.test_case_id, test_case.version);
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Records, or replaces, the result of a case within a run.
    pub fn record_run_result(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let test_case_id = required_string(body, "testCaseId")
            .ok_or_else(|| DomainError::invalid_request("Required field testCaseId is missing"))?;
        let status = required_string(body, "status")
            .ok_or_else(|| DomainError::invalid_request("Required field status is missing"))?;
        if !VALID_STATUSES.contains(&status.as_str()) {
            return Err(DomainError::invalid_status());
        }

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        // The result may name a case the store does not hold — the API has
        // always accepted that — so the case is looked up best-effort: its
        // version is pinned when it can be read and the run falls back to
        // version 1 when it cannot.
        let held = self
            .load_entity::<TestCase>(Resource::Cases, &test_case_id)
            .ok();
        composition::capture_case_version(
            &mut run,
            held.as_ref()
                .map_or(&test_case_id, |case| &case.test_case_id),
            held.as_ref().and_then(|case| case.version),
        );
        let result = TestCaseResult {
            test_case_id,
            status,
            timestamp: required_string(body, "timestamp").unwrap_or_else(current_timestamp_string),
            notes: required_string(body, "notes"),
            duration_ms: body.get("durationMs").and_then(Value::as_u64),
            attachments: None,
            defect_links: None,
        };
        composition::upsert_result(&mut run, result);
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Lists the defects linked to one case's result in a run.
    ///
    /// A run that does not exist, or one that records no result for `case_id`,
    /// answers `404`: an empty list means "this result has no linked defect",
    /// which is a different answer from "there is no result to link to".
    pub fn list_defects(
        &self,
        run_id: &str,
        case_id: &str,
    ) -> Result<Vec<DefectLink>, DomainError> {
        let run = self.load::<TestRun>(Resource::Runs, run_id, None, "Test run not found")?;
        let result = run
            .results
            .as_ref()
            .and_then(|results| results.iter().find(|result| result.test_case_id == case_id))
            .ok_or_else(|| DomainError::NotFound("Test result not found in test run".to_owned()))?;
        Ok(result.defect_links.clone().unwrap_or_default())
    }

    /// Imports a JUnit report's testcases into a run's results.
    ///
    /// Cases the run already records are counted as duplicates and left
    /// untouched, so re-importing a report is safe and never overwrites a result
    /// someone recorded by hand. Cases that cannot be named are counted as errors
    /// rather than failing the whole import.
    pub fn import_junit_results(
        &self,
        run_id: &str,
        xml: &str,
    ) -> Result<ImportSummary, DomainError> {
        let report = import::parse(xml)?;
        self.store_imported_results(run_id, report.cases, report.errors)
    }

    /// Imports a JSON result array into a run's results.
    ///
    /// Every entry must be usable: the whole body is parsed before anything is
    /// written, so a body that is malformed, names an unknown field, omits
    /// `testCaseId` or `status`, or carries a status that is not `Passed`,
    /// `Failed` or `Blocked` answers a `400` and leaves the run exactly as it
    /// was. Cases the run already records are counted as duplicates and left
    /// untouched, exactly as the JUnit importer does.
    pub fn import_json_results(
        &self,
        run_id: &str,
        body: &[u8],
    ) -> Result<ImportSummary, DomainError> {
        let cases = import::parse_json(body)?;
        self.store_imported_results(run_id, cases, 0)
    }

    /// Writes already-parsed import cases into a run, refusing to overwrite a
    /// case the run already records and reporting what was written.
    ///
    /// Both importers share this: they differ only in how they read a body and
    /// in whether a case they cannot use fails the request or is counted in
    /// `errors`.
    fn store_imported_results(
        &self,
        run_id: &str,
        cases: Vec<ParsedCase>,
        errors: usize,
    ) -> Result<ImportSummary, DomainError> {
        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        let mut seen: HashSet<String> = run
            .results
            .as_ref()
            .map(|results| {
                results
                    .iter()
                    .map(|result| result.test_case_id.clone())
                    .collect()
            })
            .unwrap_or_default();

        let mut summary = ImportCounts {
            passed: 0,
            failed: 0,
            blocked: 0,
        };
        let mut imported = 0;
        let mut duplicates = 0;

        for case in cases {
            if !seen.insert(case.test_case_id.clone()) {
                duplicates += 1;
                continue;
            }

            match case.status {
                ImportStatus::Passed => summary.passed += 1,
                ImportStatus::Failed => summary.failed += 1,
                ImportStatus::Blocked => summary.blocked += 1,
            }

            composition::upsert_result(
                &mut run,
                TestCaseResult {
                    test_case_id: case.test_case_id,
                    status: case.status.as_str().to_owned(),
                    timestamp: case.timestamp.unwrap_or_else(current_timestamp_string),
                    notes: case.notes,
                    duration_ms: None,
                    attachments: None,
                    defect_links: None,
                },
            );
            imported += 1;
        }

        let outcome = ImportSummary {
            imported,
            skipped: duplicates + errors,
            errors,
            duplicates,
            summary,
        };
        self.save(Resource::Runs, run_id, Some(&home), &run)?;
        Ok(outcome)
    }

    /// Links the top-level configuration named in `body` to a run by reference,
    /// refusing one the run already references.
    ///
    /// A configuration owns its storage, so the run keeps a reference to it
    /// rather than a copy; the referenced configuration is verified to exist.
    /// A run may link a configuration from any project, and the run's own home
    /// is preferred, so the reference means that project's configuration first.
    pub fn link_configuration_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let config_id = required_string(body, "configId")
            .ok_or_else(|| DomainError::invalid_request("Required field configId is missing"))?;

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        let configuration = self.load::<TestConfiguration>(
            Resource::Configurations,
            &config_id,
            Some(&home),
            entity_missing_message(Resource::Configurations),
        )?;
        composition::attach_configuration_to_run(&mut run, &configuration, &config_id)?;
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Removes a configuration reference from a run.
    ///
    /// The reference names a configuration, so it resolves the same way a link
    /// does: the run's home is preferred, one no project holds is absent, and
    /// one two projects hold is refused rather than guessed at.
    pub fn unlink_configuration_from_run(
        &self,
        run_id: &str,
        config_id: &str,
    ) -> Result<(), DomainError> {
        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        self.resolve(
            Resource::Configurations,
            config_id,
            Some(&home),
            entity_missing_message(Resource::Configurations),
        )?;
        composition::detach_configuration_from_run(&mut run, config_id)?;
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Links a defect to the result a run records for `case_id`.
    ///
    /// The client names where the defect lives; the API derives the link's own
    /// identifier and the moment it was made, and returns the built link. The
    /// link is returned for the caller to address rather than to serialise: the
    /// HTTP handler responds with the `CreateResponse` shape (`message` plus the
    /// derived `id`), not with this document, so a client learns the value it
    /// must use to unlink from `id` and reads the rest from the listing route.
    /// The URL is checked against the tracker it claims to belong to, and a
    /// defect the result already links is a conflict.
    pub fn link_defect_to_result(
        &self,
        run_id: &str,
        case_id: &str,
        body: &Value,
    ) -> Result<DefectLink, DomainError> {
        let request = defect::parse_request(body)?;
        let link = defect::new_link(
            request,
            format!("link-{}", unique_suffix()),
            current_timestamp_string(),
        );

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        composition::attach_defect_to_result(&mut run, case_id, link.clone())?;
        self.save(Resource::Runs, run_id, Some(&home), &run)?;
        Ok(link)
    }

    /// Removes the defect link `link_id` from the result a run records for
    /// `case_id`; a link the result does not carry is a 404.
    pub fn unlink_defect_from_result(
        &self,
        run_id: &str,
        case_id: &str,
        link_id: &str,
    ) -> Result<(), DomainError> {
        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        composition::detach_defect_from_result(&mut run, case_id, link_id)?;
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }
}
