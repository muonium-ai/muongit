//! Git describe: find the most recent tag reachable from a commit
//! Parity: libgit2 src/libgit2/describe.c

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::commit::parse_commit;
use crate::error::MuonGitError;
use crate::odb::read_loose_object;
use crate::oid::OID;
use crate::refs::list_references;
use crate::tag::parse_tag;
use crate::types::ObjectType;

/// Strategy for finding tags in describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescribeStrategy {
    Default, // annotated tags only
    Tags,    // all tags
    All,     // all refs
}

/// Options for describe.
#[derive(Debug, Clone)]
pub struct DescribeOptions {
    pub strategy: DescribeStrategy,
    pub max_candidates: usize,
    pub pattern: Option<String>,
    pub only_follow_first_parent: bool,
    pub show_commit_oid_as_fallback: bool,
}

impl Default for DescribeOptions {
    fn default() -> Self {
        Self {
            strategy: DescribeStrategy::Default,
            max_candidates: 10,
            pattern: None,
            only_follow_first_parent: false,
            show_commit_oid_as_fallback: false,
        }
    }
}

/// Options for formatting a describe result.
#[derive(Debug, Clone)]
pub struct DescribeFormatOptions {
    pub abbreviated_size: usize,
    pub always_use_long_format: bool,
    pub dirty_suffix: Option<String>,
}

impl Default for DescribeFormatOptions {
    fn default() -> Self {
        Self {
            abbreviated_size: 7,
            always_use_long_format: false,
            dirty_suffix: None,
        }
    }
}

/// Result of a describe operation.
#[derive(Debug, Clone)]
pub struct DescribeResult {
    pub tag_name: Option<String>,
    pub depth: usize,
    pub commit_id: OID,
    pub exact_match: bool,
    pub fallback_to_id: bool,
}

impl DescribeResult {
    /// Format the describe result as a string.
    pub fn format(&self, options: &DescribeFormatOptions) -> String {
        let mut result = if self.fallback_to_id {
            self.commit_id.hex[..options.abbreviated_size].to_string()
        } else if let Some(ref tag_name) = self.tag_name {
            if self.exact_match && !options.always_use_long_format {
                tag_name.clone()
            } else {
                let abbrev = &self.commit_id.hex[..options.abbreviated_size];
                format!("{}-{}-g{}", tag_name, self.depth, abbrev)
            }
        } else {
            self.commit_id.hex[..options.abbreviated_size].to_string()
        };
        if let Some(ref suffix) = options.dirty_suffix {
            result.push_str(suffix);
        }
        result
    }
}

struct TagCandidate {
    name: String,
    priority: u8, // 2=annotated, 1=lightweight, 0=other
}

/// Describe a commit — find the most recent tag reachable from it.
pub fn describe(
    git_dir: &Path,
    commit_oid: &OID,
    options: &DescribeOptions,
) -> Result<DescribeResult, MuonGitError> {
    let candidates = collect_candidates(git_dir, options)?;

    // Check if commit itself is tagged
    if let Some(candidate) = candidates.get(&commit_oid.hex) {
        return Ok(DescribeResult {
            tag_name: Some(candidate.name.clone()),
            depth: 0,
            commit_id: commit_oid.clone(),
            exact_match: true,
            fallback_to_id: false,
        });
    }

    // BFS from commit through parents
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(commit_oid.hex.clone());
    queue.push_back((commit_oid.clone(), 0usize));

    let mut best: Option<(String, u8, usize)> = None; // (name, priority, depth)

    while let Some((oid, depth)) = queue.pop_front() {
        if let Some(candidate) = candidates.get(&oid.hex) {
            let dominated = match &best {
                None => true,
                Some((_, bp, bd)) => depth < *bd || (depth == *bd && candidate.priority > *bp),
            };
            if dominated {
                best = Some((candidate.name.clone(), candidate.priority, depth));
            }
            if let Some((_, _, bd)) = &best {
                if depth > bd + options.max_candidates {
                    break;
                }
            }
            continue;
        }

        if let Ok((obj_type, data)) = read_loose_object(git_dir, &oid) {
            if obj_type == ObjectType::Commit {
                if let Ok(commit) = parse_commit(oid, data.as_slice()) {
                    let parents = if options.only_follow_first_parent {
                        commit.parent_ids.into_iter().take(1).collect::<Vec<_>>()
                    } else {
                        commit.parent_ids
                    };
                    for parent_oid in parents {
                        if visited.insert(parent_oid.hex.clone()) {
                            queue.push_back((parent_oid, depth + 1));
                        }
                    }
                }
            }
        }
    }

    if let Some((name, _, depth)) = best {
        return Ok(DescribeResult {
            tag_name: Some(name),
            depth,
            commit_id: commit_oid.clone(),
            exact_match: false,
            fallback_to_id: false,
        });
    }

    if options.show_commit_oid_as_fallback {
        return Ok(DescribeResult {
            tag_name: None,
            depth: 0,
            commit_id: commit_oid.clone(),
            exact_match: false,
            fallback_to_id: true,
        });
    }

    Err(MuonGitError::NotFound("no tag found for describe".into()))
}

fn collect_candidates(
    git_dir: &Path,
    options: &DescribeOptions,
) -> Result<HashMap<String, TagCandidate>, MuonGitError> {
    let refs = list_references(git_dir)?;
    let mut candidates = HashMap::new();

    for (refname, value) in &refs {
        let (name, priority) = match categorize_ref(refname, options) {
            Some(v) => v,
            None => continue,
        };

        if let Some(ref pattern) = options.pattern {
            if !glob_match(pattern, &name) {
                continue;
            }
        }

        if value.len() != 40 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let oid = OID::from_hex(value)
            .map_err(|_| MuonGitError::InvalidObject("bad ref value".into()))?;

        let (commit_oid, actual_priority) = peel_to_commit(git_dir, &oid, priority);
        candidates.insert(
            commit_oid.hex.clone(),
            TagCandidate {
                name,
                priority: actual_priority,
            },
        );
    }
    Ok(candidates)
}

fn categorize_ref(refname: &str, options: &DescribeOptions) -> Option<(String, u8)> {
    match options.strategy {
        DescribeStrategy::Default => {
            if let Some(rest) = refname.strip_prefix("refs/tags/") {
                Some((rest.to_string(), 2))
            } else {
                None
            }
        }
        DescribeStrategy::Tags => {
            if let Some(rest) = refname.strip_prefix("refs/tags/") {
                Some((rest.to_string(), 1))
            } else {
                None
            }
        }
        DescribeStrategy::All => {
            if let Some(rest) = refname.strip_prefix("refs/tags/") {
                Some((rest.to_string(), 2))
            } else if let Some(rest) = refname.strip_prefix("refs/heads/") {
                Some((format!("heads/{}", rest), 0))
            } else if let Some(rest) = refname.strip_prefix("refs/remotes/") {
                Some((format!("remotes/{}", rest), 0))
            } else {
                Some((refname.to_string(), 0))
            }
        }
    }
}

fn peel_to_commit(git_dir: &Path, oid: &OID, default_priority: u8) -> (OID, u8) {
    if let Ok((obj_type, data)) = read_loose_object(git_dir, oid) {
        if obj_type == ObjectType::Tag {
            if let Ok(tag) = parse_tag(oid.clone(), &data) {
                return (tag.target_id, 2);
            }
        }
    }
    (oid.clone(), default_priority)
}

/// Simple glob matching (supports * and ?).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_inner(&p, 0, &t, 0)
}

fn glob_match_inner(p: &[char], pi: usize, t: &[char], ti: usize) -> bool {
    if pi == p.len() && ti == t.len() {
        return true;
    }
    if pi == p.len() {
        return false;
    }
    if p[pi] == '*' {
        return glob_match_inner(p, pi + 1, t, ti)
            || (ti < t.len() && glob_match_inner(p, pi, t, ti + 1));
    }
    if ti < t.len() && (p[pi] == '?' || p[pi] == t[ti]) {
        return glob_match_inner(p, pi + 1, t, ti + 1);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::serialize_commit;
    use crate::odb::write_loose_object;
    use crate::oid::OID;
    use crate::refs::{write_reference, write_symbolic_reference};
    use crate::repository::Repository;
    use crate::tag::serialize_tag;
    use crate::tree::serialize_tree;
    use crate::types::Signature;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        fs::create_dir_all(&base).unwrap();
        let p = base.join(format!("test_describe_{}", name));
        if p.exists() {
            fs::remove_dir_all(&p).unwrap();
        }
        p
    }

    fn make_sig() -> Signature {
        Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 1700000000,
            offset: 0,
        }
    }

    fn make_commit(
        git_dir: &Path,
        parents: &[OID],
        msg: &str,
    ) -> OID {
        let sig = make_sig();
        let tree_data = serialize_tree(&[]);
        let tree_oid = write_loose_object(git_dir, ObjectType::Tree, &tree_data).unwrap();
        let commit_data = serialize_commit(&tree_oid, parents, &sig, &sig, msg, None);
        write_loose_object(git_dir, ObjectType::Commit, &commit_data).unwrap()
    }

    #[test]
    fn test_describe_exact_tag() {
        let tmp = test_dir("exact_tag");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let gd = repo.git_dir();
        let c = make_commit(gd, &[], "initial");
        write_reference(gd, "refs/heads/main", &c).unwrap();
        write_symbolic_reference(gd, "HEAD", "refs/heads/main").unwrap();

        // Create annotated tag
        let tag_data = serialize_tag(&c, ObjectType::Commit, "v1.0", &make_sig(), "release");
        let tag_oid = write_loose_object(gd, ObjectType::Tag, &tag_data).unwrap();
        write_reference(gd, "refs/tags/v1.0", &tag_oid).unwrap();

        let result = describe(gd, &c, &DescribeOptions::default()).unwrap();
        assert!(result.exact_match);
        assert_eq!(result.tag_name.as_deref(), Some("v1.0"));
        assert_eq!(result.depth, 0);
    }

    #[test]
    fn test_describe_with_depth() {
        let tmp = test_dir("with_depth");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let gd = repo.git_dir();
        let c0 = make_commit(gd, &[], "initial");
        write_reference(gd, "refs/heads/main", &c0).unwrap();
        write_symbolic_reference(gd, "HEAD", "refs/heads/main").unwrap();

        let tag_data = serialize_tag(&c0, ObjectType::Commit, "v1.0", &make_sig(), "release");
        let tag_oid = write_loose_object(gd, ObjectType::Tag, &tag_data).unwrap();
        write_reference(gd, "refs/tags/v1.0", &tag_oid).unwrap();

        let c1 = make_commit(gd, &[c0.clone()], "second");
        let c2 = make_commit(gd, &[c1], "third");

        let result = describe(gd, &c2, &DescribeOptions::default()).unwrap();
        assert!(!result.exact_match);
        assert_eq!(result.tag_name.as_deref(), Some("v1.0"));
        assert_eq!(result.depth, 2);
    }

    #[test]
    fn test_describe_no_tag_fallback() {
        let tmp = test_dir("no_tag_fallback");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let gd = repo.git_dir();
        let c = make_commit(gd, &[], "initial");
        write_reference(gd, "refs/heads/main", &c).unwrap();
        write_symbolic_reference(gd, "HEAD", "refs/heads/main").unwrap();

        let opts = DescribeOptions {
            show_commit_oid_as_fallback: true,
            ..Default::default()
        };
        let result = describe(gd, &c, &opts).unwrap();
        assert!(result.fallback_to_id);
        assert!(result.tag_name.is_none());
    }

    #[test]
    fn test_describe_no_tag_error() {
        let tmp = test_dir("no_tag_error");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let gd = repo.git_dir();
        let c = make_commit(gd, &[], "initial");
        write_reference(gd, "refs/heads/main", &c).unwrap();

        let result = describe(gd, &c, &DescribeOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_describe_format() {
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let result = DescribeResult {
            tag_name: Some("v1.0".into()),
            depth: 3,
            commit_id: oid,
            exact_match: false,
            fallback_to_id: false,
        };
        let fmt = result.format(&DescribeFormatOptions::default());
        assert_eq!(fmt, "v1.0-3-gaaf4c61");
    }

    #[test]
    fn test_describe_format_exact() {
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let result = DescribeResult {
            tag_name: Some("v2.0".into()),
            depth: 0,
            commit_id: oid,
            exact_match: true,
            fallback_to_id: false,
        };
        assert_eq!(result.format(&DescribeFormatOptions::default()), "v2.0");
    }

    #[test]
    fn test_describe_format_dirty() {
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let result = DescribeResult {
            tag_name: Some("v1.0".into()),
            depth: 0,
            commit_id: oid,
            exact_match: true,
            fallback_to_id: false,
        };
        let opts = DescribeFormatOptions {
            dirty_suffix: Some("-dirty".into()),
            ..Default::default()
        };
        assert_eq!(result.format(&opts), "v1.0-dirty");
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("v*", "v1.0"));
        assert!(glob_match("v1.?", "v1.0"));
        assert!(!glob_match("v2.*", "v1.0"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("release-*", "release-1.0"));
    }

    #[test]
    fn test_describe_lightweight_tag_with_tag_strategy() {
        let tmp = test_dir("lightweight_tag");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let gd = repo.git_dir();
        let c = make_commit(gd, &[], "initial");
        write_reference(gd, "refs/heads/main", &c).unwrap();
        write_symbolic_reference(gd, "HEAD", "refs/heads/main").unwrap();

        // Create lightweight tag (points directly to commit)
        write_reference(gd, "refs/tags/v0.1", &c).unwrap();

        let opts = DescribeOptions {
            strategy: DescribeStrategy::Tags,
            ..Default::default()
        };
        let result = describe(gd, &c, &opts).unwrap();
        assert!(result.exact_match);
        assert_eq!(result.tag_name.as_deref(), Some("v0.1"));
    }
}
