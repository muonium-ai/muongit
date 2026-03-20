//! Pathspec pattern matching
//! Parity: libgit2 src/libgit2/pathspec.c

/// Flags for pathspec matching behavior
#[derive(Debug, Clone, Copy, Default)]
pub struct PathspecFlags {
    pub ignore_case: bool,
    pub no_glob: bool,
    pub no_match_error: bool,
    pub find_failures: bool,
}

/// A single pathspec pattern
#[derive(Debug, Clone)]
struct PathspecPattern {
    pattern: String,
    negated: bool,
    match_all: bool,
}

/// Compiled pathspec for matching file paths
#[derive(Debug, Clone)]
pub struct Pathspec {
    patterns: Vec<PathspecPattern>,
}

/// Result of matching a pathspec against a list of paths
#[derive(Debug, Clone)]
pub struct PathspecMatchResult {
    pub matches: Vec<String>,
    pub failures: Vec<String>,
}

impl Pathspec {
    /// Create a new pathspec from a list of patterns
    pub fn new(patterns: &[&str]) -> Self {
        let mut compiled = Vec::new();
        for &pat_str in patterns {
            compiled.push(parse_pattern(pat_str));
        }
        Pathspec { patterns: compiled }
    }

    /// Check if a path matches this pathspec
    pub fn matches_path(&self, path: &str, flags: &PathspecFlags) -> bool {
        if self.patterns.is_empty() {
            return true;
        }

        let mut matched = false;

        for pattern in &self.patterns {
            if pattern.match_all {
                matched = !pattern.negated;
                continue;
            }

            let does_match = if flags.no_glob {
                path_matches_literal(path, &pattern.pattern, flags.ignore_case)
            } else {
                path_matches_glob(path, &pattern.pattern, flags.ignore_case)
            };

            if does_match {
                matched = !pattern.negated;
            }
        }

        matched
    }

    /// Match this pathspec against a list of paths
    pub fn match_paths(&self, paths: &[&str], flags: &PathspecFlags) -> PathspecMatchResult {
        let mut matches = Vec::new();
        let mut matched_patterns = vec![false; self.patterns.len()];

        for &path in paths {
            if self.matches_path(path, flags) {
                matches.push(path.to_string());
            }
            // Track which patterns matched for failure detection
            for (i, pattern) in self.patterns.iter().enumerate() {
                if !pattern.negated && pattern_matches_path(path, pattern, flags) {
                    matched_patterns[i] = true;
                }
            }
        }

        let failures = if flags.find_failures {
            self.patterns
                .iter()
                .enumerate()
                .filter(|(i, p)| !p.negated && !matched_patterns[*i])
                .map(|(_, p)| p.pattern.clone())
                .collect()
        } else {
            Vec::new()
        };

        PathspecMatchResult { matches, failures }
    }

    /// Number of patterns
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// Whether the pathspec is empty
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }
}

/// Parse a single pattern string
fn parse_pattern(pat: &str) -> PathspecPattern {
    let mut pattern = pat.to_string();
    let mut negated = false;

    // Handle negation
    if pattern.starts_with('!') {
        negated = true;
        pattern = pattern[1..].to_string();
    } else if pattern.starts_with('\\') && pattern.len() > 1 && pattern.as_bytes()[1] == b'!' {
        // Escaped negation
        pattern = pattern[1..].to_string();
    }

    // Strip leading slash (anchors to root)
    if pattern.starts_with('/') {
        pattern = pattern[1..].to_string();
    }

    let match_all = pattern == "*" || pattern.is_empty();
    PathspecPattern {
        pattern,
        negated,
        match_all,
    }
}

fn pattern_matches_path(path: &str, pattern: &PathspecPattern, flags: &PathspecFlags) -> bool {
    if pattern.match_all {
        return true;
    }
    if flags.no_glob {
        path_matches_literal(path, &pattern.pattern, flags.ignore_case)
    } else {
        path_matches_glob(path, &pattern.pattern, flags.ignore_case)
    }
}

/// Literal path matching (prefix + exact)
fn path_matches_literal(path: &str, pattern: &str, ignore_case: bool) -> bool {
    let (p, t) = if ignore_case {
        (pattern.to_lowercase(), path.to_lowercase())
    } else {
        (pattern.to_string(), path.to_string())
    };

    // Exact match
    if t == p {
        return true;
    }

    // Directory prefix match: pattern "dir" matches "dir/file"
    if t.starts_with(&p) && t.as_bytes().get(p.len()) == Some(&b'/') {
        return true;
    }

    false
}

/// Glob-based path matching
fn path_matches_glob(path: &str, pattern: &str, ignore_case: bool) -> bool {
    let (p, t) = if ignore_case {
        (pattern.to_lowercase(), path.to_lowercase())
    } else {
        (pattern.to_string(), path.to_string())
    };

    // Handle ** (any number of path levels)
    if let Some(sub) = p.strip_prefix("**/") {
        // Match at any directory level
        if wildmatch(sub, &t) {
            return true;
        }
        // Also try at every directory level
        let mut pos = 0;
        while let Some(idx) = t[pos..].find('/') {
            if wildmatch(sub, &t[pos + idx + 1..]) {
                return true;
            }
            pos += idx + 1;
        }
        return false;
    }

    // If pattern has no '/', match against basename only (git behavior)
    if !p.contains('/') {
        let basename = t.rsplit('/').next().unwrap_or(&t);
        if wildmatch(&p, basename) {
            return true;
        }
    }

    // Standard glob match against full path
    if wildmatch(&p, &t) {
        return true;
    }

    // Trailing slash stripped: pattern "dir" matches "dir/file"
    let stripped = p.trim_end_matches('/');
    if t.starts_with(stripped) && t.as_bytes().get(stripped.len()) == Some(&b'/') {
        return true;
    }

    false
}

/// Wildcard matching (supports *, ?, and [...])
fn wildmatch(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    wildmatch_inner(&p, &t)
}

fn wildmatch_inner(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(&'*'), _) => {
            // * matches everything except /
            // Try matching zero characters
            if wildmatch_inner(&pattern[1..], text) {
                return true;
            }
            // Try matching one or more characters (but not /)
            if let Some(&ch) = text.first() {
                if ch != '/' {
                    return wildmatch_inner(pattern, &text[1..]);
                }
            }
            false
        }
        (Some(&'?'), Some(&ch)) => {
            if ch != '/' {
                wildmatch_inner(&pattern[1..], &text[1..])
            } else {
                false
            }
        }
        (Some(&'['), _) => {
            // Character class
            if let Some((matched, rest_pattern)) = match_char_class(&pattern[1..], text.first()) {
                if matched {
                    return wildmatch_inner(rest_pattern, &text[1..]);
                }
            }
            false
        }
        (Some(&a), Some(&b)) if a == b => wildmatch_inner(&pattern[1..], &text[1..]),
        _ => false,
    }
}

/// Match a character class [abc] or [!abc] or [a-z]
fn match_char_class<'a>(pattern: &'a [char], ch: Option<&char>) -> Option<(bool, &'a [char])> {
    let ch = ch?;
    let mut negated = false;
    let mut i = 0;

    if i < pattern.len() && pattern[i] == '!' {
        negated = true;
        i += 1;
    }

    let mut matched = false;
    while i < pattern.len() && pattern[i] != ']' {
        if i + 2 < pattern.len() && pattern[i + 1] == '-' {
            // Range
            if *ch >= pattern[i] && *ch <= pattern[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if *ch == pattern[i] {
                matched = true;
            }
            i += 1;
        }
    }

    if i < pattern.len() && pattern[i] == ']' {
        let result = if negated { !matched } else { matched };
        Some((result, &pattern[i + 1..]))
    } else {
        None // Malformed character class
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_matching() {
        let ps = Pathspec::new(&["*.rs"]);
        let flags = PathspecFlags::default();
        assert!(ps.matches_path("src/main.rs", &flags));
        assert!(!ps.matches_path("src/main.py", &flags));
    }

    #[test]
    fn test_directory_prefix() {
        let ps = Pathspec::new(&["src"]);
        let flags = PathspecFlags::default();
        assert!(ps.matches_path("src", &flags));
        assert!(ps.matches_path("src/main.rs", &flags));
        assert!(!ps.matches_path("test/main.rs", &flags));
    }

    #[test]
    fn test_negation() {
        let ps = Pathspec::new(&["*.rs", "!test_*.rs"]);
        let flags = PathspecFlags::default();
        assert!(ps.matches_path("main.rs", &flags));
        assert!(!ps.matches_path("test_main.rs", &flags));
    }

    #[test]
    fn test_double_star() {
        let ps = Pathspec::new(&["**/test.rs"]);
        let flags = PathspecFlags::default();
        assert!(ps.matches_path("test.rs", &flags));
        assert!(ps.matches_path("src/test.rs", &flags));
        assert!(ps.matches_path("a/b/c/test.rs", &flags));
        assert!(!ps.matches_path("test.py", &flags));
    }

    #[test]
    fn test_question_mark() {
        let ps = Pathspec::new(&["?.rs"]);
        let flags = PathspecFlags::default();
        assert!(ps.matches_path("a.rs", &flags));
        assert!(!ps.matches_path("ab.rs", &flags));
    }

    #[test]
    fn test_char_class() {
        let ps = Pathspec::new(&["[abc].rs"]);
        let flags = PathspecFlags::default();
        assert!(ps.matches_path("a.rs", &flags));
        assert!(ps.matches_path("b.rs", &flags));
        assert!(!ps.matches_path("d.rs", &flags));
    }

    #[test]
    fn test_ignore_case() {
        let ps = Pathspec::new(&["*.RS"]);
        let mut flags = PathspecFlags::default();
        flags.ignore_case = true;
        assert!(ps.matches_path("main.rs", &flags));
    }

    #[test]
    fn test_no_glob() {
        let ps = Pathspec::new(&["*.rs"]);
        let mut flags = PathspecFlags::default();
        flags.no_glob = true;
        // With no_glob, *.rs is literal
        assert!(!ps.matches_path("main.rs", &flags));
        assert!(ps.matches_path("*.rs", &flags));
    }

    #[test]
    fn test_match_paths() {
        let ps = Pathspec::new(&["*.rs", "*.toml"]);
        let paths = vec!["src/main.rs", "Cargo.toml", "README.md", "src/lib.rs"];
        let flags = PathspecFlags::default();

        let result = ps.match_paths(&paths, &flags);
        assert_eq!(result.matches.len(), 3);
        assert!(result.matches.contains(&"src/main.rs".to_string()));
        assert!(result.matches.contains(&"Cargo.toml".to_string()));
        assert!(result.matches.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn test_match_paths_with_failures() {
        let ps = Pathspec::new(&["*.rs", "*.xyz"]);
        let paths = vec!["src/main.rs"];
        let mut flags = PathspecFlags::default();
        flags.find_failures = true;

        let result = ps.match_paths(&paths, &flags);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0], "*.xyz");
    }

    #[test]
    fn test_empty_pathspec_matches_all() {
        let ps = Pathspec::new(&[]);
        let flags = PathspecFlags::default();
        assert!(ps.matches_path("anything.txt", &flags));
    }
}
