#!/usr/bin/env bash
# Unit tests for scripts/release/check-fragment.sh, check-no-version-bump.sh
# and check-version-consistency.sh — run with `bash scripts/release/test-checks.sh`
# (CI runs it too).
#
# Each script is SOURCED (not executed) with BUDGEBELL_RELEASE_CHECK_TEST=1 so
# its functions are callable and its own main()/argv guard doesn't fire.
#
# The two diff-based checks run against real throwaway git repos built fresh
# per test in a temp dir: no network, no GitHub API. PR_LABELS/PR_HEAD_REF are
# plain env vars; base and head are real commit shas in the fixture repo.
set -euo pipefail

cd "$(dirname "$0")/../.."
REPO_ROOT="$(pwd)"
SCRIPTS="$REPO_ROOT/scripts/release"

export BUDGEBELL_RELEASE_CHECK_TEST=1
failures=0
tests_run=0
OUT="$(mktemp)"
FIXTURES=()
cleanup() { rm -f "$OUT"; [ "${#FIXTURES[@]}" -eq 0 ] || rm -rf "${FIXTURES[@]}"; }
trap cleanup EXIT

# assert_ok DESCRIPTION COMMAND...
assert_ok() {
  tests_run=$((tests_run + 1))
  local desc="$1"
  shift
  if "$@" >"$OUT" 2>&1; then
    echo "ok: $desc"
  else
    echo "FAIL: $desc (expected pass, got failure)"
    sed 's/^/    /' "$OUT"
    failures=$((failures + 1))
  fi
}

# assert_fails DESCRIPTION COMMAND...
assert_fails() {
  tests_run=$((tests_run + 1))
  local desc="$1"
  shift
  if "$@" >"$OUT" 2>&1; then
    echo "FAIL: $desc (expected failure, got pass)"
    sed 's/^/    /' "$OUT"
    failures=$((failures + 1))
  else
    echo "ok: $desc"
  fi
}

# --- fixture repo builder ------------------------------------------------
#
# Every fixture starts with a "base" commit that has a real .changie.yaml,
# all four version locations at 0.1.0, and a .changes/v0.1.0.md, so the
# bootstrap path is never accidentally exercised by tests that aren't
# specifically testing bootstrap.

FIXTURE_DIR=""

# write_versions DIR VERSION — (re)writes the version locations in DIR.
write_versions() {
  local dir="$1" v="$2"
  mkdir -p "$dir/src-tauri"
  cat >"$dir/package.json" <<EOF2
{
  "name": "fixture",
  "version": "$v"
}
EOF2
  cat >"$dir/src-tauri/tauri.conf.json" <<EOF2
{
  "productName": "Fixture",
  "version": "$v"
}
EOF2
  cat >"$dir/src-tauri/Cargo.toml" <<EOF2
[package]
name = "budgebell"
version = "$v"
edition = "2021"

[dependencies]
serde = { version = "1", features = [] }
EOF2
  cat >"$dir/src-tauri/Cargo.lock" <<EOF2
version = 4

[[package]]
name = "aaa"
version = "9.9.9"

[[package]]
name = "budgebell"
version = "$v"
dependencies = [
 "aaa",
]
EOF2
}

new_fixture() {
  FIXTURE_DIR="$(mktemp -d)"
  (
    cd "$FIXTURE_DIR"
    git init -q
    git config user.email test@example.com
    git config user.name "Release Check Tests"
    mkdir -p .changes/unreleased
    cat >.changie.yaml <<'EOF2'
changesDir: .changes
unreleasedDir: unreleased
EOF2
    write_versions "$FIXTURE_DIR" 0.1.0
    cat >.changes/v0.1.0.md <<'EOF2'
## v0.1.0 - 2026-01-01

### Added

* Initial release.
EOF2
    git add -A
    git commit -q -m "base"
  )
  FIXTURES+=("$FIXTURE_DIR")
}

fixture_git() { git -C "$FIXTURE_DIR" "$@"; }
commit_all() { (cd "$FIXTURE_DIR" && git add -A && git commit -q -m "$1"); }

# run_check SCRIPT FUNCTION BASE_REV HEAD_REV — runs in the fixture repo,
# inheriting PR_LABELS / PR_HEAD_REF from the caller's environment.
run_check() {
  local script="$1" fn="$2" base="$3" head="$4"
  (
    cd "$FIXTURE_DIR"
    # shellcheck source=/dev/null
    source "$SCRIPTS/$script"
    "$fn" "$(git rev-parse "$base")" "$(git rev-parse "$head")"
  )
}

frag() { # frag NAME — drop a fragment file
  cat >"$FIXTURE_DIR/.changes/unreleased/$1.yaml" <<'EOF2'
kind: added
body: something
EOF2
}

frag_check() { run_check check-fragment.sh check_fragment "$@"; }
bump_check() { run_check check-no-version-bump.sh check_no_version_bump "$@"; }
consistency() {
  (
    # shellcheck source=/dev/null
    source "$SCRIPTS/check-version-consistency.sh"
    check_version_consistency "$FIXTURE_DIR"
  )
}

# ==========================================================================
echo "== check-fragment.sh =="

new_fixture; fixture_git tag base
frag added-1; commit_all "add fragment"
assert_ok "fragment added -> pass" frag_check base HEAD

new_fixture; fixture_git tag base
echo unrelated >"$FIXTURE_DIR/README.md"; commit_all "no fragment"
assert_fails "no fragment, no label, no release branch -> fail" frag_check base HEAD
PR_LABELS='bug,skip changelog' assert_ok "'skip changelog' label -> pass" frag_check base HEAD
PR_LABELS=$'bug\nskip changelog' assert_ok "newline-separated labels include skip -> pass" frag_check base HEAD
PR_LABELS='bug' assert_fails "unrelated label only -> fail" frag_check base HEAD
PR_LABELS='skip changelog please' assert_fails "label that merely contains the phrase -> fail" frag_check base HEAD
PR_HEAD_REF='release/v0.2.0' assert_ok "head ref release/v* -> pass" frag_check base HEAD
PR_HEAD_REF='feature/release/v1' assert_fails "head ref that merely contains release/v -> fail" frag_check base HEAD

new_fixture; fixture_git tag base
frag not-a-yaml; mv "$FIXTURE_DIR/.changes/unreleased/not-a-yaml.yaml" "$FIXTURE_DIR/.changes/unreleased/note.txt"
commit_all "non-yaml file in unreleased"
assert_fails "only a non-.yaml file under unreleased -> fail" frag_check base HEAD

new_fixture
(cd "$FIXTURE_DIR" && git rm -q .changie.yaml && git commit -q -m "no changie yet")
fixture_git tag base
printf 'changesDir: .changes\n' >"$FIXTURE_DIR/.changie.yaml"; commit_all "introduce changie, no fragment"
assert_ok "bootstrap: .changie.yaml missing at base -> pass" frag_check base HEAD

# A release merged into base after the PR branched deletes fragments on base;
# a two-dot diff would report them as "added" by the PR.
new_fixture
frag other-pr; commit_all "an earlier PR's fragment, already on base"
fixture_git tag pr-point
echo unrelated >"$FIXTURE_DIR/README.md"; commit_all "PR branch: no fragment of its own"
fixture_git tag head
fixture_git checkout -q pr-point
rm "$FIXTURE_DIR/.changes/unreleased/other-pr.yaml"; echo whatever >"$FIXTURE_DIR/CHANGELOG.md"
commit_all "release commit lands on base"; fixture_git tag base
assert_fails "base advances (release deletes fragments), PR adds none -> fail" frag_check base head

echo
echo "== check-no-version-bump.sh =="

new_fixture; fixture_git tag base
frag added-1; commit_all "normal PR"
assert_ok "normal PR, nothing touched -> pass" bump_check base HEAD

new_fixture; fixture_git tag base
echo "# Changelog" >"$FIXTURE_DIR/CHANGELOG.md"; commit_all "hand-edit changelog"
assert_fails "PR touches CHANGELOG.md -> fail" bump_check base HEAD
PR_HEAD_REF='release/v0.2.0' assert_ok "release/v* head ref exempts CHANGELOG.md -> pass" bump_check base HEAD

new_fixture; fixture_git tag base
printf '## v0.2.0 - 2026-02-01\n' >"$FIXTURE_DIR/.changes/v0.2.0.md"; commit_all "hand-add release notes"
assert_fails "PR adds a .changes/v*.md -> fail" bump_check base HEAD

new_fixture; fixture_git tag base
sed -i 's/"version": "0.1.0"/"version": "0.2.0"/' "$FIXTURE_DIR/package.json"; commit_all "hand-bump package.json"
assert_fails "PR bumps package.json version -> fail" bump_check base HEAD

new_fixture; fixture_git tag base
sed -i 's/"version": "0.1.0"/"version": "0.2.0"/' "$FIXTURE_DIR/src-tauri/tauri.conf.json"; commit_all "hand-bump tauri.conf.json"
assert_fails "PR bumps tauri.conf.json version -> fail" bump_check base HEAD

new_fixture; fixture_git tag base
sed -i '0,/^version = "0.1.0"/s//version = "0.2.0"/' "$FIXTURE_DIR/src-tauri/Cargo.toml"; commit_all "hand-bump Cargo.toml"
assert_fails "PR bumps Cargo.toml package version -> fail" bump_check base HEAD

new_fixture; fixture_git tag base
sed -i 's/^serde = .*/serde = { version = "2", features = [] }/' "$FIXTURE_DIR/src-tauri/Cargo.toml"; commit_all "bump a dependency version"
assert_ok "PR bumps a dependency's version in Cargo.toml (not the package) -> pass" bump_check base HEAD

new_fixture; fixture_git tag base
sed -i 's/"name": "fixture"/"name": "renamed"/' "$FIXTURE_DIR/package.json"; commit_all "unrelated package.json edit"
assert_ok "PR edits package.json but not .version -> pass" bump_check base HEAD

new_fixture; fixture_git tag base
frag added-1; commit_all "add fragment"; fixture_git tag base2
rm "$FIXTURE_DIR/.changes/unreleased/added-1.yaml"; commit_all "delete my own fragment"
assert_ok "PR deletes its own unreleased fragment -> pass" bump_check base2 HEAD

new_fixture
(cd "$FIXTURE_DIR" && git rm -q .changie.yaml && git commit -q -m "no changie yet")
fixture_git tag base
printf 'changesDir: .changes\n' >"$FIXTURE_DIR/.changie.yaml"
echo "# Changelog" >"$FIXTURE_DIR/CHANGELOG.md"; commit_all "introduce changie + changelog"
assert_ok "bootstrap: .changie.yaml missing at base -> pass" bump_check base HEAD

# A release landing on base after the PR branched (CHANGELOG.md, .changes/vX,
# version bumps) is not this PR's doing; the merge-base form must pass it.
new_fixture; fixture_git tag pr-point
frag added-1; commit_all "PR branch: unrelated fragment only"; fixture_git tag head
fixture_git checkout -q pr-point
echo whatever >"$FIXTURE_DIR/CHANGELOG.md"
printf '## v0.2.0 - 2026-02-01\n' >"$FIXTURE_DIR/.changes/v0.2.0.md"
write_versions "$FIXTURE_DIR" 0.2.0
commit_all "release commit lands on base"; fixture_git tag base
assert_ok "base advances (release commit) after PR branches -> pass" bump_check base head

echo
echo "== check-version-consistency.sh =="

new_fixture
assert_ok "all versions match latest .changes/v*.md -> pass" consistency

new_fixture
printf '## v0.2.0 - 2026-02-01\n' >"$FIXTURE_DIR/.changes/v0.2.0.md"
assert_fails "latest release note ahead of the version files -> fail" consistency

for f in package.json src-tauri/tauri.conf.json; do
  new_fixture
  sed -i 's/"version": "0.1.0"/"version": "9.9.9"/' "$FIXTURE_DIR/$f"
  assert_fails "$f out of sync -> fail" consistency
done

new_fixture
sed -i '0,/^version = "0.1.0"/s//version = "9.9.9"/' "$FIXTURE_DIR/src-tauri/Cargo.toml"
assert_fails "Cargo.toml out of sync -> fail" consistency

new_fixture
sed -i '/name = "budgebell"/{n;s/0.1.0/9.9.9/}' "$FIXTURE_DIR/src-tauri/Cargo.lock"
assert_fails "Cargo.lock's own package entry out of sync -> fail" consistency

new_fixture
rm "$FIXTURE_DIR/src-tauri/Cargo.lock"
assert_fails "Cargo.lock missing -> fail" consistency

new_fixture
rm -rf "$FIXTURE_DIR/.changes"
assert_fails "no .changes/v*.md at all -> fail" consistency

new_fixture
printf '## v0.10.0 - 2026-03-01\n' >"$FIXTURE_DIR/.changes/v0.10.0.md"
write_versions "$FIXTURE_DIR" 0.10.0
assert_ok "v0.10.0 sorts after v0.1.0 (version sort, not lexical) -> pass" consistency

new_fixture
printf '## v1.0.0 - 2026-04-01\n' >"$FIXTURE_DIR/.changes/v1.0.0.md"
printf '## v1.0.0-rc1 - 2026-03-15\n' >"$FIXTURE_DIR/.changes/v1.0.0-rc1.md"
write_versions "$FIXTURE_DIR" 1.0.0
assert_ok "v1.0.0-rc1 sorts before v1.0.0, v1.0.0 is latest -> pass" consistency

new_fixture
printf '## v1.0.0-rc1 - 2026-03-15\n' >"$FIXTURE_DIR/.changes/v1.0.0-rc1.md"
write_versions "$FIXTURE_DIR" 1.0.0-rc1
assert_ok "prerelease-only cut: v1.0.0-rc1 is latest -> pass" consistency

new_fixture
printf '## v1.0.0-rc1 - 2026-03-01\n' >"$FIXTURE_DIR/.changes/v1.0.0-rc1.md"
printf '## v1.0.0-rc2 - 2026-03-15\n' >"$FIXTURE_DIR/.changes/v1.0.0-rc2.md"
write_versions "$FIXTURE_DIR" 1.0.0-rc2
assert_ok "v1.0.0-rc2 sorts after v1.0.0-rc1 -> pass" consistency

new_fixture
printf '## v0.2.0 - 2026-02-01\n' >"$FIXTURE_DIR/.changes/v0.2.0.md"
printf '## v0.2.0-rc1 - 2026-01-15\n' >"$FIXTURE_DIR/.changes/v0.2.0-rc1.md"
assert_ok "--latest prints the highest version (v0.2.0)" env -u BUDGEBELL_RELEASE_CHECK_TEST bash -c "[ \"\$(bash '$SCRIPTS/check-version-consistency.sh' --latest '$FIXTURE_DIR')\" = v0.2.0 ]"

echo
echo "$tests_run tests run, $failures failed"
[ "$failures" -eq 0 ]
