#!/usr/bin/env node
//
// Route-coverage check for the seed dataset specification (issue #195).
//
// Reads two files — openapi.json and docs/testing/seed-dataset-spec.md — and
// fails when the two disagree about which operations exist:
//
//   1. a route in openapi.json that no coverage-matrix row accounts for, and
//      which the spec's "Exempt operations" table does not list;
//   2. a row whose producing call resolves to no operation in openapi.json.
//
// It starts nothing, builds nothing and reaches no network: the whole point of
// the split described in the spec's §1 is that this half runs on every pull
// request for the cost of reading two files, while the seeded half runs on the
// local one-command path (scripts/demo.sh).
//
// Usage:
//   node scripts/check-matrix.mjs [--repo-root <dir>]

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

function parseRepoRoot(argv) {
  const flag = argv.indexOf("--repo-root");
  if (flag === -1) {
    return resolve(HERE, "..");
  }
  const value = argv[flag + 1];
  if (!value) {
    throw new Error("--repo-root needs a directory");
  }
  return resolve(value);
}

const METHODS = ["get", "put", "post", "delete", "patch", "head", "options"];

/** Splits one `| a | b | c |` table line into its trimmed cells. */
function tableCells(line) {
  const trimmed = line.trim();
  if (!trimmed.startsWith("|") || !trimmed.endsWith("|")) {
    return null;
  }
  return trimmed.slice(1, -1).split("|").map((cell) => cell.trim());
}

/**
 * Returns the spec document's sections, keyed by the heading text with the
 * leading `#`s and `§` numbering removed, lowercased.
 */
function sections(markdown) {
  const out = new Map();
  let current = null;
  let inFence = false;
  for (const line of markdown.split("\n")) {
    if (line.trimStart().startsWith("```")) {
      inFence = !inFence;
      continue;
    }
    if (inFence) {
      continue;
    }
    const heading = /^(#{2,4})\s+(.*?)\s*$/.exec(line);
    if (heading) {
      const title = heading[2]
        .replace(/^[\d.§\s]+/, "")
        .toLowerCase()
        .trim();
      current = title;
      out.set(title, []);
      continue;
    }
    if (current !== null) {
      out.get(current).push(line);
    }
  }
  return out;
}

/**
 * Every `METHOD /path` token in a block of prose, ignoring anything inside a
 * code fence and normalising a trailing `?query` away.
 */
function routeTokens(text) {
  const found = [];
  let inFence = false;
  for (const line of text.split("\n")) {
    if (line.trimStart().startsWith("```")) {
      inFence = !inFence;
      continue;
    }
    if (inFence) {
      continue;
    }
    const pattern = /\b(GET|PUT|POST|DELETE|PATCH|HEAD|OPTIONS)\s+(\/[A-Za-z0-9_{}$/.[\]-]*)/g;
    for (const match of line.matchAll(pattern)) {
      found.push({ method: match[1], path: match[2].split("?")[0].replace(/[.,;:)]+$/, "") });
    }
  }
  return found;
}

/** Turns an OpenAPI path or a document route into comparable segments. */
function segments(path) {
  return path.replace(/\/+$/, "").split("/").filter((part) => part.length > 0);
}

function isParameter(part) {
  return /^\{.*\}$/.test(part) || /^<.*>$/.test(part);
}

/**
 * True when `candidate` names the same route shape as `route`: same method,
 * same segment count, and every non-parameter segment equal.
 */
function sameShape(candidate, route) {
  if (candidate.method.toUpperCase() !== route.method.toUpperCase()) {
    return false;
  }
  const left = segments(candidate.path);
  const right = segments(route.path);
  if (left.length !== right.length) {
    return false;
  }
  return left.every((part, index) => isParameter(part) || isParameter(right[index]) || part === right[index]);
}

function loadContract(repoRoot) {
  const path = join(repoRoot, "openapi.json");
  const document = JSON.parse(readFileSync(path, "utf8"));
  if (!document || typeof document.paths !== "object" || document.paths === null) {
    throw new Error(`${path} has no "paths" object`);
  }
  const operations = [];
  for (const [routePath, item] of Object.entries(document.paths)) {
    if (!item || typeof item !== "object") {
      continue;
    }
    for (const method of Object.keys(item)) {
      if (METHODS.includes(method)) {
        operations.push({ method: method.toUpperCase(), path: routePath });
      }
    }
  }
  return { path, operations };
}

function loadSpec(repoRoot) {
  const path = join(repoRoot, "docs", "testing", "seed-dataset-spec.md");
  const markdown = readFileSync(path, "utf8");
  const bySection = sections(markdown);

  const matrixRow = /^\|\s*\d+\s*\|/;
  const matrix = [];
  for (const line of bySection.get("feature coverage matrix") ?? []) {
    if (matrixRow.test(line)) {
      const cells = tableCells(line);
      if (cells && cells.length >= 4) {
        // Cells: #, Feature, Seeded example, Producing call, On-disk evidence.
        matrix.push({ row: cells[0], feature: cells[1], text: `${cells[2]}\n${cells[3]}` });
      }
    }
  }

  const exemptions = [];
  for (const line of bySection.get("exempt operations") ?? []) {
    const cells = tableCells(line);
    if (!cells || cells.length < 2 || /^-+$/.test(cells[0]) || cells[0] === "Operation") {
      continue;
    }
    for (const token of routeTokens(cells[0])) {
      exemptions.push({ ...token, reason: cells[1] });
    }
  }

  return { path, matrix, exemptions };
}

function collectRoutes(matrix) {
  const routes = [];
  for (const entry of matrix) {
    for (const token of routeTokens(entry.text)) {
      routes.push({ ...token, row: entry.row, feature: entry.feature });
    }
  }
  return routes;
}

function format(route) {
  return `${route.method} ${route.path}`;
}

function run() {
  const repoRoot = parseRepoRoot(process.argv.slice(2));
  const contract = loadContract(repoRoot);
  const spec = loadSpec(repoRoot);
  const rowRoutes = collectRoutes(spec.matrix);

  const problems = [];

  // 1. Every operation is accounted for by a row, or exempted with a reason.
  for (const operation of contract.operations) {
    const row = rowRoutes.find((candidate) => sameShape(candidate, operation));
    if (row) {
      continue;
    }
    const exemption = spec.exemptions.find((candidate) => sameShape(candidate, operation));
    if (exemption) {
      continue;
    }
    problems.push(
      `openapi.json publishes ${format(operation)} but no coverage-matrix row names it and the ` +
        `"Exempt operations" table does not exempt it — add a row to §1 of ${relative(spec.path, repoRoot)}, ` +
        `or an exemption with its reason.`,
    );
  }

  // 2. Every route a row claims actually exists in the contract.
  for (const route of rowRoutes) {
    const known = contract.operations.some((operation) => sameShape(route, operation));
    if (!known) {
      problems.push(
        `coverage-matrix row ${route.row} (${route.feature}) names ${format(route)}, which ` +
          `openapi.json does not publish — fix the row or the contract.`,
      );
    }
  }

  if (problems.length > 0) {
    console.error(`check-matrix: FAIL — ${problems.length} problem(s)`);
    for (const problem of problems) {
      console.error(`  ✗ ${problem}`);
    }
    process.exit(1);
  }

  console.log(
    `check-matrix: PASS — ${contract.operations.length} contract operation(s) accounted for by ` +
      `${spec.matrix.length} matrix row(s) plus ${spec.exemptions.length} exemption(s); ` +
      `${rowRoutes.length} row route reference(s) all resolve`,
  );
}

function relative(path, repoRoot) {
  return path.startsWith(repoRoot) ? path.slice(repoRoot.length + 1) : path;
}

try {
  run();
} catch (error) {
  console.error(`check-matrix: ERROR — ${error.message}`);
  process.exit(2);
}
