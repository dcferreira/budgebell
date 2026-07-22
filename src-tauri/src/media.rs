//! Pure media-path resolution (design spec §4.1): maps a habit's relative
//! `media_path` (as stored in the DB) onto an absolute path under the app's
//! media directory, rejecting anything that would escape that directory.
//! Deliberately filesystem-free (no `canonicalize`, no existence check) so it
//! stays a plain, unit-testable helper — a missing file simply fails to load
//! in the webview and falls back to the placeholder.

use std::path::{Component, Path, PathBuf};

/// Resolves `media_path` (a relative filename or subpath, e.g. `"lunge.png"`
/// or `"sub/dir/clip.png"`) to an absolute path under `media_dir`.
///
/// Returns `None` when `media_path` is empty, absolute, or escapes
/// `media_dir` via `..` (design spec §3/§4.1) — enforced by walking the
/// path's components and tracking how many `Normal` segments are still
/// "open" above the traversal, rather than by resolving against the real
/// filesystem.
pub fn resolve_media(media_dir: &Path, media_path: &str) -> Option<PathBuf> {
    if media_path.is_empty() {
        return None;
    }

    let mut depth: i32 = 0;
    for component in Path::new(media_path).components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    Some(media_dir.join(media_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media_dir() -> PathBuf {
        PathBuf::from("/app-data/media")
    }

    #[test]
    fn a_plain_filename_resolves_under_the_media_dir() {
        // Given a plain relative filename
        // Then it resolves to an absolute path joined onto the media dir
        assert_eq!(
            resolve_media(&media_dir(), "lunge.png"),
            Some(media_dir().join("lunge.png"))
        );
    }

    #[test]
    fn a_nested_relative_subpath_resolves_under_the_media_dir() {
        // Given a relative subpath that stays within the media dir
        // Then it resolves to an absolute path joined onto the media dir
        assert_eq!(
            resolve_media(&media_dir(), "sub/dir/clip.png"),
            Some(media_dir().join("sub/dir/clip.png"))
        );
    }

    #[test]
    fn a_leading_parent_dir_escape_is_rejected() {
        // Given a path that immediately escapes via `..`
        // Then resolution yields None
        assert_eq!(resolve_media(&media_dir(), "../secrets"), None);
    }

    #[test]
    fn a_later_parent_dir_escape_is_rejected() {
        // Given a path that dips back into a subdirectory before escaping
        // above the media dir
        // Then resolution yields None
        assert_eq!(resolve_media(&media_dir(), "a/../../b"), None);
    }

    #[test]
    fn an_absolute_path_is_rejected() {
        // Given an absolute media_path, enforcing relative-only inputs
        // Then resolution yields None
        assert_eq!(resolve_media(&media_dir(), "/etc/passwd"), None);
    }

    #[test]
    fn an_empty_media_path_is_rejected() {
        // Given an empty media_path
        // Then resolution yields None
        assert_eq!(resolve_media(&media_dir(), ""), None);
    }
}
