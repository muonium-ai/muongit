//! Merge base computation
//! Parity: libgit2 src/libgit2/merge.c (git_merge_base)

use std::collections::{HashSet, VecDeque};
use std::path::Path;

use crate::error::MuonGitError;
use crate::object::read_object;
use crate::oid::OID;

/// Read and parse a commit from the object database.
fn read_commit(git_dir: &Path, oid: &OID) -> Result<crate::commit::Commit, MuonGitError> {
    read_object(git_dir, oid)?.as_commit()
}

/// Collect all ancestors of a commit (including itself) via BFS.
fn ancestors(git_dir: &Path, oid: &OID) -> Result<HashSet<OID>, MuonGitError> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(oid.clone());
    visited.insert(oid.clone());

    while let Some(current) = queue.pop_front() {
        let commit = read_commit(git_dir, &current)?;
        for parent_id in &commit.parent_ids {
            if visited.insert(parent_id.clone()) {
                queue.push_back(parent_id.clone());
            }
        }
    }

    Ok(visited)
}

/// Find the merge base (lowest common ancestor) of two commits.
///
/// Returns the best common ancestor — one that is not an ancestor of any
/// other common ancestor. Returns `None` if the commits share no history.
pub fn merge_base(git_dir: &Path, oid1: &OID, oid2: &OID) -> Result<Option<OID>, MuonGitError> {
    if oid1 == oid2 {
        return Ok(Some(oid1.clone()));
    }

    // Collect all ancestors of oid1
    let ancestors1 = ancestors(git_dir, oid1)?;

    // BFS from oid2, looking for commits that are also ancestors of oid1
    let mut common = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(oid2.clone());
    visited.insert(oid2.clone());

    while let Some(current) = queue.pop_front() {
        if ancestors1.contains(&current) {
            common.push(current.clone());
            // Don't traverse further — ancestors of this are also common
            // but are "worse" (further from the tips)
            continue;
        }
        let commit = read_commit(git_dir, &current)?;
        for parent_id in &commit.parent_ids {
            if visited.insert(parent_id.clone()) {
                queue.push_back(parent_id.clone());
            }
        }
    }

    if common.is_empty() {
        return Ok(None);
    }

    if common.len() == 1 {
        return Ok(Some(common.into_iter().next().unwrap()));
    }

    // Filter: remove any common ancestor that is itself an ancestor of another.
    // The "best" merge base is not reachable from any other common ancestor.
    let mut best = common.clone();
    for ca in &common {
        let ca_ancestors = ancestors(git_dir, ca)?;
        best.retain(|b| b == ca || !ca_ancestors.contains(b));
    }

    Ok(best.into_iter().next())
}

/// Find all merge bases between two commits.
/// In simple cases this returns one OID; for criss-cross merges it may return multiple.
pub fn merge_bases(git_dir: &Path, oid1: &OID, oid2: &OID) -> Result<Vec<OID>, MuonGitError> {
    if oid1 == oid2 {
        return Ok(vec![oid1.clone()]);
    }

    let ancestors1 = ancestors(git_dir, oid1)?;

    let mut common = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(oid2.clone());
    visited.insert(oid2.clone());

    while let Some(current) = queue.pop_front() {
        if ancestors1.contains(&current) {
            common.push(current.clone());
            continue;
        }
        let commit = read_commit(git_dir, &current)?;
        for parent_id in &commit.parent_ids {
            if visited.insert(parent_id.clone()) {
                queue.push_back(parent_id.clone());
            }
        }
    }

    // Filter out ancestors of other common ancestors
    let mut best = common.clone();
    for ca in &common {
        let ca_ancestors = ancestors(git_dir, ca)?;
        best.retain(|b| b == ca || !ca_ancestors.contains(b));
    }

    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectType;
    use crate::odb::write_loose_object;
    use crate::repository::Repository;

    /// Helper: write a commit object with given parents and return its OID.
    fn make_commit(git_dir: &Path, tree_oid: &OID, parents: &[&OID], msg: &str) -> OID {
        let mut data = format!("tree {}\n", tree_oid.hex());
        for p in parents {
            data.push_str(&format!("parent {}\n", p.hex()));
        }
        data.push_str("author Test <test@test.com> 1000000000 +0000\n");
        data.push_str("committer Test <test@test.com> 1000000000 +0000\n");
        data.push_str(&format!("\n{}", msg));
        write_loose_object(git_dir, ObjectType::Commit, data.as_bytes()).unwrap()
    }

    /// Helper: create an empty tree and return its OID.
    fn make_empty_tree(git_dir: &Path) -> OID {
        write_loose_object(git_dir, ObjectType::Tree, b"").unwrap()
    }

    #[test]
    fn test_same_commit() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_mb_same");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let tree = make_empty_tree(git_dir);
        let c1 = make_commit(git_dir, &tree, &[], "initial");

        let result = merge_base(git_dir, &c1, &c1).unwrap();
        assert_eq!(result, Some(c1));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_linear_history() {
        // A -- B -- C
        // merge_base(B, C) = B
        // merge_base(A, C) = A
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_mb_linear");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let tree = make_empty_tree(git_dir);
        let a = make_commit(git_dir, &tree, &[], "A");
        let b = make_commit(git_dir, &tree, &[&a], "B");
        let c = make_commit(git_dir, &tree, &[&b], "C");

        assert_eq!(merge_base(git_dir, &b, &c).unwrap(), Some(b.clone()));
        assert_eq!(merge_base(git_dir, &a, &c).unwrap(), Some(a.clone()));
        assert_eq!(merge_base(git_dir, &a, &b).unwrap(), Some(a));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_fork_and_merge() {
        //     B
        //    / \
        // A     D
        //    \ /
        //     C
        // merge_base(B, C) = A
        // merge_base(B, D) = B (B is ancestor of D)
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_mb_fork");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let tree = make_empty_tree(git_dir);
        let a = make_commit(git_dir, &tree, &[], "A");
        let b = make_commit(git_dir, &tree, &[&a], "B");
        let c = make_commit(git_dir, &tree, &[&a], "C");
        let d = make_commit(git_dir, &tree, &[&b, &c], "D");

        assert_eq!(merge_base(git_dir, &b, &c).unwrap(), Some(a));
        assert_eq!(merge_base(git_dir, &b, &d).unwrap(), Some(b));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_no_common_ancestor() {
        // A -- B    C -- D  (two separate roots)
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_mb_disjoint");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let tree = make_empty_tree(git_dir);
        let a = make_commit(git_dir, &tree, &[], "A");
        let b = make_commit(git_dir, &tree, &[&a], "B");
        let c = make_commit(git_dir, &tree, &[], "C");
        let d = make_commit(git_dir, &tree, &[&c], "D");

        assert_eq!(merge_base(git_dir, &b, &d).unwrap(), None);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_merge_bases_multiple() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_mb_multi");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let tree = make_empty_tree(git_dir);
        let a = make_commit(git_dir, &tree, &[], "A");
        let b = make_commit(git_dir, &tree, &[&a], "B");
        let c = make_commit(git_dir, &tree, &[&a], "C");

        let bases = merge_bases(git_dir, &b, &c).unwrap();
        assert_eq!(bases.len(), 1);
        assert_eq!(bases[0], a);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
