#!/usr/bin/env bash
# Fails unless a release's asset names include every expected bundle kind.
# Guards release.yml's publish job: tauri-action can go green without
# uploading anything (the first real run did), so an empty or partial draft
# must never be published. See docs/releasing.md.
#
# Usage: check-release-assets.sh [ASSET_NAME...]
#   With no arguments, asset names are read from stdin, one per line, e.g.
#   gh api repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID --jq '.assets[].name'
#
# Sourced by test-checks.sh; the guard at the bottom keeps main() from running
# when sourced.
set -euo pipefail

# shellcheck source=scripts/release/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# The one place to edit when a bundle kind is added or dropped. Shell globs
# matched against whole asset names, one per bundle release.yml must upload
# (Linux: AppImage, deb, rpm; macOS: arm64 and x86_64 dmg; Windows: msi and
# nsis), plus the in-app updater's signed artifacts and its latest.json
# manifest: the AppImage, each macOS .app.tar.gz and the msi are what
# latest.json's primary platform entries point at.
REQUIRED_ASSET_PATTERNS=(
  '*.AppImage'
  '*.deb'
  '*.rpm'
  '*_aarch64.dmg'
  '*_x64.dmg'
  '*.msi'
  '*-setup.exe'
  '*.AppImage.sig'
  '*_aarch64.app.tar.gz'
  '*_aarch64.app.tar.gz.sig'
  '*_x64.app.tar.gz'
  '*_x64.app.tar.gz.sig'
  '*.msi.sig'
  'latest.json'
)

# check_release_assets [NAME...] (names on stdin when none are given)
check_release_assets() {
  local -a names=()
  if [ "$#" -gt 0 ]; then
    names=("$@")
  else
    local line
    while IFS= read -r line || [ -n "$line" ]; do
      [ -z "$line" ] || names+=("$line")
    done
  fi

  local pattern name found
  local -a missing=()
  for pattern in "${REQUIRED_ASSET_PATTERNS[@]}"; do
    found=0
    for name in ${names[@]+"${names[@]}"}; do
      # $pattern is deliberately unquoted: it is a glob.
      # shellcheck disable=SC2254
      case "$name" in $pattern) found=1; break ;; esac
    done
    [ "$found" -eq 1 ] || missing+=("$pattern")
  done

  if [ "${#missing[@]}" -gt 0 ]; then
    echo "::error::Release is missing expected assets matching: ${missing[*]} (found ${#names[@]} asset(s))." >&2
    return 1
  fi
  echo "ok: all ${#REQUIRED_ASSET_PATTERNS[@]} expected asset kinds present (${#names[@]} asset(s) total)"
}

main() {
  check_release_assets "$@"
}

if [ "${BUDGEBELL_RELEASE_CHECK_TEST:-0}" != "1" ]; then
  main "$@"
fi
