#!/usr/bin/env bash
# Fails a PR that doesn't add a changie fragment (.changes/unreleased/*.yaml),
# unless it's exempt. See docs/releasing.md.
#
# Usage: check-fragment.sh <base-sha> <head-sha>
# Env:
#   PR_LABELS    - the PR's labels, newline- or comma-separated. A "skip
#                  changelog" label exempts the PR.
#   PR_HEAD_REF  - the PR's head branch name. release/v* is exempt: the
#                  release PR consumes fragments, it doesn't add one.
#
# Operates on the CALLER's cwd git repo, which lets test-checks.sh point it at
# throwaway fixture repos and is what CI wants (cwd is the checkout root).
# Sourced by test-checks.sh; the guard at the bottom keeps main() from running
# when sourced.
set -euo pipefail

# shellcheck source=scripts/release/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# pr_labels_has_skip LABELS (newline- or comma-separated)
pr_labels_has_skip() {
  echo "$1" | tr ',' '\n' | grep -qx 'skip changelog'
}

# diff_adds_fragment BASE HEAD
# Three-dot (merge-base) diff: a release merged into base after the PR
# branched deletes fragments on base, which a two-dot diff would misreport as
# "added" by this PR.
diff_adds_fragment() {
  local added
  added=$(git diff --name-only --diff-filter=A "$1...$2" -- '.changes/unreleased/*.yaml')
  [ -n "$added" ]
}

check_fragment() {
  local base="$1" head="$2"
  local labels="${PR_LABELS:-}"
  local head_ref="${PR_HEAD_REF:-}"

  if ! changie_config_exists_at "$base"; then
    echo "ok: bootstrap (.changie.yaml doesn't exist at base) - this PR introduces the changelog system"
    return 0
  fi
  if pr_head_is_release_branch "$head_ref"; then
    echo "ok: release branch ($head_ref) consumes fragments, doesn't add one"
    return 0
  fi
  if pr_labels_has_skip "$labels"; then
    echo "ok: 'skip changelog' label present"
    return 0
  fi
  if diff_adds_fragment "$base" "$head"; then
    echo "ok: PR adds a .changes/unreleased/*.yaml fragment"
    return 0
  fi

  echo "FAIL: no .changes/unreleased/*.yaml fragment added by this PR." \
    "Run 'changie new' to add one, or apply the 'skip changelog' label if this change has no user-visible effect." >&2
  return 1
}

main() {
  if [ "$#" -ne 2 ]; then
    echo "usage: check-fragment.sh <base-sha> <head-sha>" >&2
    exit 1
  fi
  check_fragment "$1" "$2"
}

if [ "${BUDGEBELL_RELEASE_CHECK_TEST:-0}" != "1" ]; then
  main "$@"
fi
