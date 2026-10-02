#!/usr/bin/env bash
# Fails unless a release's latest.json (the in-app updater's manifest) names
# the release's version and has a signed entry for every platform Budgebell
# ships. Guards release.yml's publish job: the four build jobs each merge
# their platform into one shared latest.json, so a lost race would publish a
# manifest that silently never offers some platform the update. See
# docs/releasing.md.
#
# Usage: check-updater-json.sh VERSION < latest.json
#   VERSION is the release tag (vX.Y.Z); latest.json carries it without the v.
#
# Sourced by test-checks.sh; the guard at the bottom keeps main() from running
# when sourced.
set -euo pipefail

# The `{os}-{arch}` keys the updater falls back to when no installer-specific
# key (e.g. linux-x86_64-appimage) matches; tauri-action writes one per build.
REQUIRED_PLATFORMS=(
  linux-x86_64
  darwin-aarch64
  darwin-x86_64
  windows-x86_64
)

# check_updater_json VERSION (latest.json on stdin)
check_updater_json() {
  local expected="${1#v}" json
  json=$(cat)
  if ! jq -e 'type == "object"' >/dev/null 2>&1 <<<"$json"; then
    echo "::error::latest.json is not a JSON object." >&2
    return 1
  fi

  local version
  version=$(jq -r '.version // ""' <<<"$json")
  if [ "${version#v}" != "$expected" ]; then
    echo "::error::latest.json is for version '${version}', not ${expected}." >&2
    return 1
  fi

  local platform
  local -a missing=()
  for platform in "${REQUIRED_PLATFORMS[@]}"; do
    jq -e --arg p "$platform" \
      '.platforms[$p] | (.url // "") != "" and (.signature // "") != ""' \
      >/dev/null 2>&1 <<<"$json" || missing+=("$platform")
  done
  if [ "${#missing[@]}" -gt 0 ]; then
    echo "::error::latest.json has no signed entry for: ${missing[*]}." >&2
    return 1
  fi
  echo "ok: latest.json ${expected} covers all ${#REQUIRED_PLATFORMS[@]} platforms"
}

main() {
  check_updater_json "$@"
}

if [ "${BUDGEBELL_RELEASE_CHECK_TEST:-0}" != "1" ]; then
  main "$@"
fi
