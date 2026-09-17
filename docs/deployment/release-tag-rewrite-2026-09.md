# Release Tag History Rewrite, September 2026

- **Status:** Executed, 2026-09-17
- **Issue:** [#275](https://github.com/TucanoTechnology/TucanoTestAPI/issues/275)
- **Deciders:** repository owner (ECiurleo)
- **Affects:** `main`, the release tags `v1.0.0` and `v1.0.1`, five development branches, and every
  commit published before 2026-09-17

## Why this page exists

`README.md`, `AGENTS.md`, the [deployment guide](deployment-guide.md), the
[canary runbook](canary-validation-and-rollback.md) and the
[operations wiki](../wiki/operations-and-troubleshooting.md) all state, without qualification, that
a release tag is immutable and that the commit SHA is the audit identity. On 2026-09-17 every commit
in this repository was rewritten, which moved both release tags. The policy is unchanged — the
rewrite was a single, explicitly approved exception — but for `v1.0.0` and `v1.0.1` it is no longer
literally true, and an operator who read it without this page would form a false picture of what the
images on GHCR contain and of which commit they were built from.

This page records what moved, what it means for the published images, and how the pre-rewrite
objects are recovered. It changes no route, no field and no stored document.

## Why the rewrite happened

The body of essentially every commit in this repository carried a `Co-authored-by:` trailer naming
an assistant account. GitHub builds a repository's *contributors* list from commit **authors**, so
the trailer never produced a contributor entry — but the line was still present in published commit
messages, and the owner asked for it to be removed from history rather than merely stopped going
forward. Removing text from a commit that already exists means rewriting that commit, and therefore
every commit descended from it.

`AgentRules/coding/branching-and-git.md` allows rewriting shared history only with explicit
approval. The owner gave that approval for a full rewrite of both this repository and
`TucanoTestGUI`; the procedure below is the one that approval covered.

## What changed

Each commit kept its tree, author, author date, committer date and message, and lost only the
trailer lines. Because a commit's identity is a hash of its content, every commit object was
replaced even where its tree was untouched, so all SHAs below changed. The refs that were published
on `origin` map as follows.

| Ref | Commit before | Commit after |
| --- | --- | --- |
| `main` (tip at the rewrite) | `ccc43618ef567a27bc85a60e6f4c2a22c65dbcd0` | `800bc678431fe78112c4b9126145b3546e1d0e3e` |
| `v1.0.0` | `932eceba24f8a8329f6fa44f6f4630da296c4508` | `041259e2901d2a2b784445a227fc5a6c9b601a40` |
| `v1.0.1` | `4f06812b08dcbf76efcd9174207cb8859f2c2856` | `88247e9ed75db3f326d74cf7d369ec69d23f9c9f` |
| `add-agpl-license` | `04abb1794fe384d7b5003e9cfa2109295a02e17b` | `5eb8fb57145bcda21b0e9077ca0cc394c2ff1ec8` |
| `ao/tucanotestapi-159/issue-97-benchmark-wip` | `00afc1e05c7e44291a248584f562cbd3bfde66f3` | `6b6a92112f27fcd5d9cb6a4d869726bd82e186a8` |
| `ao/tucanotestapi-163/ci-rustup-dns-robustness` | `cd1e4da25b7c97c37e596799ea2b684ec7e11b2a` | `a5a154f9efe405023d9cb70a50d4088e30b8f25f` |
| `ao/tucanotestapi-164/fix-agents-rules-links` | `12b45a222504a4edc7b19f96c8cd9bafbc8e615c` | `460d7d405f0bb17c4ecbda50546621df0654d7a8` |
| `chore/sync-agent-rules-6` | `59dd588d80807d61f9cdadde393c37c0391b96d8` | `d5d441cb088e0c1237673e9bb6bd25d3ddd7d252` |

`main` has since advanced to `65afb1dcc3d179d53118a6e8fe54d68ca866d0ea` (the ordinary merge of
[#274](https://github.com/TucanoTechnology/TucanoTestAPI/pull/274)), which is a descendant of
`800bc678` and was not part of the rewrite.

**No published content changed.** For every row above, the rewritten commit has the *same tree* as
the commit it replaced, which is the strongest available statement that the rewrite removed text and
nothing else. For the three refs that carry releases:

| Ref | Tree (identical before and after) |
| --- | --- |
| `main` | `34698dd0cd78a13905897c6ab26df53427e6ff5b` |
| `v1.0.0` | `dc46a13e8cffc3c6da720312cb923ae795ef81ba` |
| `v1.0.1` | `76011b5390b281172b2927ef5ba111345d3894a6` |

### One branch tip was replaced by a different tree

`add-agpl-license` is the one row whose tree is not identical. Its published tip before the rewrite
(`04abb179`) was a *merge of `main` into the branch*, so it carried `main`'s then-current content;
the rewritten tip (`5eb8fb57`) is the rewritten AGPL commit and does not carry that merge. Nothing
unique was lost: the branch's pull request
([#274](https://github.com/TucanoTechnology/TucanoTestAPI/pull/274)) is merged, and every commit the
merge had brought in is present on `main`, whose tree is unchanged. The branch is spent, and
`AgentRules/coding/branching-and-git.md` calls for deleting a branch once its pull request is
merged, which would also remove the anomaly.

## What this means for the published images

The release workflow builds its image labels with `docker/metadata-action`, which sets
`org.opencontainers.image.revision` to the SHA that triggered the run, unconditionally. The images
already on GHCR therefore identify the commit they were built from by a SHA that the rewrite
replaced:

| Published image tags | Release run | Built from | `org.opencontainers.image.revision` |
| --- | --- | --- | --- |
| `:v1.0.0`, `:build-117` | [#117](https://github.com/TucanoTechnology/TucanoTestAPI/actions/runs?query=branch%3Av1.0.0) | `932eceba` | `932eceba…`, no longer reachable from any ref |
| `:v1.0.1`, `:build-138` | #138 | `4f06812b` | `4f06812b…`, no longer reachable from any ref |
| `:build-137` | #137 | `4f06812b` | `4f06812b…`, same |
| `:build-139` | #139 | `ccc43618` | `ccc43618…`, same |

The practical consequences are narrow:

- **Deploying and rolling back are unaffected.** Both images are byte-identical in content to what
  the tags resolve to now, so `:v1.0.0`, `:v1.0.1` and their `build-<run number>` tags still identify
  the same software. Nothing needs re-publishing, and nothing was re-published.
- **SHA-based provenance is degraded.** A revision label pointing at a replaced SHA no longer
  resolves to a commit reachable from any branch or tag. Those objects are still readable *today* —
  GitHub retains unreachable objects for a period — so the chain can still be followed for now, but
  GitHub may collect them, after which the label can only be resolved against the recovery bundle
  below. Treat the image tag, not the revision label, as what you deploy.
- **Images built from `main` after the rewrite are unaffected.** Their revision label points at a
  commit that is still reachable.

As configured, `metadata-action` also sets `org.opencontainers.image.version` from the generated tag
name, and `release.yml` passes those labels to the build, which overrides the Dockerfile's
`BUILD_NUMBER` value for the same key. That is a property of the workflow rather than of the
rewrite, and it could not be confirmed against the registry from here because reading
`ghcr.io/tucanotechnology/tucanotestapi` requires the `read:packages` scope, which was not available.
Confirm it against the registry before relying on the `version` label, and read
`org.opencontainers.image.revision` when you need the source commit.

## What was deliberately not done

- **The published images were not re-tagged or deleted.** Doing so needs the `delete:packages`
  scope, which was not available, and it is not required: their content is unaffected.
- **`TucanoTestGUI` is a separate rewrite.** It is the companion repository and was rewritten under
  the same approval; its own record belongs to its own repository.

## Recovery

Before any ref was rewritten, every ref then present was written to an out-of-tree `git bundle`
(`TucanoTestAPI-pre-rewrite-<timestamp>.bundle`), kept outside the repository by the maintainer who
performed the rewrite. That bundle — not `refs/original`, which `filter-branch` overwrites on each
run and which now holds only the single branch it processed last — is the rollback path for the
pre-rewrite objects. To recover one:

```sh
git bundle list-heads /path/to/TucanoTestAPI-pre-rewrite-<timestamp>.bundle   # what it contains
git fetch /path/to/TucanoTestAPI-pre-rewrite-<timestamp>.bundle refs/heads/main:refs/heads/pre-rewrite-main
```

The bundle must be preserved for as long as an `org.opencontainers.image.revision` on a published
image may need resolving. The local rollback ref that `filter-branch` left behind
(`refs/original/refs/heads/chore/sync-agent-rules-6`, holding 31 pre-rewrite commits) is the only
other in-repository copy and is not published; it can be deleted once the bundle is confirmed,
and it is safe to ignore in the meantime because nothing reaches it from a live branch or tag.
