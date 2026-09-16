#!/usr/bin/env node
//
// Mirror the user wiki into the repository's GitHub Wiki (issue #236).
//
// The wiki decision record (docs/architecture/wiki-structure-and-publication.md)
// makes the in-repo `docs/` tree the only source of truth and keeps GitHub Wiki
// as a *mirror*, not an editor: the Wiki tab is what readers reach first, but a
// page is still authored in docs/, reviewed in a pull request, and copied here
// as a build step. Nothing is ever edited in the wiki repository directly — the
// next publish overwrites it.
//
// GitHub Wiki is a git repository (TucanoTestAPI.wiki.git) whose pages are flat:
//   Home.md, Installation-and-first-project.md, ...
// A link between wiki pages is therefore a bare <Page-Name> (or [[Page Name]]),
// not a relative path, and there is no subdirectory structure. This script
// flattens docs/wiki/*.md to that namespace and rewrites the cross-page links.
//
// Usage:
//   node scripts/sync-github-wiki.mjs --out <dir>   # stage the pages
//   node scripts/sync-github-wiki.mjs --check       # verify the mapping only
//
// --out writes the staged pages and starts nothing; --check reads files only.
// Both are safe on a pull request. The caller publishes the staged directory;
// this script never pushes.

import { readFileSync, writeFileSync, mkdirSync, rmSync, existsSync, readdirSync } from "node:fs";
import { dirname, resolve, join, basename } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const docsRoot = join(repoRoot, "docs");
const wikiRoot = join(docsRoot, "wiki");
const summaryPath = join(docsRoot, "SUMMARY.md");

// The wiki's own pages: everything under docs/wiki/. The index becomes Home.
const WIKI_INDEX = "wiki/README.md";
// Pages that live outside docs/wiki but belong in the reader-facing wiki: the
// generated operation reference and the deployment guide a sysadmin lands on.
const EXTRA_PAGES = [
  ["generated/operations-reference.md", "Operation-reference"],
  ["deployment/deployment-guide.md", "Deployment-guide"],
];

const args = process.argv.slice(2);
const outFlag = args.indexOf("--out");
const outDir = outFlag === -1 ? null : args[outFlag + 1];
const checkOnly = args.includes("--check");

if (!outDir && !checkOnly) {
  console.error("usage: node scripts/sync-github-wiki.mjs --out <dir> | --check");
  process.exit(2);
}

/** The wiki pages on disk, as docs/-relative paths, excluding the index. */
function wikiPages() {
  return readdirSync(wikiRoot)
    .filter((name) => name.endsWith(".md") && name !== "README.md")
    .map((name) => `wiki/${name}`)
    .sort();
}

/**
 * Map a docs/-relative source path to its flat GitHub Wiki page name.
 * docs/wiki/getting-started.md -> Getting-started
 * docs/wiki/README.md          -> Home
 */
function pageName(sourcePath) {
  if (sourcePath === WIKI_INDEX) return "Home";
  const stem = basename(sourcePath, ".md");
  return stem.charAt(0).toUpperCase() + stem.slice(1);
}

/**
 * Rewrite links for the flat wiki namespace.
 *
 * A relative link to a sibling wiki page becomes a bare page name; a link to a
 * page that is not mirrored keeps pointing at the file in the repository, which
 * is the honest destination for a reader who followed it from the wiki.
 */
const LINK = /\[([^\]]*)\]\(([^)\s]+)((?:\s+"[^"]*")?)\)/g;

function rewriteLinks(source, sourcePath) {
  const sourceDir = dirname(join(docsRoot, sourcePath));
  return source.replace(LINK, (whole, text, target, title) => {
    if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith("#") || target.startsWith("//")) {
      return whole;
    }
    const anchor = target.indexOf("#");
    const file = anchor === -1 ? target : target.slice(0, anchor);
    const fragment = anchor === -1 ? "" : target.slice(anchor + 1);
    if (!file) return whole;

    const absolute = resolve(sourceDir, decodeURIComponent(file));
    let sourceForTarget = null;
    if (absolute.startsWith(docsRoot) && existsSync(absolute)) {
      sourceForTarget = absolute.slice(docsRoot.length + 1).split("\\").join("/");
    }

    // A mirrored page becomes a flat wiki link.
    if (sourceForTarget && pageNameOrNull(sourceForTarget)) {
      const name = pageNameOrNull(sourceForTarget);
      const space = name.includes("-") ? `[[${name.replace(/-/g, " ")}]]` : `[[${name}]]`;
      return space;
    }

    // Anything else (a directory, a file outside docs/) points at the
    // repository, where the reader can actually open it.
    if (sourceForTarget) {
      const url = `https://github.com/TucanoTechnology/TucanoTestAPI/blob/main/docs/${sourceForTarget}${fragment ? `#${fragment}` : ""}`;
      return `[${text}](${url}${title})`;
    }

    // A link that climbs out of docs/ (../../README.md, ../../AGENTS.md) has no
    // wiki equivalent: resolve it against the repository root instead so the
    // reader reaches the real file.
    const fromRepoRoot = resolve(sourceDir, decodeURIComponent(file));
    if (fromRepoRoot.startsWith(repoRoot) && existsSync(fromRepoRoot)) {
      const repoRelative = fromRepoRoot.slice(repoRoot.length + 1).split("\\").join("/");
      const url = `https://github.com/TucanoTechnology/TucanoTestAPI/blob/main/${repoRelative}${fragment ? `#${fragment}` : ""}`;
      return `[${text}](${url}${title})`;
    }
    return whole;
  });
}

/** The wiki page name for a docs/-relative path, or null if it is not mirrored. */
function pageNameOrNull(sourcePath) {
  if (sourcePath === WIKI_INDEX) return "Home";
  if (wikiPages().includes(sourcePath)) return pageName(sourcePath);
  const extra = EXTRA_PAGES.find(([from]) => from === sourcePath);
  if (extra) return extra[1];
  return null;
}

const pages = wikiPages();
const staged = [];

for (const sourcePath of [WIKI_INDEX, ...pages, ...EXTRA_PAGES.map(([from]) => from)]) {
  const absolute = join(docsRoot, sourcePath);
  if (!existsSync(absolute)) {
    console.error(`sync-github-wiki: source page is missing: docs/${sourcePath}`);
    process.exit(1);
  }
  staged.push({ name: pageName(sourcePath), sourcePath, body: rewriteLinks(readFileSync(absolute, "utf8"), sourcePath) });
}

// A flat namespace means two pages must not collide on the same name.
const names = new Set();
for (const page of staged) {
  if (names.has(page.name)) {
    console.error(`sync-github-wiki: two pages map to the wiki name ${page.name}`);
    process.exit(1);
  }
  names.add(page.name);
}

if (outDir) {
  const target = resolve(outDir);
  rmSync(target, { recursive: true, force: true });
  mkdirSync(target, { recursive: true });
  for (const page of staged) writeFileSync(join(target, `${page.name}.md`), page.body);
  // A sidebar gives the flat namespace the ordering SUMMARY.md carries.
  writeFileSync(join(target, "_Sidebar.md"), sidebar(wikiPages()));
  console.log(`sync-github-wiki: staged ${staged.length} page(s) plus _Sidebar.md in ${target}`);
} else {
  console.log(`sync-github-wiki: ${staged.length} page(s) map cleanly onto the wiki namespace`);
}

/** The wiki sidebar, derived from SUMMARY.md's User wiki section. */
function sidebar(pages) {
  const summary = readFileSync(summaryPath, "utf8");
  const section = summary.split("# User wiki")[1]?.split("\n#")[0] ?? "";
  const lines = [];
  for (const match of section.matchAll(/\[([^\]]+)\]\(([^)\s]+)\)/g)) {
    const name = pageNameOrNull(match[2]);
    if (name) lines.push(`- [${match[1]}](${name})`);
  }
  for (const [from, name] of EXTRA_PAGES) {
    if (existsSync(join(docsRoot, from))) {
      const stem = basename(from, ".md");
      lines.push(`- [${stem.split("-").map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(" ")}](${name})`);
    }
  }
  return `${lines.join("\n")}\n`;
}
