# Tucano Test API Roadmap

This roadmap is ordered by delivery priority. GitHub issues are the source of
truth; paired GUI work is linked where the API contract drives the user-facing
feature.

## P0: Release gates

1. [API #9](https://github.com/TucanoTechnology/TucanoTestAPI/issues/9) - Security scanning and dependency policy
2. [API #11](https://github.com/TucanoTechnology/TucanoTestAPI/issues/11) - Threat model and compatibility contract

## P1: Contracts and core workflow capability

1. [API #14](https://github.com/TucanoTechnology/TucanoTestAPI/issues/14) - Typed HTTP API and OpenAPI contract
2. [API #34](https://github.com/TucanoTechnology/TucanoTestAPI/issues/34) - Test configurations and environment matrix
3. [API #45](https://github.com/TucanoTechnology/TucanoTestAPI/issues/45) - Step-level uploads, paired with GUI #19
4. [API #46](https://github.com/TucanoTechnology/TucanoTestAPI/issues/46) - Tags across projects, suites, cases, and runs, paired with GUI #20
5. [API #47](https://github.com/TucanoTechnology/TucanoTestAPI/issues/47) - Duplication across projects, suites, cases, and runs, paired with GUI #21
6. [API #130](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130) - Authentication and authorization (project-scoped RBAC, short-lived JWT + refresh token)

## P2: Execution depth and integrations

1. [API #35](https://github.com/TucanoTechnology/TucanoTestAPI/issues/35) - Automated test result ingestion
2. [API #37](https://github.com/TucanoTechnology/TucanoTestAPI/issues/37) - Execution metrics and pass/fail rollups
3. [API #38](https://github.com/TucanoTechnology/TucanoTestAPI/issues/38) - Test case versioning and audit history
4. [API #36](https://github.com/TucanoTechnology/TucanoTestAPI/issues/36) - External defect and issue tracker linkage

## P3: Operations and scale

1. [API #6](https://github.com/TucanoTechnology/TucanoTestAPI/issues/6) - Container deployment and operational endpoints
2. [API #15](https://github.com/TucanoTechnology/TucanoTestAPI/issues/15) - Observability and operational hardening
3. [API #16](https://github.com/TucanoTechnology/TucanoTestAPI/issues/16) - Migration, performance, and future GUI readiness
4. [API #28](https://github.com/TucanoTechnology/TucanoTestAPI/issues/28) - Enable branch protection after repository visibility allows it

Every implementation ticket follows the repository workflow: feature branch,
focused conventional commit, linked pull request, review, merge, then issue
closure. API contract changes must remain documented in `openapi.json`.
