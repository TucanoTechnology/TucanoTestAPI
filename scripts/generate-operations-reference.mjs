#!/usr/bin/env node
//
// Render the route and operation reference from openapi.json (issue #174).
//
// The wiki's operation index is generated, never hand-written: it is a view of
// the contract, so it cannot drift from it. Run with --check to compare the
// committed page against a fresh render instead of writing it; CI uses that
// form so a contract change that is not regenerated is a red build.
//
// Usage:
//   node scripts/generate-operations-reference.mjs           # write the page
//   node scripts/generate-operations-reference.mjs --check   # fail on drift
//
// The output is docs/generated/operations-reference.md. The paths in
// openapi.json are already served under the API's versioned prefix (for
// example a route registered at /projects is published as /api/v1/projects),
// so the document's own path is used verbatim.

import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const contractPath = resolve(repoRoot, "openapi.json");
const outputPath = resolve(repoRoot, "docs/generated/operations-reference.md");

const METHOD_ORDER = ["get", "post", "put", "patch", "delete", "head", "options"];

/** The operations of the contract, ordered by path then by method. */
function operations(document) {
  const rows = [];
  for (const [path, item] of Object.entries(document.paths ?? {})) {
    for (const [method, operation] of Object.entries(item ?? {})) {
      if (!METHOD_ORDER.includes(method)) continue;
      rows.push({
        method: method.toUpperCase(),
        path,
        operationId: operation.operationId ?? "",
        summary: oneLine(operation.summary ?? ""),
        tags: operation.tags ?? [],
      });
    }
  }
  rows.sort((a, b) => (a.path === b.path ? 0 : a.path < b.path ? -1 : 1));
  return rows;
}

function oneLine(text) {
  return String(text)
    .replace(/\s+/g, " ")
    .replace(/\|/g, "\\|")
    .trim();
}

function render(document) {
  const rows = operations(document);
  const lines = [
    "# Generated — do not edit",
    "",
    "This page is **generated from [`openapi.json`](../../openapi.json)** by",
    "`scripts/generate-operations-reference.mjs`; run that script to regenerate it. Editing it by hand",
    "has no effect: CI regenerates it and fails the build on any difference.",
    "",
    "Add or change a route in `openapi.json` and regenerate; never maintain a route list by hand. The",
    "page is a *view* of the contract, never a second copy, and it deliberately carries only the",
    "operation index — method, path, `operationId` and summary. Schemas, parameters, request bodies",
    "and status codes are the contract itself: read them in [`openapi.json`](../../openapi.json) or in",
    "the Swagger UI at `/api-docs`, which is rendered from the same document.",
    "",
    `The contract currently registers **${rows.length}** operations.`,
    "",
    "| Method | Path | Operation | Summary |",
    "| --- | --- | --- | --- |",
  ];
  for (const row of rows) {
    lines.push(`| \`${row.method}\` | \`${row.path}\` | \`${row.operationId}\` | ${row.summary} |`);
  }
  lines.push("");
  return lines.join("\n");
}

const document = JSON.parse(readFileSync(contractPath, "utf8"));
const rendered = render(document);

if (process.argv.includes("--check")) {
  const committed = existsSync(outputPath) ? readFileSync(outputPath, "utf8") : "";
  if (committed === rendered) {
    console.log(`operations reference is current (${operations(document).length} operations)`);
    process.exit(0);
  }
  console.error(
    `operations reference is stale: ${relative(repoRoot, outputPath)} does not match a fresh render of ${relative(repoRoot, contractPath)}.\n` +
      "Run: node scripts/generate-operations-reference.mjs",
  );
  process.exit(1);
}

mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, rendered);
console.log(`wrote ${relative(repoRoot, outputPath)} (${operations(document).length} operations)`);
