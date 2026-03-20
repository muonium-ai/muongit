//! Git pathspec: path matching and globbing
//! Parity: libgit2 src/libgit2/pathspec.c

use std::path::Path;

use crate::error::MuonGitError;

/// Flags controlling pathspec matching behavior.
#[derive(Debug, Clone, Copy, Default)]
pub struct PathspecFlags {
    pub ignore_case: bool,
    pub use_case: bool,
    pub no_glob: bool,
    pub no_match_error: bool,
    pub find_failures: bool,
    pub failures_only: bool,
}

/// A compiled set of path patterns for matching.
#[derive(Debug, Clone)]
pub struct Pathspec {
    patterns: Vec<PathspecPattern>,
    flags: PathspecFlags,
}

#[derive(Debug, Clone)]
struct PathspecPattern {
    pattern: String,
    negate: bool,
}

/// Result of matching a pathspec against a set of paths.
#[derive(Debug, Clone)]
pub struct PathspecMatchList {
    pub matched: Vec<String>,
    pub failures: Vec<String>,
}

impl Pathspec {
    /// Create a new pathspec from a list of patterns.
    pub fn new(patterns: &[&str], flags: PathspecFlags) -> Result<Self, MuonGitError> {
        let parsed: Vec<PathspecPattern> = patterns
            .iter()
            .map(|p| {
                if let Some(rest) = p.strip_prefix('!') {
                    PathspecPattern {
                        pattern: rest.to_string(),
                        negate: true,
                    }
                } else if let Some(rest) = p.strip_prefix('\\') {
                    // Escaped leading ! or #
                    PathspecPattern {
                        pattern: rest.to_string(),
                        negate: false,
                    }
                } else {
                    PathspecPattern {
                        pattern: p.to_string(),
                        negate: false,
                    }
                }
            })
            .collect();
        Ok(Self {
            patterns: parsed,
            flags,
        })
    }

    /// Check if a single path matches this pathspec.
    pub fn matches_path(&self, path: &str) -> bool {
        if self.patterns.is_empty() {
            return true;
        }
        let mut matched = false;
        for pat in &self.patterns {
            let does_match = if self.flags.no_glob {
                path_prefix_match(path, &pat.pattern, self.flags.ignore_case)
            } else {
                pathspec_glob_match(path, &pat.pattern, self.flags.ignore_case)
            };
            if does_match {
                if pat.negate {
                    matched = false;
                } else {
                    matched = true;
                }
            }
        }
        matched
    }

    /// Match this pathspec against a list of paths, returning matched paths.
    pub fn match_list(&self, paths: &[&str]) -> PathspecMatchList {
        let mut matched = Vec::new();
        let mut failures = Vec::new();

        if self.flags.failures_only {
            // Only report patterns that didn't match anything
            let mut pattern_matched = vec![false; self.patterns.len()];
            for &path in paths {
                for (i, pat) in self.patterns.iter().enumerate() {
                    if !pat.negate {
                        let m = if self.flags.no_glob {
                            path_prefix_match(path, &pat.pattern, self.flags.ignore_case)
                        } else {
                            pathspec_glob_match(path, &pat.pattern, self.flags.ignore_case)
                        };
                        if m {
                            pattern_matched[i] = true;
                        }
                    }
                }
            }
            for (i, pat) in self.patterns.iter().enumerate() {
                if !pat.negate && !pattern_matched[i] {
                    failures.push(pat.pattern.clone());
                }
            }
            return PathspecMatchList { matched, failures };
        }

        for &path in paths {
            if self.matches_path(path) {
                matched.push(path.to_string());
            }
        }

        if self.flags.find_failures {
            let mut pattern_matched = vec![false; self.patterns.len()];
            for &path in paths {
                for (i, pat) in self.patterns.iter().enumerate() {
                    if !pat.negate {
                        let m = if self.flags.no_glob {
                            path_prefix_match(path, &pat.pattern, self.flags.ignore_case)
                        } else {
                            pathspec_glob_match(path, &pat.pattern, self.flags.ignore_case)
                        };
                        if m {
                            pattern_matched[i] = true;
                        }
                    }
                }
            }
            for (i, pat) in self.patterns.iter().enumerate() {
                if !pat.negate && !pattern_matched[i] {
                    failures.push(pat.pattern.clone());
                }
            }
        }

        if self.flags.no_match_error && matched.is_empty() && !self.patterns.is_empty() {
            // This would be an error condition in libgit2
        }

        PathspecMatchList { matched, failures }
    }
}

/// Glob-style pathspec matching. Supports *, ?, and path-aware matching.
fn pathspec_glob_match(path: &str, pattern: &str, ignore_case: bool) -> bool {
    let p = if ignore_case {
        pattern.to_lowercase()
    } else {
        pattern.to_string()
    };
    let t = if ignore_case {
        path.to_lowercase()
    } else {
        path.to_string()
    };

    // If pattern has no slash, match only the basename
    if !p.contains('/') {
        let basename = t.rsplit('/').next().unwrap_or(&t);
        return glob_match_chars(&p, basename);
    }

    // Pattern has slash — match full path
    glob_match_chars(&p, &t)
}

/// Prefix-based matching (no-glob mode).
fn path_prefix_match(path: &str, pattern: &str, ignore_case: bool) -> bool {
    let p = if ignore_case {
        pattern.to_lowercase()
    } else {
        pattern.to_string()
    };
    let t = if ignore_case {
        path.to_lowercase()
    } else {
        path.to_string()
    };

    if t == p {
        return true;
    }
    // path starts with pattern/ (directory prefix)
    if t.starts_with(&p) && t.as_bytes().get(p.len()) == Some(&b'/') {
        return true;
    }
    // pattern starts with path/ (file under directory)
    if p.starts_with(&t) && p.as_bytes().get(t.len()) == Some(&b'/') {
        return true;
    }
    false
}

/// Simple glob matching for a single segment or full path.
fn glob_match_chars(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    do_glob(&p, 0, &t, 0)
}

fn do_glob(p: &[char], pi: usize, t: &[char], ti: usize) -> bool {
    if pi == p.len() && ti == t.len() {
        return true;
    }
    if pi == p.len() {
        return false;
    }
    if p[pi] == '*' {
        // ** matches across directories
        if pi + 1 < p.len() && p[pi + 1] == '*' {
            // Skip the ** and optional /
            let next_pi = if pi + 2 < p.len() && p[pi + 2] == '/' {
                pi + 3
            } else {
                pi + 2
            };
            return do_glob(p, next_pi, t, ti)
                || (ti < t.len() && do_glob(p, pi, t, ti + 1));
        }
        // * does not match /
        return do_glob(p, pi + 1, t, ti)
            || (ti < t.len() && t[ti] != '/' && do_glob(p, pi, t, ti + 1));
    }
    if ti < t.len() && (p[pi] == '?' || p[pi] == t[ti]) {
        return do_glob(p, pi + 1, t, ti + 1);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_match() {
        let ps = Pathspec::new(&["*.rs"], PathspecFlags::default()).unwrap();
        assert!(ps.matches_path("src/main.rs"));
        assert!(ps.matches_path("lib.rs"));
        assert!(!ps.matches_path("src/main.py"));
    }

    #[test]
    fn test_directory_match() {
        let ps = Pathspec::new(&["src/*.rs"], PathspecFlags::default()).unwrap();
        assert!(ps.matches_path("src/main.rs"));
        assert!(!ps.matches_path("test/main.rs"));
    }

    #[test]
    fn test_negate() {
        let ps = Pathspec::new(&["*.rs", "!test.rs"], PathspecFlags::default()).unwrap();
        assert!(ps.matches_path("main.rs"));
        assert!(!ps.matches_path("test.rs"));
    }

    #[test]
    fn test_no_glob() {
        let flags = PathspecFlags {
            no_glob: true,
            ..Default::default()
        };
        let ps = Pathspec::new(&["src"], flags).unwrap();
        assert!(ps.matches_path("src/main.rs"));
        assert!(!ps.matches_path("test/main.rs"));
    }

    #[test]
    fn test_ignore_case() {
        let flags = PathspecFlags {
            ignore_case: true,
            ..Default::default()
        };
        let ps = Pathspec::new(&["*.RS"], flags).unwrap();
        assert!(ps.matches_path("main.rs"));
        assert!(ps.matches_path("MAIN.RS"));
    }

    #[test]
    fn test_match_list() {
        let ps = Pathspec::new(&["*.rs"], PathspecFlags::default()).unwrap();
        let paths = vec!["a.rs", "b.py", "c.rs"];
        let result = ps.match_list(&paths);
        assert_eq!(result.matched, vec!["a.rs", "c.rs"]);
    }

    #[test]
    fn test_find_failures() {
        let flags = PathspecFlags {
            find_failures: true,
            ..Default::default()
        };
        let ps = Pathspec::new(&["*.rs", "*.go"], flags).unwrap();
        let paths = vec!["a.rs", "b.py"];
        let result = ps.match_list(&paths);
        assert_eq!(result.matched, vec!["a.rs"]);
        assert_eq!(result.failures, vec!["*.go"]);
    }

    #[test]
    fn test_empty_pathspec() {
        let ps = Pathspec::new(&[], PathspecFlags::default()).unwrap();
        assert!(ps.matches_path("anything"));
    }

    #[test]
    fn test_double_star() {
        let ps = Pathspec::new(&["src/**/*.rs"], PathspecFlags::default()).unwrap();
        assert!(ps.matches_path("src/a/b/c.rs"));
        assert!(ps.matches_path("src/main.rs"));
        assert!(!ps.matches_path("test/main.rs"));
    }

    #[test]
    fn test_prefix_match_no_glob() {
        let flags = PathspecFlags {
            no_glob: true,
            ..Default::default()
        };
        let ps = Pathspec::new(&["src/lib"], flags).unwrap();
        assert!(ps.matches_path("src/lib"));
        assert!(ps.matches_path("src/lib/mod.rs"));
        assert!(!ps.matches_path("src/libfoo"));
    }
}
