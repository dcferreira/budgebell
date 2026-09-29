#!/usr/bin/env bash
# Fails unless every version location matches the latest released version
# recorded under .changes/: package.json, src-tauri/tauri.conf.json,
# src-tauri/Cargo.toml ([package]) and the budgebell entry in
# src-tauri/Cargo.lock. Runs in CI on every PR and on main, and in
# release.yml / release-pr.yml as a sanity check.
#
# Usage: check-version-consistency.sh [repo-dir]   (default ".")
#        check-version-consistency.sh --latest [repo-dir]
#   --latest just prints the latest release version (vX.Y.Z), for release.yml.
#
# Deliberately doesn't shell out to changie: "latest" is the highest semver
# among .changes/v*.md filenames. Plain `sort -V` orders prereleases wrong
# (v1.0.0-rc1 after v1.0.0), so each name becomes a sort key "<base> <flag>
# <pre>" (flag 0 = prerelease, 1 = plain release) sorted with
# `-k1,1V -k2,2n -k3,3V`: by base, then a bare release after its own
# prereleases, then prereleases amongst themselves.
set -euo pipefail

# shellcheck source=scripts/release/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# latest_version_from_changes DIR
# Echoes the highest vX.Y.Z[-PRE] among DIR/.changes/v*.md, or fails if none.
latest_version_from_changes() {
  local dir="$1" f name version_part base pre flag keys=""
  shopt -s nullglob
  for f in "$dir"/.changes/v*.md; do
    name="$(basename "$f" .md)"
    version_part="${name#v}"
    case "$version_part" in
      *-*) base="${version_part%%-*}"; pre="${version_part#*-}"; flag=0 ;;
      *) base="$version_part"; pre=""; flag=1 ;;
    esac
    keys="${keys:+$keys$'\n'}$base $flag $pre"
  done
  shopt -u nullglob
  [ -n "$keys" ] || return 1
  local top base_out flag_out pre_out
  top="$(printf '%s\n' "$keys" | sort -k1,1V -k2,2n -k3,3V | tail -n1)"
  read -r base_out flag_out pre_out <<<"$top"
  if [ "$flag_out" = "1" ]; then
    echo "v${base_out}"
  else
    echo "v${base_out}-${pre_out}"
  fi
}

check_version_consistency() {
  local dir="${1:-.}" latest latest_no_v
  if ! latest=$(latest_version_from_changes "$dir"); then
    echo "FAIL: no .changes/v*.md release-notes files found under $dir/.changes - can't determine the latest version." >&2
    return 1
  fi
  latest_no_v="${latest#v}"

  local failed=0 spec kind path found
  for spec in "json:$PKG_JSON" "json:$TAURI_CONF" "cargo_toml:$CARGO_TOML" "cargo_lock:$CARGO_LOCK"; do
    kind="${spec%%:*}"
    path="${spec#*:}"
    found=$(version_of "$kind" cat "$dir/$path")
    if [ -z "$found" ]; then
      echo "FAIL: couldn't read a version from $path." >&2
      failed=1
    elif [ "$found" != "$latest_no_v" ]; then
      echo "FAIL: $path version ($found) != latest released version ($latest_no_v, from .changes/$latest.md). Versions are bumped only by the Release PR workflow (changie replacements: + cargo lock refresh)." >&2
      failed=1
    fi
  done

  [ "$failed" -eq 0 ] || return 1
  echo "ok: all version files match latest release ($latest)"
}

main() {
  if [ "${1:-}" = "--latest" ]; then
    latest_version_from_changes "${2:-.}"
    return
  fi
  check_version_consistency "${1:-.}"
}

if [ "${BUDGEBELL_RELEASE_CHECK_TEST:-0}" != "1" ]; then
  main "$@"
fi
