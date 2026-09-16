# Generated — do not edit

This page is **generated from [`openapi.json`](../../openapi.json)** by
`scripts/generate-operations-reference.mjs`; run that script to regenerate it. Editing it by hand
has no effect: CI regenerates it and fails the build on any difference.

Add or change a route in `openapi.json` and regenerate; never maintain a route list by hand. The
page is a *view* of the contract, never a second copy, and it deliberately carries only the
operation index — method, path, `operationId` and summary. Schemas, parameters, request bodies
and status codes are the contract itself: read them in [`openapi.json`](../../openapi.json) or in
the Swagger UI at `/api-docs`, which is rendered from the same document.

The contract currently registers **73** operations.

| Method | Path | Operation | Summary |
| --- | --- | --- | --- |
| `GET` | `/api-docs` | `getApiDocs` | Swagger UI |
| `POST` | `/auth/login` | `login` | Sign in |
| `POST` | `/auth/logout` | `logout` | Sign out |
| `GET` | `/auth/me` | `getCurrentUser` | Who the caller is |
| `POST` | `/auth/refresh` | `refreshSession` | Exchange a refresh token |
| `GET` | `/configurations/{id}` | `getConfiguration` | Read a configuration |
| `PUT` | `/configurations/{id}` | `updateConfiguration` | Update a configuration |
| `DELETE` | `/configurations/{id}` | `deleteConfiguration` | Delete a configuration |
| `GET` | `/diagnostics` | `getDiagnostics` | Storage diagnostics |
| `GET` | `/health` | `getHealth` | Health check |
| `GET` | `/milestones/{id}` | `getMilestone` | Read a milestone |
| `PUT` | `/milestones/{id}` | `updateMilestone` | Update a milestone |
| `DELETE` | `/milestones/{id}` | `deleteMilestone` | Delete a milestone |
| `POST` | `/milestones/{id}/duplicate` | `duplicateMilestone` | Duplicate milestone |
| `GET` | `/milestones/{id}/progress` | `getMilestoneProgress` | Get milestone progress |
| `GET` | `/openapi.json` | `getOpenApiDocument` | OpenAPI document |
| `GET` | `/projects` | `listProjects` | List projects |
| `POST` | `/projects` | `createProject` | Create a project |
| `GET` | `/projects/{id}` | `getProject` | Read a project, assembling the suites and directly owned cases it holds |
| `PUT` | `/projects/{id}` | `updateProject` | Update a project |
| `DELETE` | `/projects/{id}` | `deleteProject` | Delete a project and everything below it |
| `GET` | `/projects/{id}/configurations` | `listProjectConfigurations` | List the configurations a project owns |
| `POST` | `/projects/{id}/configurations` | `addProjectConfiguration` | Create a configuration in a project |
| `DELETE` | `/projects/{id}/configurations/{config_id}` | `removeProjectConfiguration` | Delete a configuration a project owns |
| `POST` | `/projects/{id}/duplicate` | `duplicateProject` | Duplicate resource |
| `GET` | `/projects/{id}/milestones` | `listProjectMilestones` | List the milestones a project owns |
| `POST` | `/projects/{id}/milestones` | `addProjectMilestone` | Create a milestone in a project |
| `DELETE` | `/projects/{id}/milestones/{milestone_id}` | `removeProjectMilestone` | Delete a milestone a project owns |
| `GET` | `/projects/{id}/test_cases` | `listProjectTestCases` | List the test cases a project directly owns |
| `POST` | `/projects/{id}/test_cases` | `addProjectTestCase` | Create a test case in a project, or place an existing one |
| `DELETE` | `/projects/{id}/test_cases/{case_id}` | `removeProjectTestCase` | Delete a test case a project owns |
| `GET` | `/projects/{id}/test_runs` | `listProjectTestRuns` | List the test runs a project owns |
| `POST` | `/projects/{id}/test_runs` | `addProjectTestRun` | Create a test run in a project |
| `DELETE` | `/projects/{id}/test_runs/{run_id}` | `removeProjectTestRun` | Delete a test run a project owns |
| `GET` | `/projects/{id}/test_suites` | `listProjectTestSuites` | List the test suites a project owns |
| `POST` | `/projects/{id}/test_suites` | `addProjectTestSuite` | Create a test suite in a project, or place an existing one |
| `DELETE` | `/projects/{id}/test_suites/{suite_id}` | `removeProjectTestSuite` | Delete a test suite a project owns |
| `GET` | `/ready` | `getReady` | Readiness check |
| `GET` | `/reports/coverage` | `getCoverageReport` | Get the coverage report |
| `GET` | `/reports/summary` | `getSummaryReport` | Get the summary report |
| `GET` | `/test_cases/{id}` | `getTestCase` | Read a test case |
| `PUT` | `/test_cases/{id}` | `updateTestCase` | Update a test case |
| `DELETE` | `/test_cases/{id}` | `deleteTestCase` | Delete a test case |
| `POST` | `/test_cases/{id}/attachments` | `uploadTestCaseAttachment` | Upload attachment |
| `GET` | `/test_cases/{id}/attachments/{filename}` | `downloadTestCaseAttachment` | Download attachment |
| `DELETE` | `/test_cases/{id}/attachments/{filename}` | `deleteTestCaseAttachment` | Delete attachment |
| `POST` | `/test_cases/{id}/duplicate` | `duplicateTestCase` | Duplicate test case |
| `GET` | `/test_cases/{id}/history` | `listTestCaseHistory` | List test-case revisions |
| `GET` | `/test_cases/{id}/history/{version}` | `getTestCaseVersion` | Read a test-case revision |
| `GET` | `/test_cases/{id}/steps/{step_index}/attachments` | `listStepAttachments` | List step attachments |
| `POST` | `/test_cases/{id}/steps/{step_index}/attachments` | `uploadStepAttachment` | Upload step attachment |
| `DELETE` | `/test_cases/{id}/steps/{step_index}/attachments/{filename}` | `deleteStepAttachment` | Delete step attachment |
| `GET` | `/test_runs/{id}` | `getTestRun` | Read a test run |
| `PUT` | `/test_runs/{id}` | `updateTestRun` | Update a test run |
| `DELETE` | `/test_runs/{id}` | `deleteTestRun` | Delete a test run |
| `POST` | `/test_runs/{id}/configurations` | `addTestRunConfiguration` | Link a configuration to a run |
| `DELETE` | `/test_runs/{id}/configurations/{config_id}` | `removeTestRunConfiguration` | Unlink a configuration from a run |
| `POST` | `/test_runs/{id}/duplicate` | `duplicateTestRun` | Duplicate test run without its results |
| `POST` | `/test_runs/{id}/import/json` | `importJsonResults` | Import JSON results into a run |
| `POST` | `/test_runs/{id}/import/junit` | `importJUnitResults` | Import JUnit XML results into a run |
| `POST` | `/test_runs/{id}/results` | `recordTestRunResult` | Record test case execution result in run |
| `GET` | `/test_runs/{id}/results/{case_id}/defects` | `listResultDefects` | List the defects linked to a run result |
| `POST` | `/test_runs/{id}/results/{case_id}/defects` | `linkResultDefect` | Link a defect to a run result |
| `DELETE` | `/test_runs/{id}/results/{case_id}/defects/{link_id}` | `unlinkResultDefect` | Unlink a defect from a run result |
| `POST` | `/test_runs/{id}/test_cases` | `addTestRunTestCase` | Add test case to run |
| `POST` | `/test_runs/{id}/test_suites` | `addTestRunTestSuite` | Add test suite to run |
| `GET` | `/test_suites/{id}` | `getTestSuite` | Read a test suite with the cases it holds assembled |
| `PUT` | `/test_suites/{id}` | `updateTestSuite` | Update a test suite |
| `DELETE` | `/test_suites/{id}` | `deleteTestSuite` | Delete a test suite and the cases it holds |
| `POST` | `/test_suites/{id}/duplicate` | `duplicateTestSuite` | Duplicate test suite |
| `GET` | `/test_suites/{id}/test_cases` | `listTestSuiteCases` | List the test cases a suite holds |
| `POST` | `/test_suites/{id}/test_cases` | `addTestSuiteCase` | Create a test case in a suite, or place an existing one |
| `DELETE` | `/test_suites/{id}/test_cases/{case_id}` | `removeTestSuiteCase` | Remove a test case from a suite |
