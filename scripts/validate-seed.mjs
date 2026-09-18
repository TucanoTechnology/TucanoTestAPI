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
 * It reads the seeded documents, and the only writes it makes are its own: the
 * throwaway project that proves the admin's reach, and the throwaway suite that
 * proves the editor's grant, each deleted again in the same step so a seeded
 * document is never left rewritten. The intended flow is `scripts/demo.sh`:
 * bring a stack up, seed it, smoke it, then run this.
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
 *   TUCANO_SEED_EDITOR_USERNAME  Account holding `editor` to prove role-gated
 *                                behaviour with (default editor).
 *   TUCANO_SEED_EDITOR_PASSWORD  Its password (default editor-seed-password).
 *
 * The admin token is obtained by signing in with `POST /auth/login`, the route
 * the specification documents in §3 step 0. Minting one from the signing secret
 * is deliberately not offered: the account identifier an access token must carry
 * is a random string the store assigns, so a token naming the username is not
 * resolvable by `GET /auth/me` however it is signed.
 *
 * Requires: node >= 22 (any Node with global fetch).
 */

const API = (process.argv[2] ?? process.env.TUCANO_API_URL ?? "http://localhost:3100").replace(/\/$/, "");
const VIEWER_USERNAME = process.env.TUCANO_SEED_VIEWER_USERNAME ?? "viewer";
const VIEWER_PASSWORD = process.env.TUCANO_SEED_VIEWER_PASSWORD ?? "viewer-seed-password";
const EDITOR_USERNAME = process.env.TUCANO_SEED_EDITOR_USERNAME ?? "editor";
const EDITOR_PASSWORD = process.env.TUCANO_SEED_EDITOR_PASSWORD ?? "editor-seed-password";

const PROJECTS = ["checkout.json", "payments.json"];
const SUITES = {
  "checkout.json": ["smoke.checkout.json", "regression.checkout.json", "portable.checkout.json"],
  "payments.json": ["smoke.payments.json", "portable.checkout.json"],
};
// The suite the composition places between the two projects, so its identifier
// has two homes and the routes that resolve it from a bare id are refused.
const PORTABLE = "portable.checkout.json";
// The seed's suite duplication (spec row 21) derives its identifier from the
// source's, and the route returns it; the project's own listing is what proves
// the copy reached the disk under that derived name.
const DUPLICATE_PREFIX = "smoke.checkout-copy-";
// Which cases each project owns directly, from the spec's rows 3 and 22:
// `TC-PROJECT-1` and `TC-ORDERS-1` are created in `checkout.json`, and row 22
// copies `TC-ORDERS-1` into `payments.json` beside `TC-CATALOG-1`, which is
// created there, and `TC-LOGIN-1`, which row 22 copies out of its suite.
const HOMES = {
  "checkout.json": ["TC-PROJECT-1", "TC-ORDERS-1"],
  "payments.json": ["TC-LOGIN-1", "TC-CATALOG-1", "TC-ORDERS-1"],
};
// Which cases each suite holds, from the target tree of spec §2. The empty
// expectation is an assertion too: the move of row 22 passes through
// `regression.checkout.json` and leaves it holding no case at all, which is the
// "a suite may be empty" shape made observable rather than assumed.
const SUITE_HOMES = {
  "smoke.checkout.json": ["TC-CART-1", "TC-LOGIN-1", "TC-LOGIN-2", "TC-CATALOG-1", "TC-SEARCH-1"],
  "smoke.payments.json": ["TC-MOVE-1", "TC-SEARCH-1"],
  "regression.checkout.json": [],
};
// Every case the seed creates, where its document is read from, and how many of
// its steps carry an attachment. Four cases keep one home and are read through
// the bare document route; the four row 22 composes into a second home are read
// through `parent`, a home a listing is known to hold them in, because the bare
// route answers their identifier with `409`. `parent` follows the seed's own
// recording, so a case that left a suite by moving is expected in the suite it
// ended in.
const CASES = [
  { id: "TC-LOGIN-1", parent: { kind: "project", id: "payments.json" }, stepAttachments: 1 },
  { id: "TC-LOGIN-2", parent: null, stepAttachments: 2 },
  { id: "TC-CART-1", parent: null, stepAttachments: 1 },
  { id: "TC-MOVE-1", parent: null, stepAttachments: 1 },
  { id: "TC-PROJECT-1", parent: null, stepAttachments: 0 },
  { id: "TC-ORDERS-1", parent: { kind: "project", id: "payments.json" }, stepAttachments: 1 },
  { id: "TC-CATALOG-1", parent: { kind: "suite", id: "smoke.checkout.json" }, stepAttachments: 0 },
  { id: "TC-SEARCH-1", parent: { kind: "suite", id: "smoke.checkout.json" }, stepAttachments: 1 },
];
// The two halves of the table above: cases with exactly one home, and cases a
// composition placed so that the bare document route cannot resolve them.
const SINGLE_HOME_CASES = CASES.filter((testCase) => testCase.parent === null);
const COMPOSED_CASES = CASES.filter((testCase) => testCase.parent !== null);
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
// The project the seed grants the non-admin accounts and the one it withholds
// from both (spec §5 and rows 27–28), which is the pair the configuration
// isolation and the role-gated writes below turn on.
const GRANTED_PROJECT = "checkout.json";
const WITHHELD_PROJECT = "payments.json";

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

/**
 * Asserts that a route answers the refusal it is meant to, and nothing else.
 *
 * An ambiguity is a documented `409`, so a crash, a `404` or a `500` has to fail
 * here: the refusal is only evidence when it is the refusal the contract names.
 * A refusal a write route answers is passed as `{ method, body }`, because a
 * `GET` is the default and some refusals only exist on the write paths.
 */
async function assertRefused(what, path, status, token, { method = "GET", body } = {}) {
  try {
    await request(path, { method, token, body, expect: [status] });
    pass(what);
  } catch (error) {
    if (error instanceof HttpError) {
      fail(`${what}, got ${error.status}`, JSON.stringify(error.body));
    } else {
      fail(what, error.message);
    }
  }
}

/**
 * Reads a case's own document out of the parent that holds it.
 *
 * A bare `GET /test_cases/{id}` is the natural read for a case with one home,
 * but the composed cases have two and the contract refuses the bare identifier.
 * The assembled project document carries the project's direct cases in
 * `testCases` and each of its suites' cases in that suite's `testCases`, so the
 * parent the case is known to be in is the parent the document is read from.
 */
async function caseDocument(token, testCase) {
  if (testCase.parent.kind === "project") {
    const project = await request(`/projects/${testCase.parent.id}`, { token });
    return (project?.testCases ?? []).find((entry) => entry?.testCaseId === testCase.id) ?? null;
  }
  const suite = await getOrNull(`/test_suites/${testCase.parent.id}`, token);
  return (suite?.testCases ?? []).find((entry) => entry?.testCaseId === testCase.id) ?? null;
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

/** The bearer token of the account that holds `editor`, one rung above `viewer`. */
function editorToken() {
  return login(EDITOR_USERNAME, EDITOR_PASSWORD);
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

async function stepReady() {
  const ready = await request("/ready");
  ok(
    ready?.status === "ready" && ready?.storage === "filesystem",
    "GET /ready answers status=ready storage=filesystem",
    JSON.stringify(ready),
  );

  // The same store, as evidence rather than as a status code. `lockable` is
  // asserted rather than `lockHeld`: a peer seeding a second replica may hold
  // the lock at this instant without the store being any less ready.
  const diagnostics = await request("/diagnostics");
  ok(
    diagnostics?.ready === true &&
      diagnostics?.exists === true &&
      diagnostics?.writable === true &&
      diagnostics?.lockable === true,
    "GET /diagnostics reports the seeded store ready, existing, writable and lockable",
    JSON.stringify(diagnostics),
  );
  ok(
    typeof diagnostics?.lastWriteUnix === "number",
    "GET /diagnostics names when the store was last written",
    JSON.stringify(diagnostics),
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

  // A project holds exactly what the seed put in it: one entry per suite it
  // owns, the empty one included, and `portable.checkout.json` in both projects
  // because the composition of row 22 gave it a home in each.
  for (const [project, suites] of Object.entries(SUITES)) {
    const listing = await request(`/projects/${project}/test_suites`, { token });
    const missing = suites.filter((suite) => !Array.isArray(listing) || !listing.includes(suite));
    ok(
      missing.length === 0,
      `GET /projects/${project}/test_suites lists ${suites.join(", ")}`,
      JSON.stringify(listing),
    );
  }

  // The duplicate of row 21 is a fourth suite in `checkout.json` under an
  // identifier the source does not name, so it is matched by prefix rather than
  // by an exact name the seed would have to guess.
  const duplicated = await request("/projects/checkout.json/test_suites", { token });
  ok(
    Array.isArray(duplicated) &&
      duplicated.some((suite) => typeof suite === "string" && suite.startsWith(DUPLICATE_PREFIX)),
    `GET /projects/checkout.json/test_suites lists the duplicate of smoke.checkout.json`,
    JSON.stringify(duplicated),
  );

  // Cases are read through their parents' listings rather than through a bare
  // `GET /test_cases/<id>`: the composition of row 22 copies four cases into a
  // second home while the sources stay where they were, and the contract
  // answers a bare identifier with several homes `409` by design. Which parent
  // owns which case is part of what this step checks, so the expectation is
  // written down rather than inferred from whatever the listing happens to
  // return.
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

  // A case with one home answers the bare document route; the loop that asserts
  // steps, attachments and the revision below reads those cases the same way.
  for (const { id } of SINGLE_HOME_CASES) {
    const testCase = await getOrNull(`/test_cases/${id}`, token);
    ok(Boolean(testCase), `GET /test_cases/${id} reads the seeded case back`);
    ok(
      testCase?.testCaseId === id,
      `GET /test_cases/${id} names testCaseId=${id}`,
      JSON.stringify(testCase?.testCaseId),
    );
  }

  // An ambiguous identifier is a documented refusal, not a crash: proving it
  // here keeps the two-home placements of row 22 observable through the API.
  for (const { id } of COMPOSED_CASES) {
    await assertRefused(
      `GET /test_cases/${id} is refused with 409 because the id has two homes`,
      `/test_cases/${id}`,
      409,
      token,
    );
  }
  // The composed suite is refused the same way, on the document route and on
  // the route for its own cases: both resolve the suite's parent from its bare
  // identifier, so a suite with two homes cannot name which one is meant.
  await assertRefused(
    `GET /test_suites/${PORTABLE} is refused with 409 because the suite has two homes`,
    `/test_suites/${PORTABLE}`,
    409,
    token,
  );
  await assertRefused(
    `GET /test_suites/${PORTABLE}/test_cases is refused with 409 because the suite has two homes`,
    `/test_suites/${PORTABLE}/test_cases`,
    409,
    token,
  );

  // Every case carries ordered steps, at least one attachment and the revision
  // the qualifying `PUT` wrote, which is above the `1` the creation stamps.
  // Reading a composed case through a parent is what makes it assertable at
  // all: its bare identifier is the refusal proved above.
  for (const testCase of CASES) {
    const document = testCase.parent
      ? await caseDocument(token, testCase)
      : await getOrNull(`/test_cases/${testCase.id}`, token);
    ok(Boolean(document), `${testCase.id} reads the seeded case document back`);
    ok(
      Array.isArray(document?.steps) && document.steps.length >= 1,
      `${testCase.id} carries ordered steps`,
      JSON.stringify(document?.steps),
    );
    ok(
      Array.isArray(document?.attachments) && document.attachments.length >= 1,
      `${testCase.id} carries a case-level attachment`,
      JSON.stringify(document?.attachments),
    );
    const onSteps = (document?.steps ?? []).filter(
      (step) => Array.isArray(step?.attachments) && step.attachments.length > 0,
    ).length;
    ok(
      onSteps === testCase.stepAttachments,
      `${testCase.id} carries ${testCase.stepAttachments} step attachment(s), read back as ${onSteps}`,
      JSON.stringify(document?.steps),
    );
    ok(
      typeof document?.version === "number" && document.version >= 2,
      `${testCase.id} records the steps update as version ${document?.version} (creation stamps 1)`,
      JSON.stringify(document?.version),
    );
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

  // A run holds every case it pins, every case its embedded suite snapshots
  // declare, and every case it records a result for. Progress counts that union
  // once per case id, so it is recomputed here from the run document: the five
  // buckets partition the population (`passed + failed + blocked + untested +
  // retest` always equals `totalCases`) and a held case with no result is
  // `untested`. The semantics are recorded in docs/contracts/api-compatibility.md
  // ("Milestone progress: the buckets partition `totalCases`").
  const run = await request(`/test_runs/${PROGRESS_RUN}`, { token });
  const statusOf = new Map();
  const hold = (testCaseId) => {
    if (typeof testCaseId === "string" && !statusOf.has(testCaseId)) {
      statusOf.set(testCaseId, "untested");
    }
  };
  for (const testCase of Array.isArray(run?.testCases) ? run.testCases : []) {
    hold(testCase?.testCaseId);
  }
  for (const suite of Array.isArray(run?.testSuites) ? run.testSuites : []) {
    for (const testCase of Array.isArray(suite?.testCases) ? suite.testCases : []) {
      hold(testCase?.testCaseId);
    }
  }
  const recorded = Array.isArray(run?.results) ? run.results : [];
  for (const result of recorded) {
    if (typeof result?.testCaseId !== "string") {
      continue;
    }
    hold(result.testCaseId);
    // A recorded result wins over the `untested` a hold alone implies, and an
    // unrecognised status falls back to `untested` rather than a new bucket.
    const bucket = String(result.status ?? "").toLowerCase();
    statusOf.set(result.testCaseId, buckets.includes(bucket) ? bucket : "untested");
  }

  const expected = { passed: 0, failed: 0, blocked: 0, untested: 0, retest: 0 };
  for (const status of statusOf.values()) {
    expected[status] += 1;
  }
  const total = statusOf.size;
  const sum = buckets.reduce((running, bucket) => running + (progress?.[bucket] ?? 0), 0);

  for (const bucket of buckets) {
    ok(
      progress?.[bucket] === expected[bucket],
      `progress ${bucket} (${progress?.[bucket]}) counts the ${expected[bucket]} case(s) it holds under that status`,
      JSON.stringify({ progress: progress?.[bucket], expected: expected[bucket], population: total }),
    );
  }
  ok(
    progress?.totalCases === total,
    `progress totalCases (${progress?.totalCases}) counts the ${total} case(s) the run holds once`,
    JSON.stringify({ totalCases: progress?.totalCases, population: total, results: recorded.length }),
  );
  ok(
    sum === progress?.totalCases,
    `the five buckets sum to totalCases (${sum} == ${progress?.totalCases})`,
    JSON.stringify({ sum, totalCases: progress?.totalCases, progress }),
  );

  // The percentage divides by the population, so it is a defined zero when the
  // run holds nothing and can never leave 0..=100.
  const percentage = total > 0 ? (expected.passed / total) * 100 : 0;
  ok(
    typeof progress?.passPercentage === "number" &&
      Math.abs(progress.passPercentage - percentage) < 1e-9 &&
      progress.passPercentage >= 0 &&
      progress.passPercentage <= 100,
    `passPercentage (${progress?.passPercentage}) is passed / totalCases and inside 0..=100`,
    JSON.stringify({ passPercentage: progress?.passPercentage, percentage }),
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
    me?.roles?.[GRANTED_PROJECT] === "owner",
    `GET /auth/me reports the ${VIEWER_USERNAME} session's owner role on ${GRANTED_PROJECT}`,
    JSON.stringify(me?.roles),
  );
  ok(
    me?.roles?.[WITHHELD_PROJECT] === undefined,
    `GET /auth/me reports no grant on ${WITHHELD_PROJECT} for the ${VIEWER_USERNAME} session`,
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

  const granted = await request(`/projects/${GRANTED_PROJECT}/configurations`, { token });
  ok(
    Array.isArray(granted) && granted.includes("chrome-linux.json"),
    `GET /projects/${GRANTED_PROJECT}/configurations lists chrome-linux.json for the viewer session`,
    JSON.stringify(granted),
  );

  await assertRefused(
    `a viewer session is refused GET /projects/${WITHHELD_PROJECT}/configurations with 403`,
    `/projects/${WITHHELD_PROJECT}/configurations`,
    403,
    token,
  );

  await assertRefused(
    `a ${VIEWER_USERNAME} session is refused POST /projects with 403`,
    "/projects",
    403,
    token,
    { method: "POST", body: { name: `validate-refused-${Date.now()}` } },
  );
}

/**
 * Proves the `editor` grant is a real rung on the ladder rather than another
 * account with no reach at all.
 *
 * The point of the account is the contrast: it holds `editor` on the granted
 * project and none on the withheld one, and `editor` is enough to write the
 * content inside a project but not the project document itself. So the same
 * token is accepted for a suite and refused for a `PUT` on the project, which
 * is what a client needs in order to tell "this role is too low" apart from
 * "this account has no grant here" — the two a grant-less account answers
 * identically.
 */
async function stepEditor() {
  const token = await editorToken();

  const me = await request("/auth/me", { token });
  ok(
    me?.systemAdmin === false,
    `GET /auth/me with the ${EDITOR_USERNAME} session reports systemAdmin=false`,
    JSON.stringify(me),
  );
  ok(
    me?.roles?.[GRANTED_PROJECT] === "editor",
    `GET /auth/me reports the ${EDITOR_USERNAME} session's editor role on ${GRANTED_PROJECT}`,
    JSON.stringify(me?.roles),
  );
  ok(
    me?.roles?.[WITHHELD_PROJECT] === undefined,
    `GET /auth/me reports no grant on ${WITHHELD_PROJECT} for the ${EDITOR_USERNAME} session`,
    JSON.stringify(me?.roles),
  );

  // A suite in the granted project: the write needs `editor`, so its acceptance
  // is what proves the grant reaches the content. The suite is a probe that the
  // next assertion removes again, so the seeded listing is left as it was found.
  const probeName = `validate-editor-probe-${Date.now()}`;
  let probeId = null;
  try {
    const created = await request(`/projects/${GRANTED_PROJECT}/test_suites`, {
      method: "POST",
      token,
      body: { name: probeName },
      expect: [201],
    });
    probeId = created?.id;
    ok(
      typeof probeId === "string",
      `an ${EDITOR_USERNAME} session creates a suite in ${GRANTED_PROJECT} (needs editor)`,
      JSON.stringify(created),
    );
  } catch (error) {
    if (error instanceof HttpError) {
      fail(
        `an ${EDITOR_USERNAME} session creates a suite in ${GRANTED_PROJECT} (needs editor), got ${error.status}`,
        JSON.stringify(error.body),
      );
    } else {
      fail(
        `an ${EDITOR_USERNAME} session creates a suite in ${GRANTED_PROJECT} (needs editor)`,
        error.message,
      );
    }
  }

  if (typeof probeId === "string") {
    await request(`/projects/${GRANTED_PROJECT}/test_suites/${encodeURIComponent(probeId)}`, {
      method: "DELETE",
      token,
      expect: [200, 204],
    });
    const suites = await request(`/projects/${GRANTED_PROJECT}/test_suites`, { token });
    ok(
      Array.isArray(suites) && !suites.includes(probeId),
      `the ${EDITOR_USERNAME} probe suite is deleted, so the seeded listing is unchanged`,
      JSON.stringify(suites),
    );
  }

  // The project document itself needs `owner`, one rung above the grant, so the
  // same token that just created a suite is refused here. The body is empty on
  // purpose: extraction deserializes it before the handler runs, and `{}` is a
  // well-formed JSON body, so the answer can only be the role check — the
  // document shape is validated after authorization, not before it.
  await assertRefused(
    `an ${EDITOR_USERNAME} session is refused PUT /projects/${GRANTED_PROJECT} with 403 (needs owner)`,
    `/projects/${GRANTED_PROJECT}`,
    403,
    token,
    { method: "PUT", body: {} },
  );
}

async function main() {
  console.log(`\n🔎 Validating the seeded dataset at ${API}\n`);
  const token = await adminToken();

  await stepHealth();
  await stepReady();
  await stepDocuments(token);
  await stepProgress(token);
  await stepReports(token);
  await stepRunFilters(token);
  await stepIdentity(token);
  await stepRefusal();
  await stepEditor();

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
