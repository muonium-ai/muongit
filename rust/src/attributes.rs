//! Gitattributes support
//! Parity: libgit2 src/libgit2/attr_file.c

use std::fs;
use std::path::Path;

/// A single attribute value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrValue {
    /// Attribute is set (e.g., `text`).
    Set,
    /// Attribute is unset (e.g., `-text`).
    Unset,
    /// Attribute has a custom value (e.g., `eol=lf`).
    Value(String),
}

/// A single attribute rule: pattern + attributes.
#[derive(Debug, Clone)]
struct AttrRule {
    pattern: String,
    attrs: Vec<(String, AttrValue)>,
}

/// Compiled gitattributes rules for a repository.
#[derive(Debug, Clone)]
pub struct Attributes {
    rules: Vec<AttrRule>,
}

impl Attributes {
    /// Create an empty attributes set.
    pub fn new() -> Self {
        Attributes { rules: Vec::new() }
    }

    /// Load attributes from a `.gitattributes` file.
    pub fn load(path: &Path) -> Self {
        let mut attrs = Self::new();
        if let Ok(content) = fs::read_to_string(path) {
            attrs.parse(&content);
        }
        attrs
    }

    /// Load attributes for a repository, checking:
    /// 1. `<worktree>/.gitattributes`
    /// 2. `<git_dir>/info/attributes`
    pub fn load_for_repo(git_dir: &Path, workdir: Option<&Path>) -> Self {
        let mut attrs = Self::new();

        // Worktree .gitattributes (higher priority, loaded first = lower priority in last-match-wins)
        if let Some(wd) = workdir {
            let worktree_attrs = wd.join(".gitattributes");
            if let Ok(content) = fs::read_to_string(&worktree_attrs) {
                attrs.parse(&content);
            }
        }

        // info/attributes (repo-local overrides)
        let info_attrs = git_dir.join("info").join("attributes");
        if let Ok(content) = fs::read_to_string(&info_attrs) {
            attrs.parse(&content);
        }

        attrs
    }

    /// Parse gitattributes content and add rules.
    pub fn parse(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(rule) = parse_attr_line(line) {
                self.rules.push(rule);
            }
        }
    }

    /// Get the value of a specific attribute for a path.
    /// Last matching rule wins.
    pub fn get(&self, path: &str, attr_name: &str) -> Option<&AttrValue> {
        let mut result = None;
        for rule in &self.rules {
            if attr_path_match(path, &rule.pattern) {
                for (name, value) in &rule.attrs {
                    if name == attr_name {
                        result = Some(value);
                    }
                }
            }
        }
        result
    }

    /// Get all attributes for a path.
    /// Last matching rule wins for each attribute name.
    pub fn get_all(&self, path: &str) -> Vec<(String, AttrValue)> {
        let mut map = std::collections::HashMap::new();
        for rule in &self.rules {
            if attr_path_match(path, &rule.pattern) {
                for (name, value) in &rule.attrs {
                    map.insert(name.clone(), value.clone());
                }
            }
        }
        let mut result: Vec<_> = map.into_iter().collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Check if a path is marked as binary.
    /// A file is binary if it has `binary` attribute set, or `-diff`, or `-text`.
    pub fn is_binary(&self, path: &str) -> bool {
        if let Some(AttrValue::Set) = self.get(path, "binary") {
            return true;
        }
        if let Some(AttrValue::Unset) = self.get(path, "diff") {
            return true;
        }
        if let Some(AttrValue::Unset) = self.get(path, "text") {
            return true;
        }
        false
    }

    /// Get the eol setting for a path.
    pub fn eol(&self, path: &str) -> Option<&str> {
        match self.get(path, "eol") {
            Some(AttrValue::Value(v)) => Some(v.as_str()),
            _ => None,
        }
    }
}

impl Default for Attributes {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a single gitattributes line into a rule.
fn parse_attr_line(line: &str) -> Option<AttrRule> {
    // Split into pattern and attributes
    // The pattern is the first whitespace-delimited token
    let mut parts = line.splitn(2, [' ', '\t']);
    let pattern = parts.next()?.to_string();
    let attr_str = parts.next().unwrap_or("");

    if pattern.is_empty() {
        return None;
    }

    let attrs = parse_attrs(attr_str);
    if attrs.is_empty() {
        return None;
    }

    Some(AttrRule { pattern, attrs })
}

/// Parse the attribute portion of a line.
fn parse_attrs(s: &str) -> Vec<(String, AttrValue)> {
    let mut attrs = Vec::new();

    for token in s.split_whitespace() {
        if token.is_empty() {
            continue;
        }

        // Handle macro-like attributes
        if token == "binary" {
            attrs.push(("binary".to_string(), AttrValue::Set));
            attrs.push(("diff".to_string(), AttrValue::Unset));
            attrs.push(("merge".to_string(), AttrValue::Unset));
            attrs.push(("text".to_string(), AttrValue::Unset));
            continue;
        }

        if let Some(name) = token.strip_prefix('-') {
            // Unset: -attr
            attrs.push((name.to_string(), AttrValue::Unset));
        } else if let Some(eq_pos) = token.find('=') {
            // Value: attr=value
            let name = &token[..eq_pos];
            let value = &token[eq_pos + 1..];
            attrs.push((name.to_string(), AttrValue::Value(value.to_string())));
        } else {
            // Set: attr
            attrs.push((token.to_string(), AttrValue::Set));
        }
    }

    attrs
}

/// Match a path against a gitattributes pattern.
fn attr_path_match(path: &str, pattern: &str) -> bool {
    // If pattern contains '/', match the full path
    // Otherwise, match only the basename
    if pattern.contains('/') {
        glob_match(pattern, path)
    } else {
        let basename = path.rsplit('/').next().unwrap_or(path);
        glob_match(pattern, basename)
    }
}

/// Simple glob matching (supports *, ?, [...]).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_inner(&pat, &txt)
}

fn glob_match_inner(pat: &[char], txt: &[char]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < txt.len() {
        if pi < pat.len() && pat[pi] == '?' {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pat.len() && pat[pi] == '[' {
            // Character class
            if let Some((matched, new_pi)) = match_char_class(&pat[pi..], txt[ti]) {
                if matched {
                    pi += new_pi;
                    ti += 1;
                } else if star_pi != usize::MAX {
                    pi = star_pi + 1;
                    star_ti += 1;
                    ti = star_ti;
                } else {
                    return false;
                }
            } else if star_pi != usize::MAX {
                pi = star_pi + 1;
                star_ti += 1;
                ti = star_ti;
            } else {
                return false;
            }
        } else if pi < pat.len() && pat[pi] == txt[ti] {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }

    pi == pat.len()
}

/// Match a character class pattern like [abc] or [a-z].
/// Returns (matched, chars_consumed_in_pattern).
fn match_char_class(pat: &[char], ch: char) -> Option<(bool, usize)> {
    if pat.is_empty() || pat[0] != '[' {
        return None;
    }

    let mut i = 1;
    let negated = i < pat.len() && pat[i] == '!';
    if negated {
        i += 1;
    }

    let mut matched = false;
    while i < pat.len() && pat[i] != ']' {
        if i + 2 < pat.len() && pat[i + 1] == '-' {
            let lo = pat[i];
            let hi = pat[i + 2];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if pat[i] == ch {
                matched = true;
            }
            i += 1;
        }
    }

    if i < pat.len() && pat[i] == ']' {
        if negated {
            matched = !matched;
        }
        Some((matched, i + 1))
    } else {
        None // Unterminated bracket
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_attrs() {
        let mut attrs = Attributes::new();
        attrs.parse("*.txt text\n*.bin binary\n");

        assert_eq!(attrs.get("hello.txt", "text"), Some(&AttrValue::Set));
        assert!(attrs.is_binary("image.bin"));
        assert!(!attrs.is_binary("hello.txt"));
    }

    #[test]
    fn test_parse_unset_and_value() {
        let mut attrs = Attributes::new();
        attrs.parse("*.md text eol=lf\n*.png -text -diff\n");

        assert_eq!(attrs.get("README.md", "text"), Some(&AttrValue::Set));
        assert_eq!(attrs.get("README.md", "eol"), Some(&AttrValue::Value("lf".to_string())));
        assert_eq!(attrs.eol("README.md"), Some("lf"));
        assert_eq!(attrs.get("image.png", "text"), Some(&AttrValue::Unset));
        assert!(attrs.is_binary("image.png"));
    }

    #[test]
    fn test_binary_macro() {
        let mut attrs = Attributes::new();
        attrs.parse("*.jpg binary\n");

        assert!(attrs.is_binary("photo.jpg"));
        assert_eq!(attrs.get("photo.jpg", "diff"), Some(&AttrValue::Unset));
        assert_eq!(attrs.get("photo.jpg", "merge"), Some(&AttrValue::Unset));
        assert_eq!(attrs.get("photo.jpg", "text"), Some(&AttrValue::Unset));
    }

    #[test]
    fn test_last_match_wins() {
        let mut attrs = Attributes::new();
        attrs.parse("* text\n*.bin -text\n");

        assert_eq!(attrs.get("file.txt", "text"), Some(&AttrValue::Set));
        assert_eq!(attrs.get("file.bin", "text"), Some(&AttrValue::Unset));
    }

    #[test]
    fn test_path_with_directory() {
        let mut attrs = Attributes::new();
        attrs.parse("src/*.rs text eol=lf\n");

        assert_eq!(attrs.get("src/main.rs", "text"), Some(&AttrValue::Set));
        assert_eq!(attrs.get("main.rs", "text"), None);
    }

    #[test]
    fn test_get_all() {
        let mut attrs = Attributes::new();
        attrs.parse("*.rs text eol=lf diff\n");

        let all = attrs.get_all("main.rs");
        assert_eq!(all.len(), 3);
        assert!(all.contains(&("text".to_string(), AttrValue::Set)));
        assert!(all.contains(&("eol".to_string(), AttrValue::Value("lf".to_string()))));
        assert!(all.contains(&("diff".to_string(), AttrValue::Set)));
    }

    #[test]
    fn test_comment_and_empty_lines() {
        let mut attrs = Attributes::new();
        attrs.parse("# comment\n\n*.txt text\n  # another comment\n");

        assert_eq!(attrs.get("file.txt", "text"), Some(&AttrValue::Set));
        assert_eq!(attrs.rules.len(), 1);
    }

    #[test]
    fn test_glob_patterns() {
        let mut attrs = Attributes::new();
        attrs.parse("*.txt text\n*.[ch] diff\nMakefile export-ignore\n");

        assert_eq!(attrs.get("file.txt", "text"), Some(&AttrValue::Set));
        assert_eq!(attrs.get("main.c", "diff"), Some(&AttrValue::Set));
        assert_eq!(attrs.get("util.h", "diff"), Some(&AttrValue::Set));
        assert_eq!(attrs.get("main.rs", "diff"), None);
        assert_eq!(attrs.get("Makefile", "export-ignore"), Some(&AttrValue::Set));
    }

    #[test]
    fn test_load_file() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_attrs_load");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let attrs_path = tmp.join(".gitattributes");
        std::fs::write(&attrs_path, "*.txt text\n*.bin binary\n").unwrap();

        let attrs = Attributes::load(&attrs_path);
        assert_eq!(attrs.get("file.txt", "text"), Some(&AttrValue::Set));
        assert!(attrs.is_binary("data.bin"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_for_repo() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_attrs_repo");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let workdir = tmp.clone();
        let git_dir = repo.git_dir();

        // Write .gitattributes in worktree
        std::fs::write(workdir.join(".gitattributes"), "*.txt text\n").unwrap();

        // Write info/attributes
        std::fs::create_dir_all(git_dir.join("info")).unwrap();
        std::fs::write(git_dir.join("info/attributes"), "*.bin binary\n").unwrap();

        let attrs = Attributes::load_for_repo(git_dir, Some(&workdir));
        assert_eq!(attrs.get("file.txt", "text"), Some(&AttrValue::Set));
        assert!(attrs.is_binary("data.bin"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
