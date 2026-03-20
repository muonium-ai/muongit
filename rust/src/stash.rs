//! Git stash — save and restore working directory state
//! Parity: libgit2 src/libgit2/stash.c

use std::fs;
use std::path::Path;

use crate::commit::{parse_commit, serialize_commit};
use crate::error::MuonGitError;
use crate::odb::{read_loose_object, write_loose_object};
use crate::oid::OID;
use crate::reflog::{append_reflog, drop_reflog_entry, read_reflog};
use crate::refs::{delete_reference, read_reference, resolve_reference, write_reference};
use crate::tree::{parse_tree, serialize_tree, TreeEntry};
use crate::types::{ObjectType, Signature};

/// Stash flags controlling what gets stashed.
/// Parity: git_stash_flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StashFlags {
    /// Default: stash staged + unstaged changes, reset index and workdir
    Default,
    /// Leave staged changes in the index
    KeepIndex,
    /// Include untracked files
    IncludeUntracked,
}

/// A stash entry from the reflog.
#[derive(Debug, Clone)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub oid: OID,
}

/// Result of applying a stash.
#[derive(Debug)]
pub struct StashApplyResult {
    /// Whether there were conflicts during apply
    pub has_conflicts: bool,
    /// Merged file contents: (path, content, conflicted)
    pub files: Vec<(String, String, bool)>,
}

/// Save the current working directory state as a stash entry.
///
/// Creates the multi-parent stash commit structure:
/// - w_commit (refs/stash target): tree = workdir state, parents = [HEAD, i_commit]
/// - i_commit: tree = index state, parent = HEAD
///
/// Parity: git_stash_save
pub fn stash_save(
    git_dir: &Path,
    workdir: Option<&Path>,
    stasher: &Signature,
    message: Option<&str>,
) -> Result<OID, MuonGitError> {
    let workdir = workdir.ok_or(MuonGitError::BareRepo)?;

    // Resolve HEAD
    let head_oid = resolve_reference(git_dir, "HEAD")?;

    // Read HEAD commit to get branch info for messages
    let (_, head_data) = read_loose_object(git_dir, &head_oid)?;
    let head_commit = parse_commit(head_oid.clone(), &head_data)?;
    let short_sha = &head_oid.hex()[..7];

    // Get branch name
    let branch = match read_reference(git_dir, "HEAD") {
        Ok(val) => {
            if let Some(target) = val.strip_prefix("ref: refs/heads/") {
                target.to_string()
            } else {
                "(no branch)".to_string()
            }
        }
        Err(_) => "(no branch)".to_string(),
    };

    // Get first line of HEAD commit message for reflog
    let summary = head_commit
        .message
        .lines()
        .next()
        .unwrap_or("")
        .to_string();

    // Build the index tree from the current workdir state
    // Collect files in workdir (simple flat scan)
    let workdir_entries = collect_workdir_entries(git_dir, workdir)?;

    if workdir_entries.is_empty() {
        return Err(MuonGitError::NotFound(
            "no local changes to save".to_string(),
        ));
    }

    // Create i_commit (index snapshot) — for simplicity, same as workdir tree
    let workdir_tree_data = serialize_tree(&workdir_entries);
    let workdir_tree_oid = write_loose_object(git_dir, ObjectType::Tree, &workdir_tree_data)?;

    let i_msg = format!("index on {}: {} {}\n", branch, short_sha, summary);
    let i_data = serialize_commit(
        &workdir_tree_oid,
        std::slice::from_ref(&head_oid),
        stasher,
        stasher,
        &i_msg,
        None,
    );
    let i_oid = write_loose_object(git_dir, ObjectType::Commit, &i_data)?;

    // Create w_commit (working directory snapshot)
    let stash_msg = match message {
        Some(m) => format!("On {}: {}\n", branch, m),
        None => format!("WIP on {}: {} {}\n", branch, short_sha, summary),
    };
    let w_data = serialize_commit(
        &workdir_tree_oid,
        &[head_oid.clone(), i_oid],
        stasher,
        stasher,
        &stash_msg,
        None,
    );
    let w_oid = write_loose_object(git_dir, ObjectType::Commit, &w_data)?;

    // Update refs/stash
    let old_stash = resolve_reference(git_dir, "refs/stash").unwrap_or_else(|_| OID::zero());
    write_reference(git_dir, "refs/stash", &w_oid)?;

    // Append to reflog
    let reflog_msg = stash_msg.trim().to_string();
    append_reflog(
        git_dir,
        "refs/stash",
        &old_stash,
        &w_oid,
        stasher,
        &reflog_msg,
    )?;

    Ok(w_oid)
}

/// List all stash entries.
///
/// Returns entries in reverse order (most recent first, index 0 = newest).
/// Parity: git_stash_foreach
pub fn stash_list(git_dir: &Path) -> Result<Vec<StashEntry>, MuonGitError> {
    let entries = read_reflog(git_dir, "refs/stash")?;
    let len = entries.len();
    let stashes: Vec<StashEntry> = entries
        .into_iter()
        .enumerate()
        .rev()
        .map(|(i, entry)| StashEntry {
            index: len - 1 - i,
            message: entry.message.clone(),
            oid: entry.new_oid,
        })
        .collect();
    Ok(stashes)
}

/// Apply a stash entry without removing it.
///
/// Performs a three-way merge between HEAD, the stash's base (parent[0]),
/// and the stash's working tree.
/// Parity: git_stash_apply
pub fn stash_apply(
    git_dir: &Path,
    index: usize,
) -> Result<StashApplyResult, MuonGitError> {
    let entries = read_reflog(git_dir, "refs/stash")?;
    let len = entries.len();
    if index >= len {
        return Err(MuonGitError::NotFound(format!(
            "stash@{{{}}} not found",
            index
        )));
    }

    // Reflog is chronological; index 0 = newest = last entry
    let reflog_idx = len - 1 - index;
    let stash_oid = &entries[reflog_idx].new_oid;

    apply_stash_oid(git_dir, stash_oid)
}

/// Pop the stash at position `index`: apply then drop.
/// Parity: git_stash_pop
pub fn stash_pop(
    git_dir: &Path,
    index: usize,
) -> Result<StashApplyResult, MuonGitError> {
    let result = stash_apply(git_dir, index)?;

    // Only drop if apply succeeded without conflicts
    if !result.has_conflicts {
        stash_drop(git_dir, index)?;
    }

    Ok(result)
}

/// Drop a stash entry by index.
///
/// Removes the reflog entry. If it was the last entry, deletes refs/stash.
/// If dropping index 0 (newest), updates refs/stash to point to the next entry.
/// Parity: git_stash_drop
pub fn stash_drop(git_dir: &Path, index: usize) -> Result<(), MuonGitError> {
    let entries = read_reflog(git_dir, "refs/stash")?;
    let len = entries.len();
    if index >= len {
        return Err(MuonGitError::NotFound(format!(
            "stash@{{{}}} not found",
            index
        )));
    }

    let reflog_idx = len - 1 - index;
    let remaining = drop_reflog_entry(git_dir, "refs/stash", reflog_idx)?;

    if remaining.is_empty() {
        // No more stashes — delete the ref
        delete_reference(git_dir, "refs/stash")?;
    } else {
        // Update refs/stash to the newest remaining entry (last in list)
        let newest = &remaining[remaining.len() - 1];
        write_reference(git_dir, "refs/stash", &newest.new_oid)?;
    }

    Ok(())
}

// ── Internal helpers ──

/// Collect workdir files as tree entries (single-level, skipping .git).
fn collect_workdir_entries(
    git_dir: &Path,
    workdir: &Path,
) -> Result<Vec<TreeEntry>, MuonGitError> {
    let mut entries = Vec::new();

    if !workdir.is_dir() {
        return Ok(entries);
    }

    for dir_entry in fs::read_dir(workdir)? {
        let dir_entry = dir_entry?;
        let name = dir_entry.file_name().to_string_lossy().to_string();

        // Skip .git directory
        if name == ".git" {
            continue;
        }

        let file_type = dir_entry.file_type()?;
        if file_type.is_file() {
            let data = fs::read(dir_entry.path())?;
            let blob_oid = write_loose_object(git_dir, ObjectType::Blob, &data)?;
            entries.push(TreeEntry {
                mode: crate::tree::file_mode::BLOB,
                name,
                oid: blob_oid,
            });
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// Apply a stash commit by OID.
fn apply_stash_oid(
    git_dir: &Path,
    stash_oid: &OID,
) -> Result<StashApplyResult, MuonGitError> {
    // Read the stash commit
    let (obj_type, data) = read_loose_object(git_dir, stash_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject("stash is not a commit".into()));
    }
    let w_commit = parse_commit(stash_oid.clone(), &data)?;

    // parent[0] = base (HEAD at stash time)
    if w_commit.parent_ids.is_empty() {
        return Err(MuonGitError::InvalidObject(
            "stash commit has no parents".into(),
        ));
    }
    let base_oid = &w_commit.parent_ids[0];

    // Load trees
    let base_entries = load_commit_tree(git_dir, base_oid)?;
    let stash_entries = load_tree_entries(git_dir, &w_commit.tree_id)?;

    // Load current HEAD tree
    let head_oid = resolve_reference(git_dir, "HEAD")?;
    let head_entries = load_commit_tree(git_dir, &head_oid)?;

    // Three-way merge: base=stash_base, ours=head, theirs=stash_workdir
    use crate::cherrypick::merge_trees_content;
    let merge_result = merge_trees_content(git_dir, &base_entries, &head_entries, &stash_entries)?;

    Ok(StashApplyResult {
        has_conflicts: merge_result.has_conflicts,
        files: merge_result.files,
    })
}

/// Load a commit's tree entries.
fn load_commit_tree(git_dir: &Path, commit_oid: &OID) -> Result<Vec<TreeEntry>, MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject("expected commit".into()));
    }
    let commit = parse_commit(commit_oid.clone(), &data)?;
    load_tree_entries(git_dir, &commit.tree_id)
}

/// Load tree entries for a given tree OID.
fn load_tree_entries(git_dir: &Path, tree_oid: &OID) -> Result<Vec<TreeEntry>, MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, tree_oid)?;
    if obj_type != ObjectType::Tree {
        return Err(MuonGitError::InvalidObject("expected tree".into()));
    }
    let tree = parse_tree(tree_oid.clone(), &data)?;
    Ok(tree.entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Repository;
    use std::path::PathBuf;

    fn test_tmp(name: &str) -> PathBuf {
        let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp")
            .join(name);
        let _ = fs::remove_dir_all(&tmp);
        tmp
    }

    fn make_sig() -> Signature {
        Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 1000000000,
            offset: 0,
        }
    }

    /// Helper: init repo, create initial commit with one file.
    fn setup_repo(name: &str) -> (PathBuf, Repository, OID) {
        let tmp = test_tmp(name);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();
        let workdir = repo.workdir().unwrap().to_path_buf();
        let sig = make_sig();

        // Write a blob
        let blob_data = b"initial content\n";
        let blob_oid = write_loose_object(&git_dir, ObjectType::Blob, blob_data).unwrap();

        // Create tree
        let entry = TreeEntry {
            mode: crate::tree::file_mode::BLOB,
            name: "file.txt".to_string(),
            oid: blob_oid,
        };
        let tree_data = serialize_tree(&[entry]);
        let tree_oid = write_loose_object(&git_dir, ObjectType::Tree, &tree_data).unwrap();

        // Create commit
        let commit_data = serialize_commit(&tree_oid, &[], &sig, &sig, "initial commit\n", None);
        let commit_oid = write_loose_object(&git_dir, ObjectType::Commit, &commit_data).unwrap();

        // Set HEAD
        write_reference(&git_dir, "refs/heads/main", &commit_oid).unwrap();
        crate::refs::write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        // Write file to workdir
        fs::write(workdir.join("file.txt"), blob_data).unwrap();

        (tmp, repo, commit_oid)
    }

    #[test]
    fn test_stash_save_and_list() {
        let (tmp, repo, _head_oid) = setup_repo("stash_save_list");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let sig = make_sig();

        // Modify file
        fs::write(workdir.join("file.txt"), "modified content\n").unwrap();

        // Stash save
        let stash_oid =
            stash_save(git_dir, Some(workdir), &sig, Some("test stash")).unwrap();
        assert!(!stash_oid.is_zero());

        // List stashes
        let stashes = stash_list(git_dir).unwrap();
        assert_eq!(stashes.len(), 1);
        assert_eq!(stashes[0].index, 0);
        assert!(stashes[0].message.contains("test stash"));
        assert_eq!(stashes[0].oid, stash_oid);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_multiple() {
        let (tmp, repo, _) = setup_repo("stash_multiple");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let sig = make_sig();

        // First stash
        fs::write(workdir.join("file.txt"), "change 1\n").unwrap();
        let oid1 = stash_save(git_dir, Some(workdir), &sig, Some("first")).unwrap();

        // Restore file for second stash
        fs::write(workdir.join("file.txt"), "change 2\n").unwrap();
        let oid2 = stash_save(git_dir, Some(workdir), &sig, Some("second")).unwrap();

        // List should show 2 stashes, newest first
        let stashes = stash_list(git_dir).unwrap();
        assert_eq!(stashes.len(), 2);
        assert_eq!(stashes[0].index, 0);
        assert_eq!(stashes[0].oid, oid2);
        assert_eq!(stashes[1].index, 1);
        assert_eq!(stashes[1].oid, oid1);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_apply() {
        let (tmp, repo, _) = setup_repo("stash_apply");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let sig = make_sig();

        // Modify and stash
        fs::write(workdir.join("file.txt"), "stashed content\n").unwrap();
        stash_save(git_dir, Some(workdir), &sig, None).unwrap();

        // Apply stash
        let result = stash_apply(git_dir, 0).unwrap();
        assert!(!result.has_conflicts);

        // Check that stashed content is in the result
        let file = result.files.iter().find(|f| f.0 == "file.txt").unwrap();
        assert_eq!(file.1, "stashed content\n");

        // Stash should still exist
        let stashes = stash_list(git_dir).unwrap();
        assert_eq!(stashes.len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_pop() {
        let (tmp, repo, _) = setup_repo("stash_pop");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let sig = make_sig();

        // Modify and stash
        fs::write(workdir.join("file.txt"), "popped content\n").unwrap();
        stash_save(git_dir, Some(workdir), &sig, None).unwrap();

        // Pop stash
        let result = stash_pop(git_dir, 0).unwrap();
        assert!(!result.has_conflicts);

        // Stash should be removed
        let stashes = stash_list(git_dir).unwrap();
        assert_eq!(stashes.len(), 0);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_drop() {
        let (tmp, repo, _) = setup_repo("stash_drop");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let sig = make_sig();

        // Create two stashes
        fs::write(workdir.join("file.txt"), "change A\n").unwrap();
        stash_save(git_dir, Some(workdir), &sig, Some("A")).unwrap();

        fs::write(workdir.join("file.txt"), "change B\n").unwrap();
        let oid_b = stash_save(git_dir, Some(workdir), &sig, Some("B")).unwrap();

        // Drop stash@{1} (older one)
        stash_drop(git_dir, 1).unwrap();

        let stashes = stash_list(git_dir).unwrap();
        assert_eq!(stashes.len(), 1);
        assert_eq!(stashes[0].oid, oid_b);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_drop_last() {
        let (tmp, repo, _) = setup_repo("stash_drop_last");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let sig = make_sig();

        fs::write(workdir.join("file.txt"), "only stash\n").unwrap();
        stash_save(git_dir, Some(workdir), &sig, None).unwrap();

        stash_drop(git_dir, 0).unwrap();

        // refs/stash should be gone
        assert!(resolve_reference(git_dir, "refs/stash").is_err());

        let stashes = stash_list(git_dir).unwrap();
        assert_eq!(stashes.len(), 0);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_empty_workdir_fails() {
        let tmp = test_tmp("stash_empty");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let sig = make_sig();

        // Set up a HEAD commit with initial file
        let blob = write_loose_object(git_dir, ObjectType::Blob, b"hi\n").unwrap();
        let entry = TreeEntry {
            mode: crate::tree::file_mode::BLOB,
            name: "a.txt".to_string(),
            oid: blob,
        };
        let tree_data = serialize_tree(&[entry]);
        let tree_oid = write_loose_object(git_dir, ObjectType::Tree, &tree_data).unwrap();
        let commit_data = serialize_commit(&tree_oid, &[], &sig, &sig, "init\n", None);
        let commit_oid =
            write_loose_object(git_dir, ObjectType::Commit, &commit_data).unwrap();
        write_reference(git_dir, "refs/heads/main", &commit_oid).unwrap();
        crate::refs::write_symbolic_reference(git_dir, "HEAD", "refs/heads/main").unwrap();

        // Remove all workdir files so there's nothing to stash
        let _ = fs::remove_file(workdir.join("a.txt"));

        let result = stash_save(git_dir, Some(workdir), &sig, None);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_bare_repo_fails() {
        let tmp = test_tmp("stash_bare");
        let repo = Repository::init(tmp.to_str().unwrap(), true).unwrap();
        let sig = make_sig();

        let result = stash_save(repo.git_dir(), None, &sig, None);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_invalid_index() {
        let (tmp, repo, _) = setup_repo("stash_invalid_idx");
        let git_dir = repo.git_dir();

        assert!(stash_apply(git_dir, 0).is_err());
        assert!(stash_drop(git_dir, 0).is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stash_default_message() {
        let (tmp, repo, _) = setup_repo("stash_default_msg");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let sig = make_sig();

        fs::write(workdir.join("file.txt"), "changed\n").unwrap();
        stash_save(git_dir, Some(workdir), &sig, None).unwrap();

        let stashes = stash_list(git_dir).unwrap();
        assert!(stashes[0].message.starts_with("WIP on main:"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
