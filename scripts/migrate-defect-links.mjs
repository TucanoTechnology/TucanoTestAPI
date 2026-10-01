#!/usr/bin/env node
// Moves defect links stored inside run documents (the pre-#460 shape) onto
// the cases they describe, through the public API — the server never rewrites
// a stored document for this, so an operator runs this once after upgrading.
//
//   TUCANO_API_URL=http://localhost:3000 \
//   TUCANO_ADMIN_USER=admin TUCANO_ADMIN_PASSWORD=... \
//   node scripts/migrate-defect-links.mjs [--apply]
//
// Without --apply the script only reports what it would move. A link whose
// case the deployment no longer holds anywhere is reported as unmovable: a
// phantom snapshot cannot own a link.
const BASE = process.env.TUCANO_API_URL ?? "http://localhost:3000";
const APPLY = process.argv.includes("--apply");

const session = await (
  await fetch(`${BASE}/auth/login`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      username: process.env.TUCANO_ADMIN_USER ?? "admin",
      password: process.env.TUCANO_ADMIN_PASSWORD ?? "",
    }),
  })
).json();
if (!session.accessToken) throw new Error("sign-in failed; set TUCANO_ADMIN_USER/TUCANO_ADMIN_PASSWORD");
const auth = { authorization: `Bearer ${session.accessToken}`, "content-type": "application/json" };
const get = async (uri) => (await fetch(`${BASE}${uri}`, { headers: auth })).json();

let moved = 0;
let reported = 0;
for (const runId of await get("/test_runs")) {
  const run = await get(`/test_runs/${encodeURIComponent(runId)}`);
  const projects = [
    ...new Set([
      ...(run.projects ?? []).map((p) => p.projectId),
      // The home is not always in the snapshot; resolve each candidate.
    ]),
  ];
  for (const result of run.results ?? []) {
    const links = result.defectLinks ?? [];
    if (links.length === 0) continue;
    // Find the case document this result's identifier names, among the run's projects.
    let home = null;
    for (const projectId of projects) {
      const doc = await get(`/projects/${encodeURIComponent(projectId)}`);
      const held =
        (doc.testCases ?? []).some((c) => c.testCaseId === result.testCaseId) ||
        (doc.testSuites ?? []).some((s) =>
          (s.testCases ?? []).some((c) => c.testCaseId === result.testCaseId),
        );
      if (held) { home = projectId; break; }
    }
    if (!home) {
      console.log(`unmovable: ${runId}/${result.testCaseId} — ${links.length} link(s), no case document holds them`);
      reported += 1;
      continue;
    }
    for (const link of links) {
      console.log(`${APPLY ? "moving" : "would move"} ${link.linkId} (${link.defectId}) from ${runId} onto ${result.testCaseId} in ${home}`);
      if (APPLY) {
        const response = await fetch(
          `${BASE}/test_runs/${encodeURIComponent(runId)}/results/${encodeURIComponent(result.testCaseId)}/defects`,
          {
            method: "POST",
            headers: auth,
            body: JSON.stringify({
              defectId: link.defectId,
              defectUrl: link.defectUrl,
              trackerType: link.trackerType,
              ...(link.title ? { title: link.title } : {}),
              ...(link.status ? { status: link.status } : {}),
            }),
          },
        );
        if (!response.ok) {
          const error = await response.json().catch(() => ({}));
          // A duplicate across two runs is the same fact already recorded.
          if (error?.error?.code !== "conflict") {
            throw new Error(`link move failed for ${runId}/${result.testCaseId}/${link.defectId}: ${JSON.stringify(error)}`);
          }
        }
      }
      moved += 1;
    }
  }
}
console.log(`${APPLY ? "moved" : "reported"} ${moved} link(s)${reported ? `, ${reported} unmovable result(s)` : ""}`);
