<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Staying level with upstream

A fork that falls behind is worse than no fork. It offers the reassurance of a
verified build while shipping a version with bugs upstream fixed months ago, and
the reassurance is the part people remember.

So this fork never decides for itself when to update.

## What runs, and when

`.github/workflows/sync-upstream.yml` runs **every three hours**, and on demand.

1. Ask `ventoy/Ventoy` what its default branch is called.
2. Compare it with ours.
3. If upstream has moved, merge it into `main` and push.
4. Ask whether upstream has published a release this fork has not built.
5. If either is true, rebuild every blob, twice.
6. If the rebuild succeeds, sign and publish a release.

Upstream releases a few times a year and commits in bursts. Three hours is often
enough that nobody notices the lag and rare enough to cost almost nothing.

## Why it asks for the branch name

```bash
git ls-remote --symref "$UPSTREAM" HEAD
```

Hardcoding `master` is the obvious thing and it fails in the worst possible way.
If upstream renames its branch, a hardcoded name does not error: it syncs
nothing, silently, for as long as nobody checks. A workflow that goes quiet
looks exactly like a workflow with nothing to do.

## Why it merges rather than rebases

Rebasing onto upstream would rewrite this fork's published history on every
upstream commit. That breaks every clone anybody has made and invalidates every
signature over a commit.

The whole argument of this project is that a reader can check where a byte came
from. History that is rewritten three times a day cannot support that.

## What happens when a merge conflicts

It stops, changes nothing, and opens one issue. Not a new issue every three
hours: it reopens and comments on the existing one, labelled `upstream-sync`.

It does not attempt a resolution. This is a repository full of bootloaders, and
an automatic merge that nobody read is the exact thing the project exists to
argue against.

### Why conflicts are rare

Three files exist in both this fork and upstream: `README.md`, `.gitattributes`
and `.gitignore`. All three are marked `merge=ours` in `.gitattributes`, so an
upstream edit to any of them keeps our version and does not conflict.

That attribute is inert unless the driver is configured:

```bash
git config merge.ours.driver true
```

The workflow does this on every run. Without it, git silently ignores the
attribute and `README.md` conflicts on the first upstream edit, which is a
confusing thing to debug from a cron log. If you are working on this fork
locally, run that command once in your clone.

## Release tags

Releases are tagged `<upstream tag>+repro.N`, for example `1.1.07+repro.1`.

The upstream version comes first because it is what a user is actually looking
for. Inventing a version number of our own would mean nobody could tell which
Ventoy they were holding without reading release notes. The `+repro.N` suffix
increments when this fork rebuilds the same upstream version, which happens when
the build pipeline improves or a toolchain pin changes.

## What a sync does not do

**It does not review upstream's changes.** No human reads the incoming commits
before they land on `main`. This is stated plainly in every merge commit
message, because the alternative is implying a review that did not happen.

The position that leaves you in is the same one you are in running upstream's
own releases, with one difference: here, the binary that results is compiled in
public from the source that was merged, and you can see both.

**It does not publish a failed build.** If the rebuild fails, the merge is on
`main` and no release is cut. The next successful build publishes.

**It does not sign on a fork.** The signing key is a repository secret and is
absent on forks. A fork's build produces every artefact, no signature, and an
`UNSIGNED.md` explaining which of those is missing and why.

## Running it by hand

From the Actions tab, or:

```bash
gh workflow run sync-upstream.yml
gh workflow run sync-upstream.yml -f force_release=true
```

`force_release` rebuilds and republishes even when upstream has not moved. Use
it after changing the build pipeline, when the same upstream version needs a new
`+repro.N`.

## Branches

`main` is the only branch published here. Development happens in a separate
private repository and reaches this one only as a merge into `main`, so the
public history is the history of what was actually released.
