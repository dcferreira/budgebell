# Releasing

Budgebell keeps a per-PR changelog with [changie](https://changie.dev) and cuts releases through a
bot-opened Release PR. Nobody edits `CHANGELOG.md`, a `.changes/v*.md` file, or a version number by
hand in a normal PR.

The version lives in four places, all bumped together by the Release PR workflow:

- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock` (the `budgebell` entry; refreshed with `cargo update -p budgebell`)

The first three are rewritten by changie's `replacements:` (see `.changie.yaml`).

## Writing a fragment

Every normal PR adds one fragment under `.changes/unreleased/`:

```sh
changie new --kind added --body "Short, user-facing description of the change." -i=false
```

`changie new` prompts by default, which hangs without a TTY, so scripts and agents pass `-i=false`.
Install changie with `go install github.com/miniscruff/changie@v1.26.0` (or see its docs).

`--kind` takes the lowercase key (`added`, `breaking`, `changed`, `deprecated`, `removed`, `fixed`,
`security`), not the capitalised label. Optionally add `--custom PR=<number>` to link the entry to
its PR once the number exists.

| Kind | Use for |
|---|---|
| `breaking` | A change that breaks something users already depend on (for example stored data or MCP tool shapes). |
| `added` | A new capability. |
| `changed` | A behaviour change that isn't breaking. |
| `deprecated` | Still works, on its way out. |
| `removed` | Used to work, no longer does. |
| `fixed` | A bug fix. |
| `security` | A security-relevant fix. |

Before 1.0.0, every kind except `fixed`/`security` bumps the **minor** version; `fixed` and
`security` bump the patch version. Switch `breaking` to `auto: major` in `.changie.yaml` at 1.0.0.

### No fragment needed

If a PR has no user-visible effect (refactor, CI tweak, docs typo), apply the **`skip changelog`**
label instead. `.github/workflows/changelog.yml` requires one or the other on every PR.

## What CI enforces

- `scripts/release/check-fragment.sh` fails a PR that adds no `.changes/unreleased/*.yaml`
  fragment, unless it has the `skip changelog` label or comes from a `release/v*` branch.
- `scripts/release/check-no-version-bump.sh` fails a PR that touches `CHANGELOG.md`, changes a
  `.changes/v*.md` file, or changes the version in `package.json`, `tauri.conf.json` or
  `Cargo.toml`. `release/v*` branches are exempt.
- `scripts/release/check-version-consistency.sh` (in `ci.yml`, on every PR and on `main`) fails
  unless all four version locations equal the highest version under `.changes/`.
- `scripts/release/test-checks.sh` unit-tests those three scripts against throwaway git repos.

The `release/v*` exemption is keyed on the branch name only. It is a convenience, not a security
boundary: anyone who can push a branch can already edit the workflows.

## Cutting a release

1. Go to **Actions, Release PR, Run workflow** with `main` selected. Leave `version` empty to let
   changie choose the bump from the fragments' kinds, or set it (`1.2.3`, `minor`, `major`,
   `patch`).
2. The workflow batches the fragments into `.changes/vX.Y.Z.md`, regenerates `CHANGELOG.md`, bumps
   the versions, refreshes `Cargo.lock`, and opens a `release/vX.Y.Z` PR.
3. Review it like any PR (read the notes, fix wording by editing the PR directly) and merge.
4. The merge touches `CHANGELOG.md` on `main`, which triggers `.github/workflows/release.yml`. It
   tags `vX.Y.Z`, creates a **draft** GitHub release whose body is `.changes/vX.Y.Z.md`, and builds
   unsigned bundles in parallel: Linux (AppImage, deb, rpm), macOS (arm64 and x86_64) and Windows
   (msi and nsis), uploading them to the draft.
5. Open the draft on the Releases page, sanity-check the assets, and publish it.

Both workflows only run from `main`, even though they can be dispatched from the Actions UI.

### The first release (v0.1.0)

`.changes/v0.1.0.md` and `CHANGELOG.md` were written by hand when release management was added, and
the version files are already at 0.1.0. Merging that PR touches `CHANGELOG.md` on `main`, so
`release.yml` runs on its own and creates the `v0.1.0` tag and draft release; no Release PR (and no
GitHub App) is needed for it. If that run needs repeating, use **Actions, Release, Run workflow** on
`main`, provided nothing else has landed since (see Recovery), or **Re-run failed jobs** on the
original run. The pending fragment from that PR means the next Release PR will propose v0.2.0.

## One-time setup: the GitHub App

`release-pr.yml` opens its PR with a GitHub App installation token instead of the default
`GITHUB_TOKEN`, because a PR opened with the default token triggers no workflow runs (GitHub's
anti-recursion rule), so CI would never run on the release PR.

Once per repo:

1. Create (or reuse) a GitHub App with **Contents: read and write** and **Pull requests: read and
   write** permissions.
2. Install it on this repository.
3. Add its App ID and a generated private key as the repo secrets `RELEASE_APP_ID` and
   `RELEASE_APP_PRIVATE_KEY`.

Also create the `skip changelog` label, and consider making `Fragment + no hand-bumped version`
(changelog.yml) and the CI jobs required status checks on `main`.

## Recovery

`release.yml` is safe to re-run (**Re-run failed jobs**, or **Run workflow** on `main`):

- A published release already exists: it does nothing.
- The tag exists but there is no release, or only a draft: it keeps the tag, refreshes the draft's
  notes, and builds from the tagged commit (not main's current tip), replacing same-named assets.
- Neither exists: it tags and creates the draft. On a manual dispatch this only proceeds if `main`'s
  tip is still the commit that last changed `CHANGELOG.md`.

Never push a `v*` tag by hand. The workflow doesn't trigger on tag pushes, so a hand-pushed tag
would publish nothing and leave a stray tag with no matching `.changes/vX.Y.Z.md`.

## Unsigned builds

Bundles are not code-signed or notarized yet. See the README's Install section for the macOS
Gatekeeper and Windows SmartScreen workarounds.
