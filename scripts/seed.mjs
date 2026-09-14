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
const SUITES = {
  'checkout.json': 'smoke.checkout.json',
  'payments.json': 'smoke.payments.json',
};
const VIEWER = { username: 'viewer', password: null };

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
        'the seed is not idempotent — clear the data first (scripts/clear-data.mjs)',
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

async function step1Configurations() {
  await call('POST', '/configurations', {
    body: { configId: 'chrome-linux.json', name: 'chrome-linux' },
  });
  await call('POST', '/configurations', {
    body: {
      configId: 'firefox-linux.json',
      name: 'firefox-linux',
      browser: 'firefox',
      os: 'linux',
    },
  });
  step(`configurations: ${CONFIGURATIONS.join(', ')}`);
}

async function step2Projects() {
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

async function step3Suites() {
  await call('POST', '/projects/checkout.json/test_suites', {
    body: { name: 'smoke.checkout' },
  });
  await call('POST', '/projects/payments.json/test_suites', {
    body: { name: 'smoke.payments' },
  });
  step(`suites: ${Object.values(SUITES).join(', ')}`);
}

async function step4Cases() {
  const suite = '/test_suites/smoke.checkout.json';
  await call('POST', `${suite}/test_cases`, {
    body: {
      testCaseId: 'TC-LOGIN-1',
      title: 'Sign in with a valid account',
      expectedResult: 'Session is established',
      tags: ['auth'],
    },
  });
  await call('POST', `${suite}/test_cases`, {
    body: {
      testCaseId: 'TC-LOGIN-2',
      title: 'Sign in with a locked account',
      expectedResult: 'Sign-in is refused with a message',
    },
  });
  await call('POST', `${suite}/test_cases`, {
    body: {
      testCaseId: 'TC-CART-1',
      title: 'Add an item to the cart',
      expectedResult: 'Cart shows one item',
    },
  });
  await call('POST', '/projects/checkout.json/test_cases', {
    body: {
      testCaseId: 'TC-PROJECT-1',
      title: 'Reach the checkout page',
      expectedResult: 'Checkout page renders',
    },
  });
  // A qualifying update: stamps `version`/`lastModified` and writes revisions/v1.json.
  await call('PUT', '/test_cases/TC-LOGIN-2', {
    body: {
      steps: [
        { action: 'Open the sign-in form', expectedResult: 'The form is shown' },
        { action: 'Submit a locked account', expectedResult: 'A lock message is shown' },
      ],
    },
  });
  step('cases: TC-LOGIN-1, TC-LOGIN-2, TC-CART-1, TC-PROJECT-1 (steps on TC-LOGIN-2)');
}

async function step5Attachments() {
  const caseAttachment = await upload('/test_cases/TC-LOGIN-1/attachments', 'login-flow.txt');
  const stepAttachment = await upload(
    '/test_cases/TC-LOGIN-2/steps/0/attachments',
    'step-1.txt',
  );
  step(`attachments: ${caseAttachment}, ${stepAttachment}`);
}

async function step6Run() {
  await call('POST', '/test_runs', {
    body: {
      testRunId: 'nightly.json',
      name: 'nightly',
      timestamp: '1757800000',
      tags: ['nightly'],
      projects: [{ projectId: 'checkout.json', name: 'checkout', testSuites: [] }],
    },
  });
  await call('POST', '/test_runs/nightly.json/test_suites', {
    body: { suiteId: 'smoke.checkout.json' },
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
  // Recorded Blocked first, then replaced: one stored result per case.
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
  await call('POST', '/test_runs', {
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
  await call('POST', '/milestones', {
    body: {
      milestoneId: 'v1.0.json',
      name: 'v1.0',
      status: 'open',
      testRunIds: ['nightly.json'],
    },
  });
  await call('GET', '/milestones/v1.0.json/progress');
  const duplicate = await call('POST', '/test_suites/smoke.checkout.json/duplicate', {
    body: {},
  });
  if (typeof duplicate.id !== 'string' || duplicate.id.length === 0) {
    throw new Error(`the duplicate route answered without an id: ${JSON.stringify(duplicate)}`);
  }
  step(`milestone v1.0.json; suite duplicate ${duplicate.id}`);
}

async function step11Placement() {
  // Copy is the default: the source keeps its home and both copies are editable.
  await call('POST', '/projects/payments.json/test_cases', {
    body: { testCaseId: 'TC-LOGIN-1' },
  });
  // Move is opt-in: the target parent becomes the case's only physical home.
  await call('POST', '/projects/checkout.json/test_cases', {
    body: { testCaseId: 'TC-PROJECT-1', mode: 'move' },
  });
  step('placed TC-LOGIN-1 into payments.json (copy) and TC-PROJECT-1 stayed in checkout.json (move)');
}

/**
 * Writes the seed's accounts and grants through the server binary's AuthStore
 * path (spec §5). The API has no route for this, so `--auth` is the one
 * exception to "the generator drives the API"; it runs on the volume the
 * server reads.
 */
async function stepAuth() {
  const password = process.env.TUCANO_SEED_VIEWER_PASSWORD || 'viewer-seed-password';
  const cli = process.env.TUCANO_SEED_AUTH_CMD;
  if (!cli) {
    console.log(
      '  ℹ️  skipping auth seeding: set TUCANO_SEED_AUTH_CMD to the server binary ' +
        '(e.g. `target/release/tucano-test seed-auth`) to create the viewer account and grants',
    );
    return;
  }
  const { spawnSync } = await import('node:child_process');
  const args = ['--username', VIEWER.username, '--password', password];
  for (const project of PROJECTS) {
    args.push('--grant', `${project}=owner`);
  }
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
  VIEWER.password = password;

  // GET /auth/me closes the loop: it proves the files are honoured by the server.
  const viewerSession = await call('POST', '/auth/login', {
    body: { username: VIEWER.username, password: VIEWER.password },
  });
  const previous = token;
  token = viewerSession.accessToken;
  const me = await call('GET', '/auth/me');
  token = previous;
  if (me.systemAdmin !== false) {
    throw new Error(`the seeded viewer should not be a system administrator: ${JSON.stringify(me)}`);
  }
  for (const project of PROJECTS) {
    if (me.roles?.[project] !== 'owner') {
      throw new Error(
        `GET /auth/me reports role ${JSON.stringify(me.roles?.[project])} on ${project}, expected owner`,
      );
    }
  }
  step(`auth: ${VIEWER.username} holds owner on ${PROJECTS.join(', ')}`);
}

// --- entry point ------------------------------------------------------------

async function runSeed() {
  baseUrl = await resolveBaseUrl();
  console.log(`\n🌱 Seeding Tucano Test API at ${baseUrl}\n`);

  // The clear check reads the collections, so it runs after signing in.
  await step0Session();
  await assertClear();

  await step1Configurations();
  await step2Projects();
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
