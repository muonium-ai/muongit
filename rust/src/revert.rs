//! Revert — undo the changes introduced by a commit
//! Parity: libgit2 src/libgit2/revert.c

use std::fs;
use std::path::Path;

use crate::cherrypick::merge_trees_content;
use crate::commit::parse_commit;
use crate::error::MuonGitError;
use crate::odb::read_loose_object;
use crate::oid::OID;
use crate::refs::resolve_reference;
use crate::tree::{parse_tree, TreeEntry};
use crate::types::ObjectType;

/// Options for revert
#[derive(Debug, Clone)]
pub struct RevertOptions {
    /// For merge commits, which parent to use (1-based, default 1)
    pub mainline: usize,
}

impl Default for RevertOptions {
    fn default() -> Self {
        RevertOptions { mainline: 1 }
    }
}

/// Result of a revert operation
#[derive(Debug, Clone)]
pub struct RevertResult {
    /// Whether there are conflicts
    pub has_conflicts: bool,
    /// Merged file contents: (path, content, conflicted)
    pub files: Vec<(String, String, bool)>,
    /// The commit that was reverted
    pub reverted_commit: OID,
}

/// Revert a commit against HEAD.
///
/// This performs a three-way merge with swapped arguments compared to cherry-pick:
/// `merge(commit_tree, head_tree, parent_tree)` — this inverts the changes.
///
/// State files written:
/// - `.git/REVERT_HEAD` — OID of commit being reverted
/// - `.git/MERGE_MSG` — suggested revert message
pub fn revert(
    git_dir: &Path,
    commit_oid: &OID,
    opts: &RevertOptions,
) -> Result<RevertResult, MuonGitError> {
    // Read the commit to revert
    let (obj_type, data) = read_loose_object(git_dir, commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject("not a commit".into()));
    }
    let commit = parse_commit(commit_oid.clone(), &data)?;

    // Get parent
    if commit.parent_ids.is_empty() {
        return Err(MuonGitError::InvalidObject(
            "cannot revert a root commit".into(),
        ));
    }
    let parent_idx = opts.mainline.saturating_sub(1);
    let parent_oid = commit
        .parent_ids
        .get(parent_idx)
        .ok_or_else(|| MuonGitError::InvalidObject("mainline parent not found".into()))?;

    let parent_tree = load_tree_entries_for(git_dir, parent_oid)?;
    let commit_tree = load_tree_entries_for_commit(git_dir, &commit)?;

    // Read HEAD tree
    let head_oid = resolve_reference(git_dir, "HEAD")?;
    let head_tree = load_tree_entries_for(git_dir, &head_oid)?;

    // Key difference from cherry-pick: swapped base and theirs
    // Cherry-pick: merge(parent, ours=HEAD, theirs=commit)  — applies changes
    // Revert:      merge(commit, ours=HEAD, theirs=parent)  — inverts changes
    let result = merge_trees_content(git_dir, &commit_tree, &head_tree, &parent_tree)?;

    // Write REVERT_HEAD
    let revert_head_path = git_dir.join("REVERT_HEAD");
    fs::write(&revert_head_path, format!("{}\n", commit_oid.hex()))?;

    // Write MERGE_MSG
    let msg = format!(
        "Revert \"{}\"\n\nThis reverts commit {}.\n",
        commit.message.lines().next().unwrap_or(""),
        commit_oid.hex()
    );
    fs::write(git_dir.join("MERGE_MSG"), &msg)?;

    Ok(RevertResult {
        has_conflicts: result.has_conflicts,
        files: result.files,
        reverted_commit: commit_oid.clone(),
    })
}

/// Clean up revert state files
pub fn revert_cleanup(git_dir: &Path) {
    let _ = fs::remove_file(git_dir.join("REVERT_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
}

/// Load tree entries from a commit OID
fn load_tree_entries_for(
    git_dir: &Path,
    commit_oid: &OID,
) -> Result<Vec<TreeEntry>, MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject("expected commit".into()));
    }
    let commit = parse_commit(commit_oid.clone(), &data)?;
    let (tree_type, tree_data) = read_loose_object(git_dir, &commit.tree_id)?;
    if tree_type != ObjectType::Tree {
        return Err(MuonGitError::InvalidObject("expected tree".into()));
    }
    Ok(parse_tree(commit.tree_id.clone(), &tree_data)?.entries)
}

fn load_tree_entries_for_commit(
    git_dir: &Path,
    commit: &crate::commit::Commit,
) -> Result<Vec<TreeEntry>, MuonGitError> {
    let (tree_type, tree_data) = read_loose_object(git_dir, &commit.tree_id)?;
    if tree_type != ObjectType::Tree {
        return Err(MuonGitError::InvalidObject("expected tree".into()));
    }
    Ok(parse_tree(commit.tree_id.clone(), &tree_data)?.entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::serialize_commit;
    use crate::odb::write_loose_object;
    use crate::refs::write_reference;
    use crate::tree::serialize_tree;
    use crate::types::Signature;

    fn test_sig() -> Signature {
        Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 1000000000,
            offset: 0,
        }
    }

    fn setup_repo(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name);
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        let git_dir = base.join(".git");
        fs::create_dir_all(git_dir.join("objects")).unwrap();
        fs::create_dir_all(git_dir.join("refs/heads")).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        (base, git_dir)
    }

    #[test]
    fn test_revert_basic() {
        let (_base, git_dir) = setup_repo("revert_basic");
        let sig = test_sig();

        // Base: file.txt = "hello\n"
        let base_blob =
            write_loose_object(&git_dir, ObjectType::Blob, b"hello\n").unwrap();
        let base_tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: base_blob.clone(),
        }]);
        let base_tree = write_loose_object(&git_dir, ObjectType::Tree, &base_tree_data).unwrap();
        let c0_data = serialize_commit(&base_tree, &[], &sig, &sig, "base", None);
        let c0 = write_loose_object(&git_dir, ObjectType::Commit, &c0_data).unwrap();

        // Commit to revert: changes "hello\n" to "world\n"
        let mod_blob =
            write_loose_object(&git_dir, ObjectType::Blob, b"world\n").unwrap();
        let mod_tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: mod_blob.clone(),
        }]);
        let mod_tree = write_loose_object(&git_dir, ObjectType::Tree, &mod_tree_data).unwrap();
        let c1_data = serialize_commit(&mod_tree, &[c0.clone()], &sig, &sig, "change", None);
        let c1 = write_loose_object(&git_dir, ObjectType::Commit, &c1_data).unwrap();

        // HEAD = c1 (the commit we want to revert is also HEAD)
        write_reference(&git_dir, "refs/heads/main", &c1).unwrap();

        // Revert c1 — should restore "hello\n"
        let result = revert(&git_dir, &c1, &RevertOptions::default()).unwrap();
        assert!(!result.has_conflicts);
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].1.trim(), "hello");

        // REVERT_HEAD should exist
        assert!(git_dir.join("REVERT_HEAD").exists());
        let msg = fs::read_to_string(git_dir.join("MERGE_MSG")).unwrap();
        assert!(msg.contains("Revert"));
        revert_cleanup(&git_dir);
        assert!(!git_dir.join("REVERT_HEAD").exists());
    }

    #[test]
    fn test_revert_conflict() {
        let (_base, git_dir) = setup_repo("revert_conflict");
        let sig = test_sig();

        // Base: "hello\n"
        let blob0 = write_loose_object(&git_dir, ObjectType::Blob, b"hello\n").unwrap();
        let tree0_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob0.clone(),
        }]);
        let tree0 = write_loose_object(&git_dir, ObjectType::Tree, &tree0_data).unwrap();
        let c0 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree0, &[], &sig, &sig, "base", None),
        )
        .unwrap();

        // c1: "world\n"
        let blob1 = write_loose_object(&git_dir, ObjectType::Blob, b"world\n").unwrap();
        let tree1_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob1.clone(),
        }]);
        let tree1 = write_loose_object(&git_dir, ObjectType::Tree, &tree1_data).unwrap();
        let c1 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree1, &[c0.clone()], &sig, &sig, "c1", None),
        )
        .unwrap();

        // c2 (HEAD): "universe\n" — based on c1, so reverting c1 will conflict
        let blob2 = write_loose_object(&git_dir, ObjectType::Blob, b"universe\n").unwrap();
        let tree2_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob2.clone(),
        }]);
        let tree2 = write_loose_object(&git_dir, ObjectType::Tree, &tree2_data).unwrap();
        let c2 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree2, &[c1.clone()], &sig, &sig, "c2", None),
        )
        .unwrap();
        write_reference(&git_dir, "refs/heads/main", &c2).unwrap();

        // Revert c1: merge(c1_tree, head_tree=c2, parent_tree=c0)
        // base="world", ours="universe", theirs="hello" → conflict
        let result = revert(&git_dir, &c1, &RevertOptions::default()).unwrap();
        assert!(result.has_conflicts);
        revert_cleanup(&git_dir);
    }

    #[test]
    fn test_revert_added_file() {
        let (_base, git_dir) = setup_repo("revert_added_file");
        let sig = test_sig();

        // Base: empty tree
        let tree0 = write_loose_object(&git_dir, ObjectType::Tree, &[]).unwrap();
        let c0 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree0, &[], &sig, &sig, "empty", None),
        )
        .unwrap();

        // c1: adds file.txt
        let blob = write_loose_object(&git_dir, ObjectType::Blob, b"content\n").unwrap();
        let tree1_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob.clone(),
        }]);
        let tree1 = write_loose_object(&git_dir, ObjectType::Tree, &tree1_data).unwrap();
        let c1 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree1, &[c0.clone()], &sig, &sig, "add file", None),
        )
        .unwrap();
        write_reference(&git_dir, "refs/heads/main", &c1).unwrap();

        // Revert c1 — should remove file.txt
        let result = revert(&git_dir, &c1, &RevertOptions::default()).unwrap();
        assert!(!result.has_conflicts);
        // The file should no longer be in the result (parent had empty tree)
        assert!(result.files.is_empty());
        revert_cleanup(&git_dir);
    }
}
