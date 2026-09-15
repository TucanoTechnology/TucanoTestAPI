#!/usr/bin/env node

/**
 * Validation of a seeded environment (issue #195).
 *
 * Runs against a stack that `scripts/seed.mjs` has already seeded, and asserts
 * the minimum list in `docs/testing/seed-dataset-spec.md` §3 step 12. It is the
 * seeded half of the freshness check: where `scripts/check-matrix.mjs` proves on
 * every pull request that the matrix and the contract agree, this proves that
 * the calls the matrix promises actually return what the specification says
 * they leave behind.
 *
 * It reads only; it writes nothing and removes nothing. The intended flow is
 * `scripts/demo.sh`: bring a stack up, seed it, smoke it, then run this.
 *
 * Usage:
 *   node scripts/validate-seed.mjs [BASE_URL]
 *
 * Environment:
 *   TUCANO_API_URL               Base URL when no argument is given (default
 *                                http://localhost:3100, the Compose host port).
 *   TUCANO_BOOTSTRAP_USERNAME    Sign-in name of the bootstrap account, the
 *                                system administrator the read assertions need.
 *   TUCANO_BOOTSTRAP_PASSWORD    Its password.
 *   TUCANO_VALIDATE_TOKEN        A ready-made bearer token, used instead of
 *                                signing in.
 *   TUCANO_SEED_VIEWER_USERNAME  Non-admin account to prove the refusal with
 *                                (default viewer).
 *   TUCANO_SEED_VIEWER_PASSWORD  Its password (default viewer-seed-password).
 *
 * Both tokens are obtained by signing in with `POST /auth/login`, the route the
 * specification documents in §3 step 0. Minting one from the signing secret is
 * deliberately not offered: the account identifier an access token must carry
 * is a random string the store assigns, so a token naming the username is not
 * resolvable by `GET /auth/me` however it is signed.
 *
 * Requires: node >= 22 (any Node with global fetch).
 */

const API = (process.argv[2] ?? process.env.TUCANO_API_URL ?? "http://localhost:3100").replace(/\/$/, "");
const VIEWER_USERNAME = process.env.TUCANO_SEED_VIEWER_USERNAME ?? "viewer";
const VIEWER_PASSWORD = process.env.TUCANO_SEED_VIEWER_PASSWORD ?? "viewer-seed-password";

const PROJECTS = ["checkout.json", "payments.json"];
const SUITES = {
  "checkout.json": "smoke.checkout.json",
  "payments.json": "smoke.payments.json",
};
// Which cases each parent owns, from the spec's rows 3, 4 and 22: `TC-CART-1`,
// `TC-LOGIN-1` and `TC-LOGIN-2` are created inside `smoke.checkout.json`;
// `TC-PROJECT-1` is created directly in `checkout.json` and moved there in row
// 22, so it never leaves; row 22 copies `TC-LOGIN-1` into `payments.json`.
const HOMES = {
  "checkout.json": ["TC-PROJECT-1"],
  "payments.json": ["TC-LOGIN-1"],
};
const SUITE_HOMES = {
  "smoke.checkout.json": ["TC-CART-1", "TC-LOGIN-1", "TC-LOGIN-2"],
};
// Cases a bare `GET /test_cases/{id}` can resolve: exactly one home each.
const SINGLE_HOME_CASES = ["TC-CART-1", "TC-LOGIN-2", "TC-PROJECT-1"];
const RUNS = ["nightly.json", "nightly-import.json"];
const PROGRESS_RUN = "nightly.json";
const MILESTONE = "v1.0.json";
const CONFIGURATIONS = ["chrome-linux.json", "firefox-linux.json"];
// The project each project-owned resource lives in, from the target tree of
// spec §2: both runs and the milestone belong to `checkout.json`,
// `chrome-linux.json` to `checkout.json` and `firefox-linux.json` to
// `payments.json`. The empty entries are assertions as well — a project-scoped
// listing that stays empty is what makes "the home is the project" observable
// rather than assumed.
const OWNED = {
  configurations: {
    "checkout.json": ["chrome-linux.json"],
    "payments.json": ["firefox-linux.json"],
  },
  test_runs: {
    "checkout.json": RUNS,
    "payments.json": [],
  },
  milestones: {
    "checkout.json": [MILESTONE],
    "payments.json": [],
  },
};
// The project the seed grants the viewer and the one it deliberately withholds
// (spec §5 and row 27), which is the pair the configuration isolation below
// turns on.
const VIEWER_PROJECT = "checkout.json";
const VIEWER_WITHHELD_PROJECT = "payments.json";

const failures = [];
let checks = 0;

function pass(what) {
  checks += 1;
  console.log(`  ✓ ${what}`);
}

function fail(what, detail) {
  checks += 1;
  failures.push(`${what}${detail ? ` — ${detail}` : ""}`);
  console.error(`  ✗ ${what}${detail ? ` — ${detail}` : ""}`);
}

function ok(condition, what, detail) {
  if (condition) {
    pass(what);
  } else {
    fail(what, detail);
  }
}

class HttpError extends Error {
  constructor(response, body) {
    super(`${response.status} ${response.statusText}`);
    this.status = response.status;
    this.body = body;
  }
}

async function request(path, { method = "GET", token, body, expect = [200] } = {}) {
  const headers = {};
  if (token) {
    headers.authorization = `Bearer ${token}`;
  }
  if (body !== undefined) {
    headers["content-type"] = "application/json";
  }
  const response = await fetch(`${API}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(15000),
  });
  const text = await response.text();
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    parsed = text;
  }
  if (!expect.includes(response.status)) {
    throw new HttpError(response, parsed);
  }
  return parsed;
}

/** Returns the body, or null on a 404, and throws on anything else. */
async function getOrNull(path, token) {
  const body = await request(path, { token, expect: [200, 404] });
  return body === null ? null : body;
}

// --- Signing in -------------------------------------------------------------
//
// Every call below except `/health` needs a caller, and the API issues one only
// through `POST /auth/login`. Signing in with the bootstrap credentials is what
// spec §3 step 0 tells a reader to do, so this script does the same rather than
// reaching for the deployment's signing secret: a token that carries the right
// signature but does not name a stored account is refused by `GET /auth/me`,
// and the account identifier is a random string only the store knows.

/** Signs in and returns the access token the session carries. */
async function login(username, password) {
  const session = await request("/auth/login", {
    method: "POST",
    body: { username, password },
    expect: [200, 201],
  });
  if (typeof session?.accessToken !== "string" || session.accessToken === "") {
    throw new Error(`POST /auth/login as ${username} answered without an accessToken`);
  }
  return session.accessToken;
}

/** The bearer token the read assertions use: the administrator's. */
async function adminToken() {
  const provided = process.env.TUCANO_VALIDATE_TOKEN;
  if (provided) {
    return provided;
  }
  const username = process.env.TUCANO_BOOTSTRAP_USERNAME;
  const password = process.env.TUCANO_BOOTSTRAP_PASSWORD;
  if (!username || !password) {
    throw new Error(
      "set TUCANO_BOOTSTRAP_USERNAME and TUCANO_BOOTSTRAP_PASSWORD, the credentials of the " +
        "seeded deployment, or TUCANO_VALIDATE_TOKEN with a ready-made bearer token",
    );
  }
  return login(username, password);
}

/** The bearer token of the non-admin account whose write must be refused. */
function viewerToken() {
  return login(VIEWER_USERNAME, VIEWER_PASSWORD);
}

// --- Steps ------------------------------------------------------------------

async function stepHealth() {
  const health = await request("/health");
  ok(
    health?.status === "ok" && health?.storage === "filesystem",
    "GET /health answers status=ok storage=filesystem",
    JSON.stringify(health),
  );
}

async function stepDocuments(token) {
  for (const project of PROJECTS) {
    const document = await getOrNull(`/projects/${project}`, token);
    ok(Boolean(document), `GET /projects/${project} reads the seeded project back`);
    ok(
      document?.projectId === project,
      `GET /projects/${project} names projectId=${project}`,
      JSON.stringify(document?.projectId),
    );
  }

  for (const [project, suite] of Object.entries(SUITES)) {
    const listing = await request(`/projects/${project}/test_suites`, { token });
    ok(
      Array.isArray(listing) && listing.includes(suite),
      `GET /projects/${project}/test_suites lists ${suite}`,
      JSON.stringify(listing),
    );
  }

  // Cases are read through their parents' listings rather than through a bare
  // `GET /test_cases/<id>`: row 22 places `TC-LOGIN-1` into `payments.json`
  // while the source stays in `smoke.checkout.json`, and the contract answers a
  // bare identifier with several homes `409` by design. Which parent owns which
  // case is part of what this step checks, so the expectation is written down
  // rather than inferred from whatever the listing happens to return.
  for (const [project, expected] of Object.entries(HOMES)) {
    const listing = await request(`/projects/${project}/test_cases`, { token });
    ok(
      Array.isArray(listing) && [...listing].sort().join(",") === [...expected].sort().join(","),
      `GET /projects/${project}/test_cases lists exactly ${expected.join(", ")}`,
      JSON.stringify(listing),
    );
  }

  for (const [suite, expected] of Object.entries(SUITE_HOMES)) {
    const listing = await request(`/test_suites/${suite}/test_cases`, { token });
    ok(
      Array.isArray(listing) && [...listing].sort().join(",") === [...expected].sort().join(","),
      `GET /test_suites/${suite}/test_cases lists exactly ${expected.join(", ")}`,
      JSON.stringify(listing),
    );
  }

  // `TC-LOGIN-1` and `TC-LOGIN-2` have one home each, so the globally addressed
  // read resolves for them; `TC-CART-1` too. `TC-LOGIN-1` is the case with two
  // homes and is covered by the listings above.
  for (const id of SINGLE_HOME_CASES) {
    const testCase = await getOrNull(`/test_cases/${id}`, token);
    ok(Boolean(testCase), `GET /test_cases/${id} reads the seeded case back`);
    ok(
      testCase?.testCaseId === id,
      `GET /test_cases/${id} names testCaseId=${id}`,
      JSON.stringify(testCase?.testCaseId),
    );
  }

  // The ambiguous identifier is a documented refusal, not a crash: proving it
  // here keeps the two-home placement of row 22 observable through the API.
  try {
    await request("/test_cases/TC-LOGIN-1", { token, expect: [409] });
    pass("GET /test_cases/TC-LOGIN-1 is refused with 409 because the id has two homes");
  } catch (error) {
    if (error instanceof HttpError) {
      fail(
        `GET /test_cases/TC-LOGIN-1 is refused with 409 because the id has two homes, got ${error.status}`,
        JSON.stringify(error.body),
      );
    } else {
      fail("GET /test_cases/TC-LOGIN-1 is refused with 409 because the id has two homes", error.message);
    }
  }

  const history = await request("/test_cases/TC-LOGIN-2/history", { token });
  ok(
    Array.isArray(history) && history.length >= 1,
    "GET /test_cases/TC-LOGIN-2/history reports the update's revision",
    JSON.stringify(history),
  );

  for (const configuration of CONFIGURATIONS) {
    const listing = await request("/configurations", { token });
    ok(
      Array.isArray(listing) && listing.includes(configuration),
      `GET /configurations lists ${configuration}`,
      JSON.stringify(listing),
    );
  }

  // Every project-owned resource reads back through both of its routes: the
  // collection of the project that owns it and its global document route. The
  // two have to agree on the home, so an entity the project-scoped listing does
  // not hold fails here even though the global read would still answer.
  for (const [collection, homes] of Object.entries(OWNED)) {
    for (const [project, expected] of Object.entries(homes)) {
      const listing = await request(`/projects/${project}/${collection}`, { token });
      const expectedNames = [...expected].sort().join(",");
      ok(
        Array.isArray(listing) && [...listing].sort().join(",") === expectedNames,
        `GET /projects/${project}/${collection} lists ${expectedNames || "nothing"}`,
        JSON.stringify(listing),
      );
    }
  }

  for (const configuration of CONFIGURATIONS) {
    const document = await getOrNull(`/configurations/${configuration}`, token);
    ok(
      Boolean(document),
      `GET /configurations/${configuration} reads the seeded configuration back`,
    );
    ok(
      document?.configId === configuration,
      `GET /configurations/${configuration} names configId=${configuration}`,
      JSON.stringify(document?.configId),
    );
  }

  for (const run of RUNS) {
    const document = await getOrNull(`/test_runs/${run}`, token);
    ok(Boolean(document), `GET /test_runs/${run} reads the seeded run back`);
  }

  const milestone = await getOrNull(`/milestones/${MILESTONE}`, token);
  ok(Boolean(milestone), `GET /milestones/${MILESTONE} reads the seeded milestone back`);
}

async function stepProgress(token) {
  const progress = await request(`/milestones/${MILESTONE}/progress`, { token });
  const buckets = ["passed", "failed", "blocked", "untested", "retest"];
  const missing = buckets.filter((bucket) => typeof progress?.[bucket] !== "number");
  ok(
    missing.length === 0,
    "GET /milestones/v1.0.json/progress reports all five buckets",
    missing.length ? `missing ${missing.join(", ")}` : JSON.stringify(progress),
  );
  ok(
    progress?.milestoneId === MILESTONE,
    `progress names milestoneId=${MILESTONE}`,
    JSON.stringify(progress?.milestoneId),
  );

  // The buckets are asserted against the run's own recorded results rather than
  // against `totalCases`. A run can declare a case that has no recorded result
  // yet, and `totalCases` counts those declared cases, so the two need not
  // agree; the permissive semantics are recorded in
  // docs/contracts/api-compatibility.md. What must hold is that every recorded
  // result is bucketed by its status.
  const run = await request(`/test_runs/${PROGRESS_RUN}`, { token });
  const recorded = Array.isArray(run?.results) ? run.results : [];
  const tally = {};
  for (const result of recorded) {
    const bucket = String(result?.status ?? "").toLowerCase();
    if (buckets.includes(bucket)) {
      tally[bucket] = (tally[bucket] ?? 0) + 1;
    }
  }
  for (const bucket of buckets) {
    ok(
      progress?.[bucket] === (tally[bucket] ?? 0),
      `progress ${bucket} (${progress?.[bucket]}) matches the ${tally[bucket] ?? 0} recorded result(s)`,
      JSON.stringify({ progress: progress?.[bucket], recorded: tally[bucket] ?? 0, results: recorded.length }),
    );
  }
  const sum = buckets.reduce((total, bucket) => total + (progress?.[bucket] ?? 0), 0);

  // `totalCases` counts the cases the run *declares*, not the results it
  // records, so it is asserted against the run's `testCases` array and is
  // deliberately allowed to disagree with the bucket sum — the seeded run
  // declares two cases and records four results. The permissive semantics are
  // recorded in docs/contracts/api-compatibility.md ("Milestone progress:
  // `totalCases` and the buckets need not agree"), and the fallback to the
  // counters when nothing is declared is covered by tests/milestones.rs.
  const declared = Array.isArray(run?.testCases) ? run.testCases.length : 0;
  ok(
    progress?.totalCases === (declared > 0 ? declared : sum),
    `progress totalCases (${progress?.totalCases}) counts the ${declared} declared case(s) (buckets sum to ${sum})`,
    JSON.stringify({ totalCases: progress?.totalCases, declared, sum, results: recorded.length }),
  );
}

async function stepReports(token) {
  for (const path of ["/reports/coverage", "/reports/coverage?projectId=checkout.json"]) {
    const report = await request(path, { token });
    ok(
      typeof report?.totalCases === "number" && Array.isArray(report?.suites),
      `GET ${path} answers a coverage report`,
      JSON.stringify(report),
    );
  }
  ok(
    (await request("/reports/coverage?projectId=checkout.json", { token }))?.projectId === "checkout.json",
    "GET /reports/coverage?projectId=checkout.json scopes to that project",
  );

  for (const path of [
    "/reports/summary",
    "/reports/summary?projectId=checkout.json",
    "/reports/summary?configurationId=chrome-linux.json",
  ]) {
    const report = await request(path, { token });
    ok(
      typeof report?.total === "number" && typeof report?.passPercentage === "number",
      `GET ${path} answers a summary report`,
      JSON.stringify(report),
    );
  }
}

async function stepRunFilters(token) {
  const byTag = await request("/test_runs?tags=nightly", { token });
  ok(
    Array.isArray(byTag) && byTag.includes("nightly.json"),
    "GET /test_runs?tags=nightly returns the seeded run",
    JSON.stringify(byTag),
  );

  const byConfiguration = await request("/test_runs?configuration=chrome-linux.json", { token });
  ok(
    Array.isArray(byConfiguration) && byConfiguration.includes("nightly.json"),
    "GET /test_runs?configuration=chrome-linux.json returns the seeded run",
    JSON.stringify(byConfiguration),
  );
}

async function stepIdentity(token) {
  const me = await request("/auth/me", { token });
  ok(me?.systemAdmin === true, "GET /auth/me reports the admin's systemAdmin flag", JSON.stringify(me));

  // The bootstrap account is deliberately granted nothing: a system
  // administrator is authorized by its token's claim, before the grant store is
  // consulted (`a_system_administrator_is_authorized_without_any_grant`). So
  // `roles` is expected to be empty for the admin, and the proof that the
  // account has real reach is an authorized write, not a grant. Seed row 27 and
  // spec §5 record this.
  ok(
    Object.keys(me?.roles ?? {}).length === 0,
    `GET /auth/me reports no grants for the admin, who needs none (${JSON.stringify(me?.roles)})`,
    JSON.stringify(me?.roles),
  );

  // The admin reaches every project without a grant on any of them.
  const created = await request("/projects", {
    method: "POST",
    token,
    body: { name: `validate-admin-reach-${Date.now()}` },
    expect: [201],
  });
  const createdId = created?.projectId ?? created?.id;
  ok(
    typeof createdId === "string",
    "the admin creates a project without holding any grant on it",
    JSON.stringify(created),
  );
  if (typeof createdId === "string") {
    await request(`/projects/${encodeURIComponent(createdId)}`, { method: "DELETE", token, expect: [200, 204] });
  }
}

async function stepRefusal() {
  const token = await viewerToken();

  // The refusal has to come from authorization rather than from the token: a
  // 401 would also read as "refused", but for the wrong reason. The session is
  // proved real first, then the write is proved refused.
  const me = await request("/auth/me", { token });
  ok(
    me?.systemAdmin === false,
    `GET /auth/me with the ${VIEWER_USERNAME} session reports systemAdmin=false`,
    JSON.stringify(me),
  );
  ok(
    me?.roles?.[VIEWER_PROJECT] === "owner",
    `GET /auth/me reports the ${VIEWER_USERNAME} session's owner role on ${VIEWER_PROJECT}`,
    JSON.stringify(me?.roles),
  );
  ok(
    me?.roles?.[VIEWER_WITHHELD_PROJECT] === undefined,
    `GET /auth/me reports no grant on ${VIEWER_WITHHELD_PROJECT} for the ${VIEWER_USERNAME} session`,
    JSON.stringify(me?.roles),
  );

  // A configuration is a project resource, so this account's reach decides what
  // the collection answers: its granted project's configuration and nothing from
  // the project it holds no grant in. Read from the project side too, where no
  // reach is a 403 rather than an empty listing.
  const reachable = await request("/configurations", { token });
  ok(
    Array.isArray(reachable) &&
      reachable.includes("chrome-linux.json") &&
      !reachable.includes("firefox-linux.json"),
    "GET /configurations with the viewer session lists chrome-linux.json and not firefox-linux.json",
    JSON.stringify(reachable),
  );

  const granted = await request(`/projects/${VIEWER_PROJECT}/configurations`, { token });
  ok(
    Array.isArray(granted) && granted.includes("chrome-linux.json"),
    `GET /projects/${VIEWER_PROJECT}/configurations lists chrome-linux.json for the viewer session`,
    JSON.stringify(granted),
  );

  try {
    await request(`/projects/${VIEWER_WITHHELD_PROJECT}/configurations`, { token, expect: [403] });
    pass(
      `a viewer session is refused GET /projects/${VIEWER_WITHHELD_PROJECT}/configurations with 403`,
    );
  } catch (error) {
    const what = `a viewer session is refused GET /projects/${VIEWER_WITHHELD_PROJECT}/configurations with 403`;
    if (error instanceof HttpError) {
      fail(`${what}, got ${error.status}`, JSON.stringify(error.body));
    } else {
      fail(what, error.message);
    }
  }

  try {
    await request("/projects", {
      method: "POST",
      token,
      body: { name: `validate-refused-${Date.now()}` },
      expect: [403],
    });
    pass(`a ${VIEWER_USERNAME} session is refused POST /projects with 403`);
  } catch (error) {
    if (error instanceof HttpError) {
      fail(
        `a ${VIEWER_USERNAME} session is refused POST /projects with 403, got ${error.status}`,
        JSON.stringify(error.body),
      );
    } else {
      fail(`a ${VIEWER_USERNAME} session is refused POST /projects with 403`, error.message);
    }
  }
}

async function main() {
  console.log(`\n🔎 Validating the seeded dataset at ${API}\n`);
  const token = await adminToken();

  await stepHealth();
  await stepDocuments(token);
  await stepProgress(token);
  await stepReports(token);
  await stepRunFilters(token);
  await stepIdentity(token);
  await stepRefusal();

  console.log(`\n${checks - failures.length}/${checks} assertion(s) passed.`);
  if (failures.length > 0) {
    console.error(`\n❌ Validation failed: ${failures.length} assertion(s) did not hold.\n`);
    process.exit(1);
  }
  console.log("\n✨ Validation complete. See docs/testing/seed-dataset-spec.md §3 step 12.\n");
}

main().catch((error) => {
  console.error(`❌ Validation could not run: ${error.message}`);
  process.exit(1);
});
