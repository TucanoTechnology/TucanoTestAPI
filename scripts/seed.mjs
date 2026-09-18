#!/usr/bin/env node

/**
 * Seed script for Tucano Test API (issue #193).
 *
 * Builds the full demo environment specified by
 * `docs/testing/seed-dataset-spec.md` against a running deployment. Every
 * document, folder and attachment is produced by an HTTP call, so the seeded
 * tree is always a shape the API itself would write. The one documented
 * exception is the auth accounts and project grants of spec §5: the API
 * publishes no route that creates an account or sets a grant, so those are
 * written through the AuthStore path by the server binary — see `--auth`.
 *
 * Usage:
 *   node scripts/seed.mjs [BASE_URL]
 *   TUCANO_API_URL=http://localhost:3000 node scripts/seed.mjs
 *
 * The deployment must be started with auth enforced and a bootstrap account,
 * because the sequence signs in as it and creates projects::
 *
 *   TUCANO_AUTH_REQUIRED=true
 *   TUCANO_JWT_SECRET=<at least 32 bytes>
 *   TUCANO_BOOTSTRAP_USERNAME=admin
 *   TUCANO_BOOTSTRAP_PASSWORD=<password>
 *
 * Environment:
 *   TUCANO_BOOTSTRAP_USERNAME / TUCANO_BOOTSTRAP_PASSWORD  sign-in credentials
 *   TUCANO_SEED_VIEWER_PASSWORD                            password for the
 *                                                          seeded `viewer` account
 *   TUCANO_SEED_EDITOR_PASSWORD                            password for the
 *                                                          seeded `editor` account
 *
 * The seed is not idempotent, by design: placing a case onto an identifier the
 * target parent already holds answers 409, so a second run onto the same volume
 * fails at step 11. The script refuses up front when the seed's own identifiers
 * are already present and tells you to clear first.
 */

import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), 'fixtures');

/** Identifiers the spec fixes; anything else is read back from a response. */
const PROJECTS = ['checkout.json', 'payments.json'];
const CONFIGURATIONS = ['chrome-linux.json', 'firefox-linux.json'];
const RUNS = ['nightly.json', 'nightly-import.json'];
const MILESTONES = ['v1.0.json'];
/**
 * The suites the seed creates, named as the API is asked for them: it derives
 * `<name>.json` as the identifier, so `smoke.checkout` is stored as
 * `smoke.checkout.json`.
 *
 * `checkout.json` gets three. `smoke.checkout` holds the cases it always held;
 * `regression.checkout` is a second suite in the same project, the parent a
 * move passes a case through, and it ends up holding no case at all — the
 * "a suite may be empty" shape; `portable.checkout` is placed between the two
 * projects by step 11 and ends up with a home in each — the composed-suite
 * shape, which needs its own empty suite because placing a suite carries the
 * cases inside it along.
 */
const SUITES = {
  'checkout.json': ['smoke.checkout', 'regression.checkout', 'portable.checkout'],
  'payments.json': ['smoke.payments'],
};
/** The identifiers the names above derive to, for the calls that name them. */
const SMOKE_CHECKOUT = 'smoke.checkout.json';
const REGRESSION = 'regression.checkout.json';
const PORTABLE = 'portable.checkout.json';
const SMOKE_PAYMENTS = 'smoke.payments.json';

/**
 * The accounts the seed writes through the server binary's AuthStore path
 * (spec §5), each with the single grant it holds.
 *
 * Configurations are project resources, so a non-admin account needs a grant to
 * reach the one the seed puts in `checkout.json`. Both accounts deliberately
 * hold none on `payments.json` — the isolation spec §3 step 12 asserts.
 *
 * The ladder `viewer < editor < owner` is why both accounts exist. An `owner`
 * may write anything in the project, an `editor` may write the content inside it
 * — `POST /projects/{id}/test_suites` and `POST /projects/{id}/test_cases` — but
 * not the project document itself, which `PUT /projects/{id}` refuses with
 * `403 forbidden`. Holding a grant at a lower rung is what lets a client tell
 * "the role is too low for this operation" apart from "the account holds no
 * grant here at all", which the account with no grant would otherwise answer
 * identically.
 */
const SEED_ACCOUNTS = [
  {
    username: 'viewer',
    role: 'owner',
    project: 'checkout.json',
    passwordEnv: 'TUCANO_SEED_VIEWER_PASSWORD',
    passwordDefault: 'viewer-seed-password',
    password: null,
  },
  {
    username: 'editor',
    role: 'editor',
    project: 'checkout.json',
    passwordEnv: 'TUCANO_SEED_EDITOR_PASSWORD',
    passwordDefault: 'editor-seed-password',
    password: null,
  },
];

const candidateUrls = [
  process.argv[2],
  process.env.TUCANO_API_URL,
  process.env.API_URL,
  'http://localhost:3100',
  'http://localhost:8080/api',
  'http://localhost:3000',
].filter(Boolean);

let baseUrl = '';
let token = '';

/** A call the API refused, carrying everything needed to diagnose it. */
class CallFailed extends Error {
  constructor(method, path, status, body) {
    super(`${method} ${path} answered ${status}: ${body}`);
    this.method = method;
    this.path = path;
    this.status = status;
    this.body = body;
  }
}

async function resolveBaseUrl() {
  for (const url of candidateUrls) {
    const cleanUrl = url.replace(/\/$/, '');
    try {
      const res = await fetch(`${cleanUrl}/health`, { signal: AbortSignal.timeout(1500) });
      if (res.ok) {
        return cleanUrl;
      }
    } catch {
      // Continue searching
    }
  }
  throw new Error(
    `no deployment answered /health; tried ${candidateUrls.map((u) => u.replace(/\/$/, '')).join(', ')}`,
  );
}

/**
 * One API call. Returns the parsed body and throws `CallFailed` with the
 * failing request, its response status and its body when the status is not the
 * one the caller expected — which is the "fail loudly with the failing call and
 * its response" the issue asks for.
 *
 * `body` is JSON-encoded unless it is a `Buffer` (the JUnit import posts raw
 * XML), and `contentType` says which.
 */
async function call(
  method,
  path,
  { body, expected = [200, 201, 204], contentType = 'application/json' } = {},
) {
  const headers = {};
  if (token) {
    headers.authorization = `Bearer ${token}`;
  }
  headers['content-type'] = contentType;
  // A body on GET or DELETE is a protocol error, so only send one when the
  // method carries it. The DELETE that unlinks a defect takes no payload.
  const carriesBody = !['GET', 'DELETE'].includes(method);
  const payload = carriesBody
    ? Buffer.isBuffer(body)
      ? body
      : JSON.stringify(body ?? {})
    : undefined;

  const res = await fetch(`${baseUrl}${path}`, { method, headers, body: payload });
  const text = await res.text();
  let data = text;
  try {
    data = JSON.parse(text);
  } catch {
    // Keep the raw text; some routes answer plain text (e.g. a 413).
  }

  if (!expected.includes(res.status)) {
    throw new CallFailed(method, path, res.status, text);
  }
  return data;
}

/** Uploads one file as the single `file` part of a multipart request. */
async function upload(path, filename) {
  const form = new FormData();
  const bytes = await readFile(join(FIXTURES, filename));
  form.append('file', new Blob([bytes], { type: 'text/plain' }), filename);
  const res = await fetch(`${baseUrl}${path}`, {
    method: 'POST',
    headers: token ? { authorization: `Bearer ${token}` } : {},
    body: form,
  });
  const text = await res.text();
  if (res.status !== 201) {
    throw new CallFailed('POST', path, res.status, text);
  }
  const data = JSON.parse(text);
  if (typeof data.filename !== 'string' || data.filename.length === 0) {
    throw new Error(`POST ${path} answered without a stored filename: ${text}`);
  }
  return data.filename;
}

function step(message) {
  console.log(`  → ${message}`);
}

/**
 * The collection path that owns a case, from the parent the seed records.
 *
 * A case can be created directly in a project or inside one of its suites, and
 * the two are different routes: the parent decides which. Every create, listing
 * and placement the script makes for a case is built from this, so the seed
 * never guesses which of the two it is addressing.
 */
function parentPath(parent) {
  return parent.kind === 'suite' ? `/test_suites/${parent.id}` : `/projects/${parent.id}`;
}

/** Refuses to run over data the previous seed left behind. */
async function assertClear() {
  const runs = await call('GET', '/test_runs');
  const projects = await call('GET', '/projects');
  const present = [...runs, ...projects].filter(
    (id) => RUNS.includes(id) || PROJECTS.includes(id),
  );
  if (present.length > 0) {
    throw new Error(
      `the seed's identifiers already exist (${present.join(', ')}); ` +
        'the seed is not idempotent — tear the environment down first (scripts/teardown.mjs)',
    );
  }
}

// --- steps ------------------------------------------------------------------

async function step0Session() {
  const username = process.env.TUCANO_BOOTSTRAP_USERNAME;
  const password = process.env.TUCANO_BOOTSTRAP_PASSWORD;
  if (!username || !password) {
    throw new Error(
      'set TUCANO_BOOTSTRAP_USERNAME and TUCANO_BOOTSTRAP_PASSWORD; the seed signs in as the ' +
        'bootstrap account and starts with the deployment configured for auth (see the header)',
    );
  }
  const session = await call('POST', '/auth/login', { body: { username, password } });
  token = session.accessToken;
  if (!token) {
    throw new Error('POST /auth/login answered without an accessToken');
  }
  step(`signed in as ${username}`);
}

async function step1Projects() {
  await call('POST', '/projects', {
    body: {
      projectId: 'checkout.json',
      name: 'checkout',
      description: 'Checkout demo project',
      tags: ['checkout', 'regression'],
    },
  });
  await call('POST', '/projects', {
    body: { projectId: 'payments.json', name: 'payments' },
  });
  step(`projects: ${PROJECTS.join(', ')}`);
}

/**
 * The configurations, each created inside the project that owns it.
 *
 * A configuration is a project resource, so `POST /configurations` is retired
 * and answers `400`: it is created through the project that holds it, and only
 * that project sees it. `chrome-linux.json` belongs to the project the seed's
 * run executes, `firefox-linux.json` to the other one — which is what lets the
 * validation step prove one project's configuration is invisible to a caller
 * restricted to the other.
 */
async function step2Configurations() {
  await call('POST', '/projects/checkout.json/configurations', {
    body: { configId: 'chrome-linux.json', name: 'chrome-linux' },
  });
  await call('POST', '/projects/payments.json/configurations', {
    body: {
      configId: 'firefox-linux.json',
      name: 'firefox-linux',
      browser: 'firefox',
      os: 'linux',
    },
  });
  step('configurations: chrome-linux.json in checkout.json, firefox-linux.json in payments.json');
}

/**
 * The suites, each created inside the project that owns it.
 *
 * The names are asked for without the `.json` the API appends, so the request
 * carries `"name": "smoke.checkout"` and the suite is stored as
 * `smoke.checkout.json`. `checkout.json` gets three: the two the cases need and
 * the empty pair step 11 places — see the `SUITES` comment.
 */
async function step3Suites() {
  for (const [projectId, names] of Object.entries(SUITES)) {
    for (const name of names) {
      await call('POST', `/projects/${projectId}/test_suites`, { body: { name } });
    }
  }
  step(
    `suites: ${Object.values(SUITES)
      .flat()
      .map((name) => `${name}.json`)
      .join(', ')}`,
  );
}

/**
 * Every case the seed creates, with the ordered steps written onto it straight
 * after creation.
 *
 * Every case carries steps: the steps are the second call, a qualifying `PUT`
 * that stamps `version: 2` and writes `revisions/v1.json`, which is the version
 * history the spec asserts. Creation is `POST` against the parent's collection
 * and the parent decides the route, so a case lands directly in a project or
 * inside one suite depending on what is recorded here.
 *
 * The steps run before any placement. Placing a case duplicates its folder
 * under a second parent, after which its bare identifier resolves to two homes
 * and every document-level route for it — the `PUT` that carries steps, the
 * attachment uploads — answers 409. So cases, steps and attachments all happen
 * while each identifier still has exactly one home, and step 11 places last.
 */
const CASES = [
  {
    id: 'TC-LOGIN-1',
    parent: { kind: 'suite', id: SMOKE_CHECKOUT },
    title: 'Sign in with a valid account',
    expectedResult: 'Session is established',
    tags: ['auth'],
    steps: [
      { action: 'Open the sign-in form', expectedResult: 'The form is shown' },
      { action: 'Submit a valid account', expectedResult: 'The dashboard is shown' },
    ],
  },
  {
    id: 'TC-LOGIN-2',
    parent: { kind: 'suite', id: SMOKE_CHECKOUT },
    title: 'Sign in with a locked account',
    expectedResult: 'Sign-in is refused with a message',
    tags: ['auth'],
    steps: [
      { action: 'Open the sign-in form', expectedResult: 'The form is shown' },
      { action: 'Submit a locked account', expectedResult: 'A lock message is shown' },
    ],
  },
  {
    id: 'TC-CART-1',
    parent: { kind: 'suite', id: SMOKE_CHECKOUT },
    title: 'Add an item to the cart',
    expectedResult: 'Cart shows one item',
    steps: [{ action: 'Add an item to the cart', expectedResult: 'The cart badge shows 1' }],
  },
  {
    // Created in the suite step 11 moves it out of, four times over.
    id: 'TC-MOVE-1',
    parent: { kind: 'suite', id: SMOKE_CHECKOUT },
    title: 'Keep an item in the cart across sign-in',
    expectedResult: 'Cart still shows the item after signing in',
    steps: [
      {
        action: 'Add an item, sign in, return to the cart',
        expectedResult: 'The cart still holds the item',
      },
    ],
  },
  {
    id: 'TC-PROJECT-1',
    parent: { kind: 'project', id: 'checkout.json' },
    title: 'Reach the checkout page',
    expectedResult: 'Checkout page renders',
    steps: [{ action: 'Open the checkout page', expectedResult: 'The checkout form is shown' }],
  },
  {
    // A project-owned case that step 11 copies into the other project.
    id: 'TC-ORDERS-1',
    parent: { kind: 'project', id: 'checkout.json' },
    title: 'List the orders of an account',
    expectedResult: 'Every order of the account is listed with its status',
    steps: [
      { action: 'Open the order history', expectedResult: 'The orders are listed newest first' },
    ],
  },
  {
    // A project-owned case in the other project, copied into a suite.
    id: 'TC-CATALOG-1',
    parent: { kind: 'project', id: 'payments.json' },
    title: 'Browse the catalog page by page',
    expectedResult: 'Each page holds the page size asked for',
    steps: [{ action: 'Request page 1 of the catalog', expectedResult: 'Three items are returned' }],
  },
  {
    // The suite-owned case that step 11 copies into the other project's suite.
    id: 'TC-SEARCH-1',
    parent: { kind: 'suite', id: SMOKE_PAYMENTS },
    title: 'Search the catalog for an item',
    expectedResult: 'The matching items come back best match first',
    steps: [
      { action: 'Search for an item', expectedResult: 'The matching items are ranked by score' },
    ],
  },
];

async function step4Cases() {
  for (const testCase of CASES) {
    await call('POST', `${parentPath(testCase.parent)}/test_cases`, {
      body: {
        testCaseId: testCase.id,
        title: testCase.title,
        expectedResult: testCase.expectedResult,
        ...(testCase.tags ? { tags: testCase.tags } : {}),
      },
    });
    // A qualifying update: stamps `version`/`lastModified` and writes revisions/v1.json.
    await call('PUT', `/test_cases/${testCase.id}`, { body: { steps: testCase.steps } });
  }
  step(`cases: ${CASES.map((testCase) => testCase.id).join(', ')} — each with ordered steps`);
}

/**
 * The files the seed attaches, and where each one belongs.
 *
 * Every case carries at least one case-level attachment, and the cases with a
 * step worth illustrating carry a step-level attachment too. A step-level
 * upload addresses `steps/<index>/attachments`, so it is only valid once the
 * step exists — which is why the `PUT` that writes the steps runs in step 4,
 * and why the index recorded here must stay inside the steps of that case.
 *
 * The fixtures are the recorded evidence of the case: the cart state, the
 * locked sign-in, the order listing. They are read from `scripts/fixtures/`
 * and uploaded, never written by hand onto the volume.
 */
const ATTACHMENTS = [
  {
    id: 'TC-LOGIN-1',
    file: 'login-flow.txt',
    stepFiles: [{ index: 0, file: 'step-1.txt' }],
  },
  {
    id: 'TC-LOGIN-2',
    file: 'lock-message.txt',
    stepFiles: [
      { index: 0, file: 'step-1.txt' },
      { index: 1, file: 'step-2.txt' },
    ],
  },
  {
    id: 'TC-CART-1',
    file: 'cart-state.txt',
    stepFiles: [{ index: 0, file: 'step-2.txt' }],
  },
  {
    id: 'TC-MOVE-1',
    file: 'move-trace.txt',
    stepFiles: [{ index: 0, file: 'step-1.txt' }],
  },
  { id: 'TC-PROJECT-1', file: 'checkout-page.txt' },
  {
    id: 'TC-ORDERS-1',
    file: 'orders-payload.txt',
    stepFiles: [{ index: 0, file: 'step-2.txt' }],
  },
  { id: 'TC-CATALOG-1', file: 'catalog-snapshot.txt' },
  {
    id: 'TC-SEARCH-1',
    file: 'search-response.txt',
    stepFiles: [{ index: 0, file: 'step-1.txt' }],
  },
];

async function step5Attachments() {
  let onSteps = 0;
  for (const plan of ATTACHMENTS) {
    await upload(`/test_cases/${plan.id}/attachments`, plan.file);
    for (const stepFile of plan.stepFiles ?? []) {
      await upload(`/test_cases/${plan.id}/steps/${stepFile.index}/attachments`, stepFile.file);
      onSteps += 1;
    }
  }
  step(
    `attachments: one on each of the ${ATTACHMENTS.length} cases, plus ${onSteps} on their steps`,
  );
}

async function step6Run() {
  // Created inside the project that owns it. Its `projects` array stays: it
  // records which projects the run covered, which is not where the run lives.
  await call('POST', '/projects/checkout.json/test_runs', {
    body: {
      testRunId: 'nightly.json',
      name: 'nightly',
      timestamp: '1757800000',
      tags: ['nightly'],
      projects: [{ projectId: 'checkout.json', name: 'checkout', testSuites: [] }],
    },
  });
  await call('POST', '/test_runs/nightly.json/test_suites', {
    body: { suiteId: SMOKE_CHECKOUT },
  });
  await call('POST', '/test_runs/nightly.json/test_cases', {
    body: { testCaseId: 'TC-LOGIN-1' },
  });
  await call('POST', '/test_runs/nightly.json/test_cases', {
    body: { testCaseId: 'TC-PROJECT-1' },
  });
  await call('POST', '/test_runs/nightly.json/configurations', {
    body: { configId: 'chrome-linux.json' },
  });
  step('run nightly.json: suite, pinned cases and the chrome-linux link');
}

async function step7Results() {
  const results = '/test_runs/nightly.json/results';
  await call('POST', results, {
    body: { testCaseId: 'TC-LOGIN-1', status: 'Passed', notes: 'signed in', durationMs: 1200 },
  });
  // Recorded Blocked first, then re-recorded: one stored result per case, merged.
  await call('POST', results, {
    body: { testCaseId: 'TC-LOGIN-2', status: 'Blocked' },
  });
  await call('POST', results, {
    body: {
      testCaseId: 'TC-LOGIN-2',
      status: 'Failed',
      notes: 'lock message missing',
      durationMs: 800,
    },
  });
  await call('POST', results, {
    body: { testCaseId: 'TC-PROJECT-1', status: 'Blocked' },
  });
  await call('POST', results, {
    body: { testCaseId: 'TC-CART-1', status: 'Retest' },
  });
  step('results: Passed, Failed (replaced), Blocked, Retest; Untested left implicit');
}

async function step8Defects() {
  const defects = '/test_runs/nightly.json/results/TC-LOGIN-2/defects';
  await call('POST', defects, {
    body: {
      defectId: 'BUG-101',
      defectUrl: 'https://acme.atlassian.net/browse/BUG-101',
      trackerType: 'jira',
      title: 'Lock message missing',
      status: 'Open',
    },
  });
  const github = await call('POST', defects, {
    body: {
      defectId: '101',
      defectUrl: 'https://github.com/acme/checkout/issues/101',
      trackerType: 'github',
    },
  });
  await call('POST', defects, {
    body: {
      defectId: '42',
      defectUrl: 'https://gitlab.com/acme/checkout/-/issues/42',
      trackerType: 'gitlab',
    },
  });
  await call('POST', defects, {
    body: { defectId: 'OPS-7', defectUrl: 'https://tracker.example/OPS-7', trackerType: 'custom' },
  });
  // The link id is derived by the API; read it back, never invent one.
  if (typeof github.id !== 'string' || github.id.length === 0) {
    throw new Error(`POST ${defects} answered without a link id: ${JSON.stringify(github)}`);
  }
  await call('DELETE', `${defects}/${encodeURIComponent(github.id)}`);
  step('defects: jira, github (linked then unlinked), gitlab, custom');
}

async function step9ImportedRun() {
  // The imported run lives in the same project as `nightly.json`, so it is
  // created through the same project route, under its own identifier.
  await call('POST', '/projects/checkout.json/test_runs', {
    body: {
      testRunId: 'nightly-import.json',
      name: 'nightly-import',
      tags: ['nightly'],
      projects: [{ projectId: 'checkout.json', name: 'checkout', testSuites: [] }],
    },
  });
  await call('POST', '/test_runs/nightly-import.json/test_cases', {
    body: { testCaseId: 'TC-LOGIN-1' },
  });
  await call('POST', '/test_runs/nightly-import.json/test_cases', {
    body: { testCaseId: 'TC-CART-1' },
  });
  await call('POST', '/test_runs/nightly-import.json/import/json', {
    body: {
      results: [
        { testCaseId: 'TC-LOGIN-1', status: 'Passed' },
        { testCaseId: 'TC-CART-1', status: 'Failed', notes: 'item missing' },
      ],
    },
  });
  const xml = await readFile(join(FIXTURES, 'junit-nightly.xml'));
  // The JUnit route reads the body as XML, not JSON; the fixture names its
  // testcases exactly by case id, so the import lands on the pinned cases.
  await call('POST', '/test_runs/nightly-import.json/import/junit', {
    body: xml,
    contentType: 'application/xml',
    expected: [200],
  });
  step('run nightly-import.json: JSON and JUnit imports');
}

async function step10MilestoneAndDuplicate() {
  // The milestone lives in the run's project and references the run by id.
  await call('POST', '/projects/checkout.json/milestones', {
    body: {
      milestoneId: 'v1.0.json',
      name: 'v1.0',
      status: 'open',
      testRunIds: ['nightly.json'],
    },
  });
  await call('GET', '/milestones/v1.0.json/progress');
  const duplicate = await call('POST', `/test_suites/${SMOKE_CHECKOUT}/duplicate`, {
    body: {},
  });
  if (typeof duplicate.id !== 'string' || duplicate.id.length === 0) {
    throw new Error(`the duplicate route answered without an id: ${JSON.stringify(duplicate)}`);
  }
  step(`milestone v1.0.json; suite duplicate ${duplicate.id}`);
}

/**
 * The composition shapes, run last.
 *
 * Every placement here gives an identifier a second home (copy) or a new one
 * (move), so each one runs only after the documents it touches have been
 * written: creation, steps, attachments and run membership all address a case
 * by bare identifier and would answer 409 once the identifier resolves to two
 * parents. Nothing after this step addresses a placed identifier at all.
 *
 * Both parent kinds and both modes are covered, and each parent pair appears
 * once per mode:
 *
 *   copy  TC-LOGIN-1    suite   -> project   smoke.checkout.json  -> payments.json
 *   copy  TC-ORDERS-1   project -> project   checkout.json        -> payments.json
 *   copy  TC-CATALOG-1  project -> suite     payments.json        -> smoke.checkout.json
 *   copy  TC-SEARCH-1   suite   -> suite     smoke.payments.json -> smoke.checkout.json
 *   move  TC-MOVE-1     suite   -> project   smoke.checkout.json  -> checkout.json
 *   move  TC-MOVE-1     project -> project   checkout.json        -> payments.json
 *   move  TC-MOVE-1     project -> suite     payments.json        -> regression.checkout.json
 *   move  TC-MOVE-1     suite   -> suite     regression.checkout.json -> smoke.payments.json
 *   move  TC-PROJECT-1  project -> project   checkout.json        -> checkout.json (same parent)
 *
 * One case runs through every move: its identifier stays unique from the first
 * hop to the last, so the four hops are four calls onto one case and the final
 * tree shows it in its fourth home, with `revisions/v1.json` and both of its
 * attachments intact — `place` copies or renames the whole folder. `regression`
 * is left empty by the last hop, which is the empty-suite shape. `TC-PROJECT-1`
 * moves onto the parent that already holds it, the no-op the rules answer
 * `201` to without touching the disk. Each copied case is copied once and is
 * never addressed again, so the ambiguity its second home creates is harmless.
 *
 * The last two calls place a suite rather than a case: `portable.checkout.json`
 * is moved out of `checkout.json` into `payments.json` and then copied back, so
 * the suite ends up with one home in each project while the case identifiers
 * stay unique. It is created empty for exactly that reason — a suite placement
 * carries the cases inside it along.
 */
async function step11Placement() {
  // Copy is the default: the source keeps its home and both copies are editable.
  const copies = [
    '/projects/payments.json/test_cases',
    '/projects/payments.json/test_cases',
    `/test_suites/${SMOKE_CHECKOUT}/test_cases`,
    `/test_suites/${SMOKE_CHECKOUT}/test_cases`,
  ];
  const copiedCases = ['TC-LOGIN-1', 'TC-ORDERS-1', 'TC-CATALOG-1', 'TC-SEARCH-1'];
  for (const [index, target] of copies.entries()) {
    await call('POST', target, { body: { testCaseId: copiedCases[index] } });
  }

  // Move is opt-in: the target parent becomes the case's only physical home.
  for (const target of [
    '/projects/checkout.json/test_cases',
    '/projects/payments.json/test_cases',
    `/test_suites/${REGRESSION}/test_cases`,
    `/test_suites/${SMOKE_PAYMENTS}/test_cases`,
  ]) {
    await call('POST', target, { body: { testCaseId: 'TC-MOVE-1', mode: 'move' } });
  }

  // A move onto the parent that already holds the case, which is a no-op.
  await call('POST', '/projects/checkout.json/test_cases', {
    body: { testCaseId: 'TC-PROJECT-1', mode: 'move' },
  });

  // The suite composition, which is why `portable.checkout.json` is empty.
  await call('POST', '/projects/payments.json/test_suites', {
    body: { suiteId: PORTABLE, mode: 'move' },
  });
  await call('POST', '/projects/checkout.json/test_suites', {
    body: { suiteId: PORTABLE },
  });

  step(
    `placement: ${copies.length} copies, 5 moves (TC-MOVE-1 through all four directions) and ` +
      `${PORTABLE} held by both projects`,
  );
}

/**
 * Writes the seed's accounts and grants through the server binary's AuthStore
 * path (spec §5). The API has no route for this, so `TUCANO_SEED_AUTH_CMD` is
 * the one exception to "the generator drives the API"; it runs on the volume the
 * server reads.
 */
async function stepAuth() {
  const cli = process.env.TUCANO_SEED_AUTH_CMD;
  if (!cli) {
    console.log(
      '  ℹ️  skipping auth seeding: set TUCANO_SEED_AUTH_CMD to the server binary ' +
        '(e.g. `target/release/tucano-test seed-auth`) to create the viewer and editor ' +
        'accounts and their grants',
    );
    return;
  }
  const { spawnSync } = await import('node:child_process');
  for (const account of SEED_ACCOUNTS) {
    const password = process.env[account.passwordEnv] || account.passwordDefault;
    const args = ['--username', account.username, '--password', password];
    args.push('--grant', `${account.project}=${account.role}`);
    // `TUCANO_SEED_AUTH_CMD` is a command *line* (it may carry its own
    // `VAR=value` prefix), so it is handed to a shell. Everything the script adds
    // to it — the username, the password, the project ids and roles — is quoted.
    const quoted = args.map((arg) => `'${String(arg).replaceAll("'", `'\\''`)}'`).join(' ');
    const result = spawnSync(`${cli} ${quoted}`, { encoding: 'utf8', shell: true });
    if (result.error) {
      throw new Error(`auth seeding via ${cli} could not run: ${result.error.message}`);
    }
    if (result.status !== 0) {
      throw new Error(
        `auth seeding via ${cli} failed (exit ${result.status}):\n${result.stderr || result.stdout}`,
      );
    }
    process.stdout.write(result.stdout);
    account.password = password;

    // GET /auth/me closes the loop: it proves the files are honoured by the server.
    const session = await call('POST', '/auth/login', {
      body: { username: account.username, password: account.password },
    });
    const previous = token;
    token = session.accessToken;
    const me = await call('GET', '/auth/me');
    token = previous;
    if (me.systemAdmin !== false) {
      throw new Error(
        `the seeded ${account.username} should not be a system administrator: ${JSON.stringify(me)}`,
      );
    }
    if (me.roles?.[account.project] !== account.role) {
      throw new Error(
        `GET /auth/me reports role ${JSON.stringify(me.roles?.[account.project])} on ` +
          `${account.project}, expected ${account.role}`,
      );
    }
    // The grant is deliberately one project only: the validation step uses each
    // account's lack of reach elsewhere to prove configurations are per-project.
    for (const project of PROJECTS.filter((id) => id !== account.project)) {
      if (me.roles?.[project] !== undefined) {
        throw new Error(
          `GET /auth/me reports role ${JSON.stringify(me.roles[project])} on ${project}, but the ` +
            `seed grants ${account.username} none`,
        );
      }
    }
    step(
      `auth: ${account.username} holds ${account.role} on ${account.project} and no grant on ` +
        PROJECTS.filter((id) => id !== account.project).join(', '),
    );
  }
}

// --- entry point ------------------------------------------------------------

async function runSeed() {
  baseUrl = await resolveBaseUrl();
  console.log(`\n🌱 Seeding Tucano Test API at ${baseUrl}\n`);

  // The clear check reads the collections, so it runs after signing in.
  await step0Session();
  await assertClear();

  await step1Projects();
  await step2Configurations();
  await step3Suites();
  await step4Cases();
  await step5Attachments();
  await step6Run();
  await step7Results();
  await step8Defects();
  await step9ImportedRun();
  await step10MilestoneAndDuplicate();
  await step11Placement();
  await stepAuth();

  console.log('\n✨ Seed complete. See docs/testing/seed-dataset-spec.md §2 for the tree.\n');
}

runSeed().catch((err) => {
  if (err instanceof CallFailed) {
    console.error(`❌ Seed failed: ${err.message}`);
  } else {
    console.error('❌ Seed failed:', err instanceof Error ? err.message : err);
  }
  process.exit(1);
});
