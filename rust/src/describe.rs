//! Git describe — find the most recent tag reachable from a commit
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

/// Strategy for finding tags in describe
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescribeStrategy {
    /// Only annotated tags (default)
    Default,
    /// All tags (annotated + lightweight)
    Tags,
    /// All refs
    All,
}

/// Options for describe
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
        DescribeOptions {
            strategy: DescribeStrategy::Default,
            max_candidates: 10,
            pattern: None,
            only_follow_first_parent: false,
            show_commit_oid_as_fallback: false,
        }
    }
}

/// Options for formatting a describe result
#[derive(Debug, Clone)]
pub struct DescribeFormatOptions {
    pub abbreviated_size: usize,
    pub always_use_long_format: bool,
    pub dirty_suffix: Option<String>,
}

impl Default for DescribeFormatOptions {
    fn default() -> Self {
        DescribeFormatOptions {
            abbreviated_size: 7,
            always_use_long_format: false,
            dirty_suffix: None,
        }
    }
}

/// Result of a describe operation
#[derive(Debug, Clone)]
pub struct DescribeResult {
    pub tag_name: Option<String>,
    pub depth: usize,
    pub commit_id: OID,
    pub exact_match: bool,
    pub fallback_to_id: bool,
}

impl DescribeResult {
    /// Format the describe result as a string
    pub fn format(&self, opts: &DescribeFormatOptions) -> String {
        let mut result = if self.fallback_to_id {
            self.commit_id.hex()[..opts.abbreviated_size].to_string()
        } else if let Some(ref tag_name) = self.tag_name {
            if self.exact_match && !opts.always_use_long_format {
                tag_name.clone()
            } else {
                let abbrev = &self.commit_id.hex()[..opts.abbreviated_size];
                format!("{}-{}-g{}", tag_name, self.depth, abbrev)
            }
        } else {
            self.commit_id.hex()[..opts.abbreviated_size].to_string()
        };

        if let Some(ref suffix) = opts.dirty_suffix {
            result.push_str(suffix);
        }

        result
    }
}

/// A tag/ref candidate for describe
#[derive(Debug, Clone)]
struct TagCandidate {
    name: String,
    priority: u8, // 2=annotated tag, 1=lightweight tag, 0=other ref
}

/// Describe a commit — find the most recent tag reachable from it
///
/// Walks commit history via BFS to find the nearest tag ancestor and returns
/// a description like `v1.0-3-gabcdef`.
pub fn describe(
    git_dir: &Path,
    commit_oid: &OID,
    opts: &DescribeOptions,
) -> Result<DescribeResult, MuonGitError> {
    // Step 1: collect all reference targets
    let candidates = collect_candidates(git_dir, opts)?;

    // Check if commit itself is tagged
    if let Some(candidate) = candidates.get(commit_oid) {
        return Ok(DescribeResult {
            tag_name: Some(candidate.name.clone()),
            depth: 0,
            commit_id: commit_oid.clone(),
            exact_match: true,
            fallback_to_id: false,
        });
    }

    // Step 2: BFS from commit through parents
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((commit_oid.clone(), 0usize));
    visited.insert(commit_oid.clone());

    let mut best: Option<(TagCandidate, usize)> = None;

    while let Some((oid, depth)) = queue.pop_front() {
        // Check if this commit is a candidate
        if let Some(candidate) = candidates.get(&oid) {
            let dominated = if let Some((ref current_best, current_depth)) = best {
                // Prefer: closer tag, higher priority, earlier discovery
                depth < current_depth
                    || (depth == current_depth && candidate.priority > current_best.priority)
            } else {
                true
            };
            if dominated {
                best = Some((candidate.clone(), depth));
            }
            // Don't continue past found tags unless looking for better candidates
            if best.is_some() && depth > best.as_ref().unwrap().1 + opts.max_candidates {
                break;
            }
            continue;
        }

        // Read commit and enqueue parents
        if let Ok((obj_type, data)) = read_loose_object(git_dir, &oid) {
            if obj_type == ObjectType::Commit {
                if let Ok(commit) = parse_commit(oid.clone(), &data) {
                    let parents = if opts.only_follow_first_parent {
                        commit.parent_ids.into_iter().take(1).collect::<Vec<_>>()
                    } else {
                        commit.parent_ids
                    };
                    for parent_oid in parents {
                        if visited.insert(parent_oid.clone()) {
                            queue.push_back((parent_oid, depth + 1));
                        }
                    }
                }
            }
        }
    }

    match best {
        Some((candidate, depth)) => Ok(DescribeResult {
            tag_name: Some(candidate.name),
            depth,
            commit_id: commit_oid.clone(),
            exact_match: false,
            fallback_to_id: false,
        }),
        None => {
            if opts.show_commit_oid_as_fallback {
                Ok(DescribeResult {
                    tag_name: None,
                    depth: 0,
                    commit_id: commit_oid.clone(),
                    exact_match: false,
                    fallback_to_id: true,
                })
            } else {
                Err(MuonGitError::NotFound(
                    "no tag found for describe".to_string(),
                ))
            }
        }
    }
}

/// Collect all tag/ref candidates from refs
fn collect_candidates(
    git_dir: &Path,
    opts: &DescribeOptions,
) -> Result<HashMap<OID, TagCandidate>, MuonGitError> {
    let refs = list_references(git_dir)?;
    let mut candidates = HashMap::new();

    for (refname, value) in refs {
        let (name, priority) = match categorize_ref(&refname, opts) {
            Some(v) => v,
            None => continue,
        };

        // Apply pattern filter
        if let Some(ref pattern) = opts.pattern {
            if !glob_match(pattern, &name) {
                continue;
            }
        }

        // Resolve the ref to a commit OID
        let oid = match OID::from_hex(&value) {
            Ok(oid) => oid,
            Err(_) => continue,
        };

        // If it's a tag object, peel to commit
        let (commit_oid, actual_priority) = peel_to_commit(git_dir, &oid, priority);

        candidates.insert(
            commit_oid.clone(),
            TagCandidate {
                name,
                priority: actual_priority,
            },
        );
    }

    Ok(candidates)
}

/// Categorize a ref and return (short_name, priority) if it matches the strategy
fn categorize_ref(refname: &str, opts: &DescribeOptions) -> Option<(String, u8)> {
    match opts.strategy {
        DescribeStrategy::Default => {
            // Only annotated tags
            refname
                .strip_prefix("refs/tags/")
                .map(|tag_name| (tag_name.to_string(), 2))
        }
        DescribeStrategy::Tags => refname
            .strip_prefix("refs/tags/")
            .map(|tag_name| (tag_name.to_string(), 1)),
        DescribeStrategy::All => {
            if let Some(tag_name) = refname.strip_prefix("refs/tags/") {
                Some((tag_name.to_string(), 2))
            } else if let Some(branch) = refname.strip_prefix("refs/heads/") {
                Some((format!("heads/{}", branch), 0))
            } else if let Some(remote) = refname.strip_prefix("refs/remotes/") {
                Some((format!("remotes/{}", remote), 0))
            } else {
                Some((refname.to_string(), 0))
            }
        }
    }
}

/// Peel a tag object to its target commit OID
fn peel_to_commit(git_dir: &Path, oid: &OID, default_priority: u8) -> (OID, u8) {
    if let Ok((obj_type, data)) = read_loose_object(git_dir, oid) {
        match obj_type {
            ObjectType::Tag => {
                if let Ok(tag) = parse_tag(oid.clone(), &data) {
                    // Annotated tag: priority 2
                    return (tag.target_id, 2);
                }
            }
            ObjectType::Commit => {
                return (oid.clone(), default_priority);
            }
            _ => {}
        }
    }
    (oid.clone(), default_priority)
}

/// Simple glob matching (supports * and ?)
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    glob_match_inner(&pat_chars, &text_chars)
}

fn glob_match_inner(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(&'*'), _) => {
            // Try matching 0 or more characters
            glob_match_inner(&pattern[1..], text)
                || (!text.is_empty() && glob_match_inner(pattern, &text[1..]))
        }
        (Some(&'?'), Some(_)) => glob_match_inner(&pattern[1..], &text[1..]),
        (Some(&a), Some(&b)) if a == b => glob_match_inner(&pattern[1..], &text[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::odb::write_loose_object;
    use crate::commit::serialize_commit;
    use crate::tag::serialize_tag;
    use crate::types::Signature;
    use crate::refs::write_reference;
    use std::fs;

    fn test_sig() -> Signature {
        Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 1000000000,
            offset: 0,
        }
    }

    fn setup_repo(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name);
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        let git_dir = base.join(".git");
        fs::create_dir_all(git_dir.join("objects")).unwrap();
        fs::create_dir_all(git_dir.join("refs/heads")).unwrap();
        fs::create_dir_all(git_dir.join("refs/tags")).unwrap();
        (base, git_dir)
    }

    fn make_commit(
        git_dir: &Path,
        tree_oid: &OID,
        parents: &[OID],
        msg: &str,
    ) -> OID {
        let sig = test_sig();
        let data = serialize_commit(tree_oid, parents, &sig, &sig, msg, None);
        write_loose_object(git_dir, ObjectType::Commit, &data).unwrap()
    }

    fn make_empty_tree(git_dir: &Path) -> OID {
        write_loose_object(git_dir, ObjectType::Tree, &[]).unwrap()
    }

    fn make_annotated_tag(git_dir: &Path, target: &OID, tag_name: &str) -> OID {
        let sig = test_sig();
        let data = serialize_tag(target, ObjectType::Commit, tag_name, Some(&sig), &format!("Tag {}", tag_name));
        write_loose_object(git_dir, ObjectType::Tag, &data).unwrap()
    }

    #[test]
    fn test_describe_exact_match() {
        let (_base, git_dir) = setup_repo("describe_exact");
        let tree = make_empty_tree(&git_dir);
        let c1 = make_commit(&git_dir, &tree, &[], "initial");

        // Create annotated tag pointing to c1
        let tag_oid = make_annotated_tag(&git_dir, &c1, "v1.0");
        write_reference(&git_dir, "refs/tags/v1.0", &tag_oid).unwrap();

        let result = describe(&git_dir, &c1, &DescribeOptions::default()).unwrap();
        assert!(result.exact_match);
        assert_eq!(result.tag_name.as_deref(), Some("v1.0"));
        assert_eq!(result.depth, 0);
    }

    #[test]
    fn test_describe_with_depth() {
        let (_base, git_dir) = setup_repo("describe_depth");
        let tree = make_empty_tree(&git_dir);
        let c1 = make_commit(&git_dir, &tree, &[], "first");
        let c2 = make_commit(&git_dir, &tree, &[c1.clone()], "second");
        let c3 = make_commit(&git_dir, &tree, &[c2.clone()], "third");

        // Tag c1
        let tag_oid = make_annotated_tag(&git_dir, &c1, "v1.0");
        write_reference(&git_dir, "refs/tags/v1.0", &tag_oid).unwrap();

        let result = describe(&git_dir, &c3, &DescribeOptions::default()).unwrap();
        assert_eq!(result.tag_name.as_deref(), Some("v1.0"));
        assert_eq!(result.depth, 2);
        assert!(!result.exact_match);
    }

    #[test]
    fn test_describe_fallback_to_oid() {
        let (_base, git_dir) = setup_repo("describe_fallback");
        let tree = make_empty_tree(&git_dir);
        let c1 = make_commit(&git_dir, &tree, &[], "no tags");

        let mut opts = DescribeOptions::default();
        opts.show_commit_oid_as_fallback = true;

        let result = describe(&git_dir, &c1, &opts).unwrap();
        assert!(result.fallback_to_id);
        assert!(result.tag_name.is_none());
    }

    #[test]
    fn test_describe_format() {
        let result = DescribeResult {
            tag_name: Some("v1.0".to_string()),
            depth: 3,
            commit_id: OID::from_hex("abcdef1234567890abcdef1234567890abcdef12").unwrap(),
            exact_match: false,
            fallback_to_id: false,
        };

        let formatted = result.format(&DescribeFormatOptions::default());
        assert_eq!(formatted, "v1.0-3-gabcdef1");
    }

    #[test]
    fn test_describe_format_exact() {
        let result = DescribeResult {
            tag_name: Some("v2.0".to_string()),
            depth: 0,
            commit_id: OID::from_hex("abcdef1234567890abcdef1234567890abcdef12").unwrap(),
            exact_match: true,
            fallback_to_id: false,
        };

        let formatted = result.format(&DescribeFormatOptions::default());
        assert_eq!(formatted, "v2.0");
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("v*", "v1.0"));
        assert!(glob_match("v?.0", "v1.0"));
        assert!(!glob_match("v?.0", "v10.0"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("release-*", "release-1.0"));
        assert!(!glob_match("release-*", "v1.0"));
    }
}
