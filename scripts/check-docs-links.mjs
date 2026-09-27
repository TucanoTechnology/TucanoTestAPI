#!/usr/bin/env node
//
// Check the documentation set's internal consistency (issues #236, #174).
//
// The rules are the ones the wiki decision record states
// (docs/architecture/wiki-structure-and-publication.md, "What CI checks") and
// the ones AGENTS.md requires of the docs tree:
//
//   1. Every relative link in every tracked Markdown document resolves to a
//      file that exists. A link to a page that is not on disk is a 404 for the
//      reader, which is the defect issue #236 reports.
//   2. Every page under docs/ is listed in docs/SUMMARY.md, the mdBook table of
//      contents. A page that is not in the book is invisible to a reader.
//   3. The README is a quick-start, not an index (issue #405): its job is to
//      link the canonical indexes — docs/SUMMARY.md, the wiki index,
//      CONTRIBUTING and AGENTS — which must therefore exist and be linked.
//      Page-level completeness is rule 2's job (SUMMARY), not the README's.
//   5. Every Rust module under src/ is named — as a `file.rs` — in README.md or
//      AGENTS.md, so the architecture tables cannot rot into fiction when a
//      module is added, renamed, or split (issue #424's presence assertion:
//      links are checked for validity by rule 1, but only presence catches a
//      table that no longer describes the tree).
//   4. Every wiki page linked from the wiki index exists, and every wiki page on
//      disk is reachable from the index: the index is exhaustive by decision.
//
// Usage:
//   node scripts/check-docs-links.mjs
//
// Exits non-zero with one line per violation. It reads files and starts
// nothing, so it is cheap enough to run on every pull request.

import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { dirname, resolve, relative, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const docsRoot = join(repoRoot, "docs");
const wikiIndexPath = join(docsRoot, "wiki", "README.md");
const summaryPath = join(docsRoot, "SUMMARY.md");
const readmePath = join(repoRoot, "README.md");

const violations = [];

/** Every Markdown file under docs/, repo-relative, sorted. */
function docsPages(dir = docsRoot) {
  const found = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) found.push(...docsPages(path));
    else if (entry.name.endsWith(".md")) found.push(relative(repoRoot, path));
  }
  return found.sort();
}

// Markdown link targets: [text](target) with an optional "title" and an
// optional #anchor. Reference-style links are not used in this repository.
const LINK = /\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;

const TEXT_EXTENSIONS = new Set([".md", ".json", ".toml", ".html", ".yaml", ".yml", ".sh", ".mjs"]);

function isExternal(target) {
  return /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith("//");
}

/** Rule 1 — every relative link in every tracked document resolves. */
function checkLinks(page) {
  const source = readFileSync(join(repoRoot, page), "utf8");
  const seen = new Set();
  for (const match of source.matchAll(LINK)) {
    let target = match[1].trim();
    if (!target || isExternal(target) || target.startsWith("#")) continue;
    const anchor = target.indexOf("#");
    if (anchor !== -1) target = target.slice(0, anchor);
    if (!target) continue;
    target = decodeURIComponent(target);
    if (seen.has(target)) continue;
    seen.add(target);

    const resolved = resolve(dirname(join(repoRoot, page)), target);
    // A directory link (docs/, AgentRules/, scripts/) is valid as written.
    if (existsSync(resolved)) continue;
    if (resolvesFromRepoRoot(page, target, resolved)) continue;
    violations.push(`${page}: link target does not exist: ${target}`);
  }
}

/**
 * Links in prose are checked against the file's own directory, which is correct
 * for a Markdown document but wrong for a script path that looks like it points
 * at a text file: `../scripts/demo.sh` written inside docs/testing/ resolves to
 * docs/scripts/demo.sh, while the repository path the prose means is
 * scripts/demo.sh. Accept a target that resolves from the repository root too.
 */
function resolvesFromRepoRoot(page, target, resolved) {
  if (existsSync(resolved)) return true;
  // A document below docs/ climbs to the repository root, so a target that
  // climbs out of docs/ may be written against the root instead.
  if (!page.startsWith("docs/")) return false;
  if (!target.startsWith("../") && !target.startsWith("./../")) return false;
  return existsSync(resolve(repoRoot, target.replace(/^(\.\/)*\.\.\//, "")));
}

/** Rule 2 — every docs page appears in SUMMARY.md. */
function checkSummary(pages) {
  const summary = readFileSync(summaryPath, "utf8");
  // SUMMARY.md uses paths relative to docs/ (mdBook's src = ".")
  for (const page of pages) {
    if (page === "docs/SUMMARY.md") continue;
    const inBook = relative(docsRoot, join(repoRoot, page));
    const referenced = new RegExp(`\\(${escapeRegExp(inBook)}\\)`).test(summary);
    if (!referenced) violations.push(`docs/SUMMARY.md: does not list ${page}`);
  }
  // And every entry points at a page that exists.
  for (const match of summary.matchAll(LINK)) {
    const target = match[1].trim();
    if (isExternal(target) || target.startsWith("#")) continue;
    const resolved = resolve(docsRoot, target);
    if (!existsSync(resolved)) violations.push(`docs/SUMMARY.md: entry points at a missing page: ${target}`);
  }
}

/** Rule 3 (retargeted by issue #405) — the README links the canonical indexes. */
function checkReadmeIndex() {
  const readme = readFileSync(readmePath, "utf8");
  for (const index of [
    "docs/SUMMARY.md",
    "docs/wiki/README.md",
    "CONTRIBUTING.md",
    "AGENTS.md",
  ]) {
    if (!readme.includes(`](${index})`)) {
      violations.push(`README.md: no longer links the canonical index ${index} (issue #405 moved the table to docs/SUMMARY.md)`);
    }
  }
}

/** Rule 4 — the wiki index is exhaustive and links only pages that exist. */
function checkWikiIndex(pages) {
  const index = readFileSync(wikiIndexPath, "utf8");
  const linked = new Set();
  for (const match of index.matchAll(LINK)) {
    const target = match[1].trim();
    if (isExternal(target) || target.startsWith("#")) continue;
    const anchor = target.indexOf("#");
    const file = anchor === -1 ? target : target.slice(0, anchor);
    if (!file) continue;
    const resolved = resolve(dirname(wikiIndexPath), decodeURIComponent(file));
    if (!existsSync(resolved)) {
      violations.push(`docs/wiki/README.md: links a page that does not exist: ${file}`);
      continue;
    }
    // Only pages inside docs/wiki are the wiki's own pages.
    if (file.startsWith("wiki/") || !file.includes("/")) linked.add(resolve(dirname(wikiIndexPath), file));
  }
  for (const page of pages) {
    const isWikiPage = page.startsWith("docs/wiki/") && page !== "docs/wiki/README.md";
    if (!isWikiPage) continue;
    if (!linked.has(join(repoRoot, page))) {
      violations.push(`docs/wiki/README.md: does not link the wiki page ${page}`);
    }
  }
}

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// Rule 5 (issue #424): every module under src/ must be NAMED somewhere in the
// architecture documents (the README table, AGENTS, and the delivered-tree
// table in docs/architecture/rust-service-core.md). The check is by file name
// (`guardrails.rs`), because the tables describe files by name; mod.rs is the directory itself
// and main.rs/lib.rs are crate surfaces, so they are exempt, as is anything
// named tests.rs. A new module that no doc mentions fails CI — the drift this
// rule exists to catch was exactly `fs.rs` outliving its split into `fs/`.
function checkModulesNamed() {
  const listing = readdirSync(join(repoRoot, "src"), {
    recursive: true,
    encoding: "utf8",
  });
  const haystack = ["README.md", "AGENTS.md", "docs/architecture/rust-service-core.md"].map((f) =>
    readFileSync(join(repoRoot, f), "utf8"),
  ).join("\n");
  for (const entry of listing) {
    if (!entry.endsWith(".rs")) continue;
    const name = entry.split("/").pop();
    if (["mod.rs", "main.rs", "lib.rs", "tests.rs"].includes(name)) continue;
    if (!haystack.includes(name)) {
      violations.push(`src/${entry}: named in neither README.md nor AGENTS.md (architecture tables must list every module, issue #424)`);
    }
  }
}

const pages = docsPages();

for (const page of [...pages, "README.md", "AGENTS.md"]) checkLinks(page);
checkSummary(pages);
checkReadmeIndex();
checkWikiIndex(pages);
checkModulesNamed();

if (violations.length > 0) {
  console.error(`docs link check failed with ${violations.length} violation(s):`);
  for (const violation of violations) console.error(`  - ${violation}`);
  process.exit(1);
}
console.log(`docs link check passed (${pages.length} pages)`);
