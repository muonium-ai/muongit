//! Gitignore pattern matching
//! Parity: libgit2 src/libgit2/ignore.c

use std::fs;
use std::path::Path;

/// A single gitignore pattern.
#[derive(Debug, Clone)]
struct IgnorePattern {
    /// The glob pattern (after stripping negation/trailing slash).
    pattern: String,
    /// If true, this pattern negates a previous ignore.
    negated: bool,
    /// If true, this pattern only matches directories.
    dir_only: bool,
    /// The directory prefix this pattern applies to (empty for root .gitignore).
    base_dir: String,
}

/// Compiled gitignore rules for a repository.
#[derive(Debug, Clone)]
pub struct Ignore {
    patterns: Vec<IgnorePattern>,
}

impl Ignore {
    /// Create an empty ignore set.
    pub fn new() -> Self {
        Ignore {
            patterns: Vec::new(),
        }
    }

    /// Load gitignore rules for a repository.
    /// Reads `.git/info/exclude` and root `.gitignore`.
    pub fn load(git_dir: &Path, workdir: &Path) -> Self {
        let mut ignore = Ignore::new();

        // .git/info/exclude
        let exclude_path = git_dir.join("info").join("exclude");
        if exclude_path.exists() {
            if let Ok(content) = fs::read_to_string(&exclude_path) {
                ignore.add_patterns(&content, "");
            }
        }

        // Root .gitignore
        let gitignore_path = workdir.join(".gitignore");
        if gitignore_path.exists() {
            if let Ok(content) = fs::read_to_string(&gitignore_path) {
                ignore.add_patterns(&content, "");
            }
        }

        ignore
    }

    /// Load gitignore rules including subdirectory `.gitignore` files.
    /// Call this when you need full recursive ignore support.
    pub fn load_for_path(&mut self, workdir: &Path, rel_dir: &str) {
        // Check for .gitignore in this directory
        let dir_path = if rel_dir.is_empty() {
            workdir.to_path_buf()
        } else {
            workdir.join(rel_dir)
        };
        let gitignore_path = dir_path.join(".gitignore");
        if gitignore_path.exists() {
            if let Ok(content) = fs::read_to_string(&gitignore_path) {
                let base = if rel_dir.is_empty() {
                    String::new()
                } else {
                    format!("{}/", rel_dir)
                };
                self.add_patterns(&content, &base);
            }
        }
    }

    /// Parse and add patterns from gitignore content.
    pub fn add_patterns(&mut self, content: &str, base_dir: &str) {
        for line in content.lines() {
            let line = line.trim_end();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut pattern = line.to_string();
            let mut negated = false;
            let mut dir_only = false;

            // Handle negation
            if pattern.starts_with('!') {
                negated = true;
                pattern = pattern[1..].to_string();
            }

            // Handle trailing slash (directory only)
            if pattern.ends_with('/') {
                dir_only = true;
                pattern.pop();
            }

            // Handle leading slash (anchored to base)
            if pattern.starts_with('/') {
                pattern = pattern[1..].to_string();
            }

            if pattern.is_empty() {
                continue;
            }

            self.patterns.push(IgnorePattern {
                pattern,
                negated,
                dir_only,
                base_dir: base_dir.to_string(),
            });
        }
    }

    /// Check if a path is ignored.
    /// `path` is relative to the workdir (e.g., "src/main.rs").
    /// `is_dir` indicates whether the path is a directory.
    pub fn is_ignored(&self, path: &str, is_dir: bool) -> bool {
        let mut ignored = false;

        for pat in &self.patterns {
            if pat.dir_only && !is_dir {
                continue;
            }

            if self.matches(pat, path) {
                ignored = !pat.negated;
            }
        }

        ignored
    }

    /// Match a pattern against a path.
    fn matches(&self, pat: &IgnorePattern, path: &str) -> bool {
        let pattern = &pat.pattern;

        // If pattern contains '/', it's anchored to the base directory
        if pattern.contains('/') {
            let full_pattern = format!("{}{}", pat.base_dir, pattern);
            return glob_match(&full_pattern, path);
        }

        // If there's a base_dir, only match paths under that directory
        if !pat.base_dir.is_empty() {
            if let Some(rel) = path.strip_prefix(&pat.base_dir) {
                return glob_match(pattern, rel) || match_basename(pattern, rel);
            }
            return false;
        }

        // Pattern without '/' matches against any path component (basename)
        match_basename(pattern, path)
    }
}

impl Default for Ignore {
    fn default() -> Self {
        Self::new()
    }
}

/// Match a pattern against just the filename portion of a path.
fn match_basename(pattern: &str, path: &str) -> bool {
    let basename = path.rsplit('/').next().unwrap_or(path);
    glob_match(pattern, basename)
}

/// Simple glob matcher supporting `*`, `?`, `[...]`, and `**`.
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == b'*' {
            if pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
                // '**' matches everything including '/'
                // Try matching rest of pattern against every suffix of text
                let rest = &pattern[pi + 2..];
                let rest = if rest.first() == Some(&b'/') {
                    &rest[1..]
                } else {
                    rest
                };
                if rest.is_empty() {
                    return true;
                }
                for i in ti..=text.len() {
                    if glob_match_inner(rest, &text[i..]) {
                        return true;
                    }
                }
                return false;
            }
            // Single '*' — does not match '/'
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'?' && text[ti] != b'/' {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'[' {
            // Character class
            if let Some((matched, new_pi)) = match_char_class(&pattern[pi..], text[ti]) {
                if matched {
                    pi += new_pi;
                    ti += 1;
                } else if star_pi != usize::MAX {
                    star_ti += 1;
                    ti = star_ti;
                    pi = star_pi + 1;
                } else {
                    return false;
                }
            } else if star_pi != usize::MAX {
                star_ti += 1;
                ti = star_ti;
                pi = star_pi + 1;
            } else {
                return false;
            }
        } else if pi < pattern.len() && pattern[pi] == text[ti] {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX && text[ti] != b'/' {
            star_ti += 1;
            ti = star_ti;
            pi = star_pi + 1;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Match a character class like `[abc]` or `[a-z]` or `[!abc]`.
/// Returns (matched, bytes consumed from pattern) or None if malformed.
fn match_char_class(pattern: &[u8], ch: u8) -> Option<(bool, usize)> {
    if pattern.is_empty() || pattern[0] != b'[' {
        return None;
    }

    let mut i = 1;
    let negate = if i < pattern.len() && (pattern[i] == b'!' || pattern[i] == b'^') {
        i += 1;
        true
    } else {
        false
    };

    let mut matched = false;
    while i < pattern.len() && pattern[i] != b']' {
        if i + 2 < pattern.len() && pattern[i + 1] == b'-' {
            // Range
            if ch >= pattern[i] && ch <= pattern[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if ch == pattern[i] {
                matched = true;
            }
            i += 1;
        }
    }

    if i < pattern.len() && pattern[i] == b']' {
        Some((if negate { !matched } else { matched }, i + 1))
    } else {
        None // Malformed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match_basic() {
        assert!(glob_match("*.txt", "hello.txt"));
        assert!(!glob_match("*.txt", "hello.rs"));
        assert!(glob_match("hello.*", "hello.txt"));
        assert!(glob_match("?ello.txt", "hello.txt"));
        assert!(!glob_match("?ello.txt", "hhello.txt"));
    }

    #[test]
    fn test_glob_match_star_no_slash() {
        assert!(!glob_match("*.txt", "dir/hello.txt"));
        assert!(glob_match("*.txt", "hello.txt"));
    }

    #[test]
    fn test_glob_match_double_star() {
        assert!(glob_match("**/*.txt", "hello.txt"));
        assert!(glob_match("**/*.txt", "dir/hello.txt"));
        assert!(glob_match("**/*.txt", "a/b/c/hello.txt"));
        assert!(glob_match("**/build", "build"));
        assert!(glob_match("**/build", "src/build"));
    }

    #[test]
    fn test_glob_match_char_class() {
        assert!(glob_match("[abc].txt", "a.txt"));
        assert!(glob_match("[abc].txt", "b.txt"));
        assert!(!glob_match("[abc].txt", "d.txt"));
        assert!(glob_match("[a-z].txt", "m.txt"));
        assert!(!glob_match("[a-z].txt", "M.txt"));
        assert!(glob_match("[!abc].txt", "d.txt"));
        assert!(!glob_match("[!abc].txt", "a.txt"));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("Makefile", "Makefile"));
        assert!(!glob_match("Makefile", "makefile"));
    }

    #[test]
    fn test_parse_patterns() {
        let mut ignore = Ignore::new();
        ignore.add_patterns("# comment\n*.o\n!important.o\nbuild/\n/root_only\n", "");

        assert_eq!(ignore.patterns.len(), 4);
        assert!(!ignore.patterns[0].negated);
        assert_eq!(ignore.patterns[0].pattern, "*.o");
        assert!(ignore.patterns[1].negated);
        assert_eq!(ignore.patterns[1].pattern, "important.o");
        assert!(ignore.patterns[2].dir_only);
        assert_eq!(ignore.patterns[2].pattern, "build");
        assert_eq!(ignore.patterns[3].pattern, "root_only");
    }

    #[test]
    fn test_is_ignored_basic() {
        let mut ignore = Ignore::new();
        ignore.add_patterns("*.o\n*.log\nbuild/\n", "");

        assert!(ignore.is_ignored("main.o", false));
        assert!(ignore.is_ignored("debug.log", false));
        assert!(ignore.is_ignored("src/test.o", false));
        assert!(!ignore.is_ignored("main.c", false));
        assert!(ignore.is_ignored("build", true));
        assert!(!ignore.is_ignored("build", false)); // dir_only
    }

    #[test]
    fn test_is_ignored_negation() {
        let mut ignore = Ignore::new();
        ignore.add_patterns("*.log\n!important.log\n", "");

        assert!(ignore.is_ignored("debug.log", false));
        assert!(!ignore.is_ignored("important.log", false));
    }

    #[test]
    fn test_is_ignored_double_star() {
        let mut ignore = Ignore::new();
        ignore.add_patterns("**/build\nlogs/**/*.log\n", "");

        assert!(ignore.is_ignored("build", false));
        assert!(ignore.is_ignored("src/build", false));
        assert!(ignore.is_ignored("logs/2024/error.log", false));
    }

    #[test]
    fn test_is_ignored_with_path() {
        let mut ignore = Ignore::new();
        ignore.add_patterns("doc/*.html\n", "");

        assert!(ignore.is_ignored("doc/index.html", false));
        assert!(!ignore.is_ignored("src/index.html", false));
    }

    #[test]
    fn test_load_from_repo() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_ignore_load");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        // Create .gitignore
        let workdir = repo.workdir().unwrap();
        std::fs::write(workdir.join(".gitignore"), "*.o\nbuild/\n").unwrap();

        let ignore = Ignore::load(repo.git_dir(), workdir);
        assert!(ignore.is_ignored("main.o", false));
        assert!(ignore.is_ignored("build", true));
        assert!(!ignore.is_ignored("main.c", false));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_with_exclude() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_ignore_exclude");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        // Create .git/info/exclude
        let info_dir = repo.git_dir().join("info");
        std::fs::create_dir_all(&info_dir).unwrap();
        std::fs::write(info_dir.join("exclude"), "*.swp\n").unwrap();

        let ignore = Ignore::load(repo.git_dir(), repo.workdir().unwrap());
        assert!(ignore.is_ignored("file.swp", false));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_subdir_gitignore() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_ignore_subdir");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let workdir = repo.workdir().unwrap();

        // Create root .gitignore
        std::fs::write(workdir.join(".gitignore"), "*.o\n").unwrap();
        // Create subdirectory with its own .gitignore
        std::fs::create_dir_all(workdir.join("vendor")).unwrap();
        std::fs::write(workdir.join("vendor/.gitignore"), "*.tmp\n").unwrap();

        let mut ignore = Ignore::load(repo.git_dir(), workdir);
        ignore.load_for_path(workdir, "vendor");

        assert!(ignore.is_ignored("main.o", false));
        assert!(ignore.is_ignored("vendor/cache.tmp", false));
        assert!(!ignore.is_ignored("src/cache.tmp", false)); // .tmp only ignored under vendor

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
