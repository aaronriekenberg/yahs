//! Utility functions for glob pattern compilation and matching.

use globset::{Glob, GlobSet, GlobSetBuilder};

/// Build a `GlobSet` from a slice of pattern strings.
/// Each pattern is matched case-sensitively.  Patterns without a `/` are
/// automatically treated as path-component substring matches by wrapping them
/// in `**/<pattern>/**` and `**/<pattern>` forms so that e.g. `".git"` blocks
/// any path segment named `.git`.
pub fn build_glob_set(patterns: &[String]) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        // If the pattern has no path separator and no wildcard, treat it as a
        // path-component match: block any path that *contains* the segment.
        if !pattern.contains('/') && !pattern.contains('*') && !pattern.contains('?') {
            builder.add(Glob::new(&format!("**/{pattern}"))?);
            builder.add(Glob::new(&format!("**/{pattern}/**"))?);
            builder.add(Glob::new(pattern)?);
        } else {
            builder.add(Glob::new(pattern)?);
        }
    }
    Ok(builder.build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_pattern_creates_three_globs() {
        let patterns = vec![".git".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        // Should match ".git" at root
        assert!(glob_set.is_match(".git"));
        // Should match ".git" as path component
        assert!(glob_set.is_match("path/to/.git"));
        // Should match paths under ".git"
        assert!(glob_set.is_match("path/.git/objects"));
    }

    #[test]
    fn test_pattern_with_slash_uses_as_is() {
        let patterns = vec!["config/secret".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        // Should match the exact pattern
        assert!(glob_set.is_match("config/secret"));
    }

    #[test]
    fn test_pattern_with_wildcard_uses_as_is() {
        let patterns = vec!["*.tmp".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        // Should match files matching the wildcard
        assert!(glob_set.is_match("file.tmp"));
    }

    #[test]
    fn test_multiple_patterns() {
        let patterns = vec![".git".to_string(), ".env".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(glob_set.is_match(".git"));
        assert!(glob_set.is_match(".env"));
        assert!(!glob_set.is_match("regular_file.txt"));
    }

    #[test]
    fn test_empty_patterns() {
        let patterns: Vec<String> = vec![];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(!glob_set.is_match("anything"));
    }

    #[test]
    fn test_nested_path_matching() {
        let patterns = vec![".git".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(glob_set.is_match("subdir/.git"));
        assert!(glob_set.is_match("deep/nested/path/.git"));
        assert!(glob_set.is_match("deep/nested/path/.git/objects"));
    }
}
