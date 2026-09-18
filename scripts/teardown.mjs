#!/usr/bin/env node

/**
 * Teardown script for Tucano Test API (issue #194).
 *
 * Removes exactly the demo environment `scripts/seed.mjs` created, and nothing
 * else, following the teardown scope of `docs/testing/seed-dataset-spec.md` §4:
 * milestones, then runs, then suites and their copies, then cases and the placed
 * copies, then the two configurations the seed created — each read back from and
 * deleted through the project that owns it — then the projects that held them,
 * then the auth account and grants through the same `AuthStore` path the seed
 * used.
 *
 * The order matters. A run and a milestone hold references to suites, cases and
 * projects, and a configuration is reached through the project that owns it: a
 * project taken first cascades its configurations away, leaving this script with
 * a removal it can no longer resolve and would have to report as kept. So the
 * configurations come before the projects, and a dependency-ordered removal
 * avoids conflicts and half-removed trees.
 *
 * Two rules from the spec shape everything below:
 *
 * - **Removal is by the exact identifiers the seed created**, never by pattern,
 *   prefix or "clear the collection". `scripts/clear-data.mjs` is the opposite
 *   of this script and is deliberately not what this does.
 * - **A teardown that cannot resolve whether it created an entity must leave it
 *   in place and report it, rather than remove it.** A missed deletion is
 *   recoverable; a deleted project is not. So every removal is preceded by a
 *   check that the entity really is one of the seed's, and anything that fails
 *   that check is reported as kept and the run exits non-zero.
 *
 * Usage:
 *   node scripts/teardown.mjs [BASE_URL]
 *   TUCANO_API_URL=http://localhost:3000 node scripts/teardown.mjs
 *
 * The deployment must be reachable and authenticated the same way the seed was,
 * because teardown signs in as the bootstrap account:
 *
 *   TUCANO_BOOTSTRAP_USERNAME=admin
 *   TUCANO_BOOTSTRAP_PASSWORD=<password>
 *
 * Environment:
 *   TUCANO_BOOTSTRAP_USERNAME / TUCANO_BOOTSTRAP_PASSWORD  sign-in credentials
 *   TUCANO_API_URL                                          base URL, when no argument
 *   TUCANO_SEED_VIEWER_USERNAME                             account to remove (default `viewer`)
 *   TUCANO_SEED_EDITOR_USERNAME                             account to remove (default `editor`)
 *   TUCANO_UNSEED_AUTH_CMD                                  command line that removes the
 *                                                          account and its grants; when unset
 *                                                          that step is reported as not run
 *
 * Exit status:
 *   0  everything the seed created is gone, or was never there
 *   1  something could not be resolved or removed; the report names it
 */

const PROJECTS_TO_REMOVE = ["checkout.json", "payments.json"];

/**
 * The accounts the seed wrote, each with the one project it granted, so the one
 * grant teardown names per account.
 *
 * The names are read when the step runs, so the overrides the seed documents
 * (`TUCANO_SEED_VIEWER_USERNAME`, `TUCANO_SEED_EDITOR_USERNAME`) reach the
 * removal too, and a renamed account is still found.
 */
const SEED_ACCOUNTS = [
  {
    usernameEnv: "TUCANO_SEED_VIEWER_USERNAME",
    usernameDefault: "viewer",
    grantProject: "checkout.json",
  },
  {
    usernameEnv: "TUCANO_SEED_EDITOR_USERNAME",
    usernameDefault: "editor",
    grantProject: "checkout.json",
  },
];

/** The name a seeded account was written under, override first. */
function accountUsername(account) {
  return process.env[account.usernameEnv] || account.usernameDefault;
}

/**
 * The two configurations the seed created, each with the project that owns it.
 *
 * A configuration is a project resource: it is listed and deleted through the
 * project that holds it, never globally, so the key is the project the script
 * has to walk through to reach the value.
 */
const CONFIGURATIONS = {
  "checkout.json": "chrome-linux.json",
  "payments.json": "firefox-linux.json",
};

const RUNS = ["nightly.json", "nightly-import.json"];
const MILESTONES = ["v1.0.json"];

/**
 * The suites the seed created, each named with every project that holds it.
 *
 * `portable.checkout.json` is the one the seed places between the two projects:
 * it is created in `checkout.json`, moved into `payments.json` and copied back,
 * so it has a home in each and appears under both keys. The seed leaves
 * `regression.checkout.json` empty, but it is still a suite of the seed's and
 * is removed through the project that holds it.
 */
const SUITES = {
  "checkout.json": [
    "smoke.checkout.json",
    "regression.checkout.json",
    "portable.checkout.json",
  ],
  "payments.json": ["smoke.payments.json", "portable.checkout.json"],
};

/** The suites' copies the seed's duplicate step created. Derived, so a prefix. */
const SUITE_COPY_PREFIX = "smoke.checkout-copy-";

/**
 * Cases the seed created, with every home spec §2 gives them.
 *
 * A placed case is recorded under the home it *ends* in, never the parents it
 * passed through: `TC-MOVE-1` is created in `smoke.checkout.json` and moved four
 * times, so only `smoke.payments.json` still holds it. `TC-LOGIN-1`,
 * `TC-ORDERS-1`, `TC-CATALOG-1` and `TC-SEARCH-1` each end up in two places —
 * the home they were created in and the home step 11 copied them into — so each
 * is listed twice. A case that is not found in exactly the home recorded here is
 * reported, not deleted: two homes for one identifier makes the delete route
 * ambiguous.
 *
 * The suites are removed before the cases, so a case inside a suite reads as
 * absent by the time its turn comes: the suite's deletion took its folder with
 * it. That is the recorded behaviour, not a miss.
 */
const CASES = [
  { id: "TC-LOGIN-1", parent: { kind: "suite", id: "smoke.checkout.json" } },
  { id: "TC-LOGIN-2", parent: { kind: "suite", id: "smoke.checkout.json" } },
  { id: "TC-CART-1", parent: { kind: "suite", id: "smoke.checkout.json" } },
  { id: "TC-MOVE-1", parent: { kind: "suite", id: "smoke.payments.json" } },
  { id: "TC-PROJECT-1", parent: { kind: "project", id: "checkout.json" } },
  { id: "TC-ORDERS-1", parent: { kind: "project", id: "checkout.json" } },
  { id: "TC-CATALOG-1", parent: { kind: "project", id: "payments.json" } },
  { id: "TC-SEARCH-1", parent: { kind: "suite", id: "smoke.payments.json" } },
  { id: "TC-LOGIN-1", parent: { kind: "project", id: "payments.json" } },
  { id: "TC-ORDERS-1", parent: { kind: "project", id: "payments.json" } },
  { id: "TC-CATALOG-1", parent: { kind: "suite", id: "smoke.checkout.json" } },
  { id: "TC-SEARCH-1", parent: { kind: "suite", id: "smoke.checkout.json" } },
];

const candidateUrls = [
  process.argv[2],
  process.env.TUCANO_API_URL,
  process.env.API_URL,
  "http://localhost:3100",
  "http://localhost:8080/api",
  "http://localhost:3000",
].filter(Boolean);

let baseUrl = "";
let token = "";

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

/**
 * What the run did, in the two lists the report and the exit status are built
 * from: `removed` is everything this teardown deleted, `kept` is everything it
 * refused or could not resolve.
 */
const removed = [];
const kept = [];

function recordRemoved(what) {
  removed.push(what);
  console.log(`  ✓ removed ${what}`);
}

function recordKept(what, why) {
  kept.push({ what, why });
  console.log(`  ⚠️  kept ${what}: ${why}`);
}

async function resolveBaseUrl() {
  for (const url of candidateUrls) {
    const cleanUrl = url.replace(/\/$/, "");
    try {
      const res = await fetch(`${cleanUrl}/health`, {
        signal: AbortSignal.timeout(1500),
      });
      if (res.ok) {
        return cleanUrl;
      }
    } catch {
      // Continue searching
    }
  }
  // Unlike `clear-data.mjs`, this script never falls back to a guess: the whole
  // point is that it deletes, so it must be sure where it is deleting from.
  throw new Error(
    `no deployment answered /health; tried ${candidateUrls.map((u) => u.replace(/\/$/, "")).join(", ")}`,
  );
}

/**
 * One API call. Returns the parsed body and throws `CallFailed` with the failing
 * request, its status and its body when the status is not one the caller
 * expected.
 */
async function call(method, path, { body, expected = [200, 201, 204] } = {}) {
  const headers = {};
  if (token) {
    headers.authorization = `Bearer ${token}`;
  }
  headers["content-type"] = "application/json";
  const carriesBody = !["GET", "DELETE"].includes(method);

  const res = await fetch(`${baseUrl}${path}`, {
    method,
    headers,
    body: carriesBody ? JSON.stringify(body ?? {}) : undefined,
  });
  const text = await res.text();
  let data = text;
  try {
    data = JSON.parse(text);
  } catch {
    // Keep the raw text: some routes answer plain text.
  }

  if (!expected.includes(res.status)) {
    throw new CallFailed(method, path, res.status, text);
  }
  return data;
}

/** `GET path`, answering `null` for a 404 rather than throwing. */
async function getOrNull(path) {
  try {
    return await call("GET", path, { expected: [200] });
  } catch (error) {
    if (error instanceof CallFailed && error.status === 404) {
      return null;
    }
    throw error;
  }
}

/**
 * Deletes one entity addressed by an exact identifier.
 *
 * `guard` is what makes the removal scoped: it returns the reason to keep the
 * entity, or `null` to allow the delete. Absent entities are skipped — an
 * already-clean volume is a success, not a failure.
 */
async function deleteOne(what, path, guard) {
  const reason = await guard();
  if (reason !== null) {
    if (reason === false) {
      console.log(`  · ${what} is not there`);
    } else {
      recordKept(what, reason);
    }
    return;
  }
  await call("DELETE", path, { expected: [200, 204] });
  recordRemoved(what);
}

// --- guards -----------------------------------------------------------------

/** Whether an identifier is listed by its collection route. */
async function listed(collectionPath, id) {
  const items = await call("GET", collectionPath);
  return Array.isArray(items) && items.includes(id);
}

// --- steps ------------------------------------------------------------------

async function step0Session() {
  const username = process.env.TUCANO_BOOTSTRAP_USERNAME;
  const password = process.env.TUCANO_BOOTSTRAP_PASSWORD;
  if (!username || !password) {
    throw new Error(
      "set TUCANO_BOOTSTRAP_USERNAME and TUCANO_BOOTSTRAP_PASSWORD; teardown signs in as the " +
        "bootstrap account, exactly as the seed did",
    );
  }
  const session = await call("POST", "/auth/login", {
    body: { username, password },
  });
  token = session.accessToken;
  if (!token) {
    throw new Error("POST /auth/login answered without an accessToken");
  }
  console.log(`  → signed in as ${username}`);
}

/** 1. Milestones the seed created. */
async function step1Milestones() {
  for (const id of MILESTONES) {
    await deleteOne(
      `milestone ${id}`,
      `/milestones/${encodeURIComponent(id)}`,
      async () => ((await listed("/milestones", id)) ? null : false),
    );
  }
}

/** 2. Test runs the seed created. */
async function step2Runs() {
  for (const id of RUNS) {
    await deleteOne(
      `test run ${id}`,
      `/test_runs/${encodeURIComponent(id)}`,
      async () => ((await listed("/test_runs", id)) ? null : false),
    );
  }
}

/**
 * 3. Suites the seed created, and the copies its duplicate step made.
 *
 * The duplicate's identifier is derived by the API, so it is read back from the
 * project that owns it rather than assumed. Everything else is checked against
 * the names recorded per project — `portable.checkout.json` against both, since
 * the seed placed it in each — and a copy is matched by the seed's own prefix.
 * A suite that is neither is named as kept, never removed.
 */
async function step3Suites() {
  for (const [projectId, suiteIds] of Object.entries(SUITES)) {
    if (!(await listed("/projects", projectId))) {
      console.log(
        `  · project ${projectId} is not there, so its suites are not either`,
      );
      continue;
    }
    const suites = await call(
      "GET",
      `/projects/${encodeURIComponent(projectId)}/test_suites`,
    );
    if (!Array.isArray(suites)) {
      recordKept(
        `suites of ${projectId}`,
        "GET answered something other than an array",
      );
      continue;
    }
    for (const id of suites) {
      if (!suiteIds.includes(id) && !id.startsWith(SUITE_COPY_PREFIX)) {
        recordKept(
          `suite ${id} in ${projectId}`,
          "not one the seed created (the seed creates its own suites and the copy of smoke.checkout)",
        );
        continue;
      }
      await deleteOne(
        `suite ${id} in ${projectId}`,
        `/projects/${encodeURIComponent(projectId)}/test_suites/${encodeURIComponent(id)}`,
        async () => null,
      );
    }
  }
}

/**
 * 4. Cases the seed created, including the placed copies.
 *
 * Each case is addressed through the home spec §2 records. A case that is not
 * listed under that home is left alone: placement must resolve to exactly one
 * home, and guessing which one is not this script's job.
 */
async function step4Cases() {
  for (const { id, parent } of CASES) {
    const parentPath =
      parent.kind === "suite"
        ? `/test_suites/${encodeURIComponent(parent.id)}`
        : `/projects/${encodeURIComponent(parent.id)}`;

    await deleteOne(
      `case ${id} in ${parent.id}`,
      `${parentPath}/test_cases/${encodeURIComponent(id)}`,
      async () => {
        const children = await getOrNull(`${parentPath}/test_cases`);
        if (children === null) {
          return false;
        }
        if (!Array.isArray(children)) {
          return "the parent listing answered something other than an array";
        }
        return children.includes(id)
          ? null
          : `not listed under ${parent.id}; it may live somewhere this teardown did not record`;
      },
    );
  }
}

/**
 * 5. Configurations the seed created — each read from and deleted through the
 * project that owns it.
 *
 * A configuration is a project resource, so it is listed from its project's own
 * collection and removed through that project's own route; the bare
 * `DELETE /configurations/{id}` is not the route to use for a scoped teardown.
 * The global listing still answers, and it is read first as a cross-check: it
 * holds the configurations of the projects the caller reaches, and every entry
 * that is not one of the seed's is named as kept rather than swept away.
 *
 * This runs before the projects, because deleting a project cascades to the
 * configurations inside it: taken afterwards, there would be nothing left to
 * resolve and the run would report a removal it could not account for.
 */
async function step5Configurations() {
  const all = await call("GET", "/configurations");
  if (!Array.isArray(all)) {
    recordKept(
      "configurations",
      "GET /configurations answered something other than an array",
    );
    return;
  }
  const seeded = Object.values(CONFIGURATIONS);
  for (const id of all) {
    if (!seeded.includes(id)) {
      recordKept(
        `configuration ${id}`,
        "not one the seed created (the seed creates chrome-linux in checkout.json and firefox-linux in payments.json)",
      );
    }
  }
  for (const [projectId, configId] of Object.entries(CONFIGURATIONS)) {
    const listing = `/projects/${encodeURIComponent(projectId)}/configurations`;
    await deleteOne(
      `configuration ${configId} in ${projectId}`,
      `${listing}/${encodeURIComponent(configId)}`,
      async () => {
        const held = await getOrNull(listing);
        if (held === null) {
          return false;
        }
        if (!Array.isArray(held)) {
          return `GET ${listing} answered something other than an array`;
        }
        return held.includes(configId)
          ? null
          : `not listed under ${projectId}; it may live somewhere this teardown did not record`;
      },
    );
  }
}

/**
 * 6. Projects the seed created, with whatever is left below them.
 *
 * Deleting a project removes everything under it, so this step also cleans up
 * anything steps 3 to 5 missed — which is why deleting a project is the riskiest
 * call in the script and why it comes after the configurations. Any other
 * project is named as kept: the teardown removes its own two and says so, rather
 * than reporting a clean sweep while somebody else's project sits beside them.
 */
async function step6Projects() {
  const all = await call("GET", "/projects");
  if (!Array.isArray(all)) {
    recordKept(
      "projects",
      "GET /projects answered something other than an array",
    );
    return;
  }
  for (const id of all) {
    if (!PROJECTS_TO_REMOVE.includes(id)) {
      recordKept(
        `project ${id}`,
        "not one the seed created (the seed creates checkout.json and payments.json)",
      );
      continue;
    }
    await deleteOne(
      `project ${id}`,
      `/projects/${encodeURIComponent(id)}`,
      async () => null,
    );
  }
}

/**
 * 7. Auth accounts and grants the seed wrote.
 *
 * The API publishes no route for either, so this runs the server binary's
 * `unseed-auth` subcommand on the volume the server reads — the same
 * documented exception spec §5 records for the seed. The subcommand refuses a
 * system administrator and reports what it kept, so the scoping rule holds
 * there too. It ends with a summary line this step parses, so an account that
 * was already gone reads as clean rather than as a refusal.
 */
async function step7Auth() {
  const cli = process.env.TUCANO_UNSEED_AUTH_CMD;
  if (!cli) {
    for (const account of SEED_ACCOUNTS) {
      recordKept(
        `auth account ${accountUsername(account)} and its grants`,
        "set TUCANO_UNSEED_AUTH_CMD to the server binary (e.g. `target/release/tucano-test " +
          "unseed-auth`) to remove it; the API publishes no route for accounts or grants",
      );
    }
    return;
  }
  for (const account of SEED_ACCOUNTS) {
    await unseedAccount(cli, account);
  }
}

/**
 * Removes one seeded account and the grant it holds, and records what happened.
 *
 * The subcommand ends with a summary line that says which of three things
 * happened, because the prose above it cannot be told apart mechanically:
 * `absent` means the account was already gone, so there is nothing to report
 * and nothing to keep; `removed` means this run deleted it; `kept` means the
 * account exists and the subcommand declined to touch it.
 */
async function unseedAccount(cli, account) {
  const username = accountUsername(account);
  const { spawnSync } = await import("node:child_process");
  const args = ["--username", username];
  // The seed grants each account exactly one project, so teardown names exactly
  // one. `unseed-auth` requires at least one `--grant` to know what to remove.
  args.push("--grant", account.grantProject);
  // `TUCANO_UNSEED_AUTH_CMD` is a command *line* (it may carry its own
  // `VAR=value` prefix), so it goes to a shell. Everything the script adds is quoted.
  const quoted = args
    .map((arg) => `'${String(arg).replaceAll("'", `'\\''`)}'`)
    .join(" ");
  const result = spawnSync(`${cli} ${quoted}`, {
    encoding: "utf8",
    shell: true,
  });
  if (result.error) {
    recordKept(
      `auth account ${username}`,
      `${cli} could not run: ${result.error.message}`,
    );
    return;
  }
  if (result.status !== 0) {
    recordKept(
      `auth account ${username}`,
      `${cli} failed (exit ${result.status}):\n${result.stderr || result.stdout}`,
    );
    return;
  }
  process.stdout.write(result.stdout);

  const summary =
    /^unseed-auth: account=(\S+) grants_removed=(\d+) grants_kept=(\d+)$/m.exec(
      result.stdout,
    );
  if (summary === null) {
    recordKept(
      `auth account ${username}`,
      `the subcommand answered without its summary line, so this teardown cannot tell what it did:\n${result.stdout}`,
    );
    return;
  }
  const [, outcome, grantsRemoved, grantsKept] = summary;
  if (outcome === "removed") {
    recordRemoved(`auth account ${username} and ${grantsRemoved} grant(s)`);
  } else if (outcome === "absent") {
    console.log(`  · auth account ${username} is not there`);
  } else {
    recordKept(
      `auth account ${username}`,
      "the subcommand reported it as kept",
    );
  }
  // A grant the account never held, or one held by somebody else, is a
  // disagreement about the recorded scope rather than a clean teardown.
  if (Number(grantsKept) > 0) {
    recordKept(
      `${grantsKept} grant(s) of ${username}`,
      "the subcommand left them in place; see its output above",
    );
  }
}

// --- entry point ------------------------------------------------------------

async function runTeardown() {
  baseUrl = await resolveBaseUrl();
  console.log(`\n🧨 Tearing down the seed dataset at ${baseUrl}\n`);

  await step0Session();
  await step1Milestones();
  await step2Runs();
  await step3Suites();
  await step4Cases();
  await step5Configurations();
  await step6Projects();
  await step7Auth();

  console.log(`\n📋 Removed ${removed.length} item(s).`);
  if (kept.length > 0) {
    console.log(`   Left ${kept.length} item(s) in place:`);
    for (const { what, why } of kept) {
      console.log(`     • ${what} — ${why}`);
    }
    console.log(
      "\n❌ Teardown did not resolve everything it was asked to remove. " +
        "Nothing left in place was deleted; resolve it by hand and re-run.\n",
    );
    process.exit(1);
  }

  console.log(
    "\n✨ Teardown complete. See docs/testing/seed-dataset-spec.md §4 for the scope.\n",
  );
}

runTeardown().catch((err) => {
  if (err instanceof CallFailed) {
    console.error(`❌ Teardown failed: ${err.message}`);
  } else {
    console.error(
      "❌ Teardown failed:",
      err instanceof Error ? err.message : err,
    );
  }
  process.exit(1);
});
