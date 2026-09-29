#!/usr/bin/env bash
# Shared helpers for the release check scripts. Sourced, never executed.
# See docs/releasing.md.

# Paths of the version locations, relative to the repo root.
export PKG_JSON="package.json"
export TAURI_CONF="src-tauri/tauri.conf.json"
export CARGO_TOML="src-tauri/Cargo.toml"
export CARGO_LOCK="src-tauri/Cargo.lock"
# The crate name whose entry in Cargo.lock must track Cargo.toml's version.
export CRATE_NAME="budgebell"

# pr_head_is_release_branch REF
# release/v* branches (opened by release-pr.yml) are exempt from the PR
# checks. A convenience keyed on the branch name alone, not a security
# boundary: anyone who can push a branch can already edit the workflows.
pr_head_is_release_branch() {
  case "$1" in
    release/v*) return 0 ;;
    *) return 1 ;;
  esac
}

# changie_config_exists_at REV
# Before .changie.yaml exists on the base, a PR is bootstrapping the system.
changie_config_exists_at() {
  git cat-file -e "${1}:.changie.yaml" 2>/dev/null
}

# The extractors read a file's content on stdin and print the version, or
# nothing if it can't be found.

# The "version" field of a JSON document.
json_version() {
  jq -r '.version // empty' 2>/dev/null || true
}

# The `version = "..."` line inside Cargo.toml's [package] table.
cargo_toml_version() {
  awk '
    /^\[package\][[:space:]]*$/ { in_pkg = 1; next }
    /^\[/ { in_pkg = 0 }
    in_pkg && /^version[[:space:]]*=/ { gsub(/"/, "", $3); print $3; exit }
  '
}

# The version of the crate's own [[package]] entry in Cargo.lock.
cargo_lock_version() {
  awk -v name="$CRATE_NAME" '
    /^name = / { hit = ($3 == "\"" name "\""); next }
    hit && /^version = / { gsub(/"/, "", $3); print $3; exit }
  '
}

# version_of KIND SOURCE...  — runs the extractor for KIND (json, cargo_toml,
# cargo_lock) over the file content produced by SOURCE (a command).
version_of() {
  local kind="$1"
  shift
  "$@" 2>/dev/null | "${kind}_version" || true
}
