//! Cherry-pick — apply changes from a commit onto HEAD
//! Parity: libgit2 src/libgit2/cherrypick.c

use std::fs;
use std::path::Path;

use crate::commit::parse_commit;
use crate::error::MuonGitError;
use crate::merge::merge3;
use crate::odb::read_loose_object;
use crate::oid::OID;
use crate::refs::resolve_reference;
use crate::tree::{parse_tree, TreeEntry};
use crate::types::ObjectType;

/// Options for cherry-pick
#[derive(Debug, Clone)]
pub struct CherryPickOptions {
    /// For merge commits, which parent to diff against (1-based, default 1)
    pub mainline: usize,
}

impl Default for CherryPickOptions {
    fn default() -> Self {
        CherryPickOptions { mainline: 1 }
    }
}

/// Result of a cherry-pick operation
#[derive(Debug, Clone)]
pub struct CherryPickResult {
    /// Whether there are conflicts
    pub has_conflicts: bool,
    /// Merged file contents: (path, content, conflicted)
    pub files: Vec<(String, String, bool)>,
    /// The commit that was cherry-picked
    pub cherry_picked_commit: OID,
}

/// Cherry-pick a commit onto HEAD.
///
/// This performs a three-way merge: merge(parent_tree, head_tree, commit_tree)
/// where parent is the commit's parent. The diff parent→commit is applied on top of HEAD.
///
/// State files written:
/// - `.git/CHERRY_PICK_HEAD` — OID of commit being cherry-picked
/// - `.git/MERGE_MSG` — suggested commit message
pub fn cherry_pick(
    git_dir: &Path,
    commit_oid: &OID,
    opts: &CherryPickOptions,
) -> Result<CherryPickResult, MuonGitError> {
    // Read the commit to cherry-pick
    let (obj_type, data) = read_loose_object(git_dir, commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject("not a commit".into()));
    }
    let commit = parse_commit(commit_oid.clone(), &data)?;

    // Get parent tree (base for the merge)
    if commit.parent_ids.is_empty() {
        return Err(MuonGitError::InvalidObject(
            "cannot cherry-pick a root commit".into(),
        ));
    }
    let parent_idx = opts.mainline.saturating_sub(1);
    let parent_oid = commit
        .parent_ids
        .get(parent_idx)
        .ok_or_else(|| MuonGitError::InvalidObject("mainline parent not found".into()))?;

    let parent_tree = load_commit_tree(git_dir, parent_oid)?;
    let commit_tree = load_commit_tree_from_commit(git_dir, &commit)?;

    // Read HEAD commit tree
    let head_oid = resolve_reference(git_dir, "HEAD")?;
    let head_tree = load_commit_tree(git_dir, &head_oid)?;

    // Three-way merge of blob contents
    let result = merge_trees_content(git_dir, &parent_tree, &head_tree, &commit_tree)?;

    // Write CHERRY_PICK_HEAD
    let cp_head_path = git_dir.join("CHERRY_PICK_HEAD");
    fs::write(&cp_head_path, format!("{}\n", commit_oid.hex()))?;

    // Write MERGE_MSG
    let merge_msg_path = git_dir.join("MERGE_MSG");
    fs::write(&merge_msg_path, &commit.message)?;

    Ok(CherryPickResult {
        has_conflicts: result.has_conflicts,
        files: result.files,
        cherry_picked_commit: commit_oid.clone(),
    })
}

/// Clean up cherry-pick state files after commit or abort
pub fn cherry_pick_cleanup(git_dir: &Path) {
    let _ = fs::remove_file(git_dir.join("CHERRY_PICK_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
}

// ── Shared helpers ──────────────────────────────────────────

/// Result of merging tree contents
#[derive(Debug, Clone)]
pub(crate) struct TreeMergeResult {
    pub has_conflicts: bool,
    pub files: Vec<(String, String, bool)>,
}

/// Load a commit's tree entries
fn load_commit_tree(
    git_dir: &Path,
    commit_oid: &OID,
) -> Result<Vec<TreeEntry>, MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject("expected commit".into()));
    }
    let commit = parse_commit(commit_oid.clone(), &data)?;
    load_tree_entries(git_dir, &commit.tree_id)
}

/// Load tree entries from a commit object directly
fn load_commit_tree_from_commit(
    git_dir: &Path,
    commit: &crate::commit::Commit,
) -> Result<Vec<TreeEntry>, MuonGitError> {
    load_tree_entries(git_dir, &commit.tree_id)
}

/// Load tree entries for a given tree OID
fn load_tree_entries(
    git_dir: &Path,
    tree_oid: &OID,
) -> Result<Vec<TreeEntry>, MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, tree_oid)?;
    if obj_type != ObjectType::Tree {
        return Err(MuonGitError::InvalidObject("expected tree".into()));
    }
    let tree = parse_tree(tree_oid.clone(), &data)?;
    Ok(tree.entries)
}

/// Read a blob's contents as a UTF-8 string (empty for missing blobs)
fn read_blob_text(git_dir: &Path, oid: &OID) -> String {
    if oid.is_zero() {
        return String::new();
    }
    match read_loose_object(git_dir, oid) {
        Ok((ObjectType::Blob, data)) => String::from_utf8(data).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Merge two trees against a base, producing per-file merge results.
///
/// This is a simplified tree-level merge: for each file present in any of the
/// three trees, perform a three-way text merge on the blob contents.
pub(crate) fn merge_trees_content(
    git_dir: &Path,
    base_entries: &[TreeEntry],
    ours_entries: &[TreeEntry],
    theirs_entries: &[TreeEntry],
) -> Result<TreeMergeResult, MuonGitError> {
    use std::collections::BTreeMap;

    // Index entries by name
    let mut all_paths = BTreeMap::new();
    for e in base_entries {
        all_paths
            .entry(e.name.clone())
            .or_insert((None, None, None))
            .0 = Some(e.oid.clone());
    }
    for e in ours_entries {
        all_paths
            .entry(e.name.clone())
            .or_insert((None, None, None))
            .1 = Some(e.oid.clone());
    }
    for e in theirs_entries {
        all_paths
            .entry(e.name.clone())
            .or_insert((None, None, None))
            .2 = Some(e.oid.clone());
    }

    let mut files = Vec::new();
    let mut has_conflicts = false;

    for (path, (base_oid, ours_oid, theirs_oid)) in &all_paths {
        let zero = OID::zero();
        let b = base_oid.as_ref().unwrap_or(&zero);
        let o = ours_oid.as_ref().unwrap_or(&zero);
        let t = theirs_oid.as_ref().unwrap_or(&zero);

        // If ours == theirs, no merge needed
        if o == t {
            let content = read_blob_text(git_dir, o);
            files.push((path.clone(), content, false));
            continue;
        }

        // If only one side changed from base, take that side
        if o == b {
            // Ours unchanged, theirs changed
            if t.is_zero() {
                // Theirs deleted
                continue;
            }
            let content = read_blob_text(git_dir, t);
            files.push((path.clone(), content, false));
            continue;
        }
        if t == b {
            // Theirs unchanged, ours changed
            if o.is_zero() {
                // Ours deleted
                continue;
            }
            let content = read_blob_text(git_dir, o);
            files.push((path.clone(), content, false));
            continue;
        }

        // Both sides changed — need content-level merge
        let base_text = read_blob_text(git_dir, b);
        let ours_text = read_blob_text(git_dir, o);
        let theirs_text = read_blob_text(git_dir, t);

        let merge_result = merge3(&base_text, &ours_text, &theirs_text);
        if merge_result.has_conflicts {
            has_conflicts = true;
            files.push((
                path.clone(),
                merge_result.to_string_with_markers(),
                true,
            ));
        } else {
            files.push((
                path.clone(),
                merge_result.to_clean_string().unwrap_or_default(),
                false,
            ));
        }
    }

    Ok(TreeMergeResult {
        has_conflicts,
        files,
    })
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
    fn test_cherry_pick_basic() {
        let (_base, git_dir) = setup_repo("cherrypick_basic");
        let sig = test_sig();

        // Create base: file.txt = "line1\nline2\nline3\n"
        let base_blob =
            write_loose_object(&git_dir, ObjectType::Blob, b"line1\nline2\nline3\n").unwrap();
        let base_tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: base_blob.clone(),
        }]);
        let base_tree = write_loose_object(&git_dir, ObjectType::Tree, &base_tree_data).unwrap();
        let c0_data = serialize_commit(&base_tree, &[], &sig, &sig, "base", None);
        let c0 = write_loose_object(&git_dir, ObjectType::Commit, &c0_data).unwrap();

        // HEAD branch: modify line2 → "line2-head"
        let head_blob =
            write_loose_object(&git_dir, ObjectType::Blob, b"line1\nline2-head\nline3\n")
                .unwrap();
        let head_tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: head_blob.clone(),
        }]);
        let head_tree =
            write_loose_object(&git_dir, ObjectType::Tree, &head_tree_data).unwrap();
        let c1_data = serialize_commit(&head_tree, &[c0.clone()], &sig, &sig, "head edit", None);
        let c1 = write_loose_object(&git_dir, ObjectType::Commit, &c1_data).unwrap();
        write_reference(&git_dir, "refs/heads/main", &c1).unwrap();

        // Other branch: add line4
        let other_blob =
            write_loose_object(&git_dir, ObjectType::Blob, b"line1\nline2\nline3\nline4\n")
                .unwrap();
        let other_tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: other_blob.clone(),
        }]);
        let other_tree =
            write_loose_object(&git_dir, ObjectType::Tree, &other_tree_data).unwrap();
        let c2_data =
            serialize_commit(&other_tree, &[c0.clone()], &sig, &sig, "add line4", None);
        let c2 = write_loose_object(&git_dir, ObjectType::Commit, &c2_data).unwrap();

        // Cherry-pick c2 onto HEAD (c1)
        let result = cherry_pick(&git_dir, &c2, &CherryPickOptions::default()).unwrap();
        assert!(!result.has_conflicts);
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].0, "file.txt");
        assert!(result.files[0].1.contains("line4"));
        assert!(result.files[0].1.contains("line2-head"));

        // CHERRY_PICK_HEAD should exist
        assert!(git_dir.join("CHERRY_PICK_HEAD").exists());
        cherry_pick_cleanup(&git_dir);
        assert!(!git_dir.join("CHERRY_PICK_HEAD").exists());
    }

    #[test]
    fn test_cherry_pick_conflict() {
        let (_base, git_dir) = setup_repo("cherrypick_conflict");
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

        // HEAD: file.txt = "hello-head\n"
        let head_blob =
            write_loose_object(&git_dir, ObjectType::Blob, b"hello-head\n").unwrap();
        let head_tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: head_blob.clone(),
        }]);
        let head_tree =
            write_loose_object(&git_dir, ObjectType::Tree, &head_tree_data).unwrap();
        let c1_data = serialize_commit(&head_tree, &[c0.clone()], &sig, &sig, "head", None);
        let c1 = write_loose_object(&git_dir, ObjectType::Commit, &c1_data).unwrap();
        write_reference(&git_dir, "refs/heads/main", &c1).unwrap();

        // Other: file.txt = "hello-other\n"
        let other_blob =
            write_loose_object(&git_dir, ObjectType::Blob, b"hello-other\n").unwrap();
        let other_tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: other_blob.clone(),
        }]);
        let other_tree =
            write_loose_object(&git_dir, ObjectType::Tree, &other_tree_data).unwrap();
        let c2_data =
            serialize_commit(&other_tree, &[c0.clone()], &sig, &sig, "other", None);
        let c2 = write_loose_object(&git_dir, ObjectType::Commit, &c2_data).unwrap();

        let result = cherry_pick(&git_dir, &c2, &CherryPickOptions::default()).unwrap();
        assert!(result.has_conflicts);
        assert!(result.files[0].2); // file is conflicted
        cherry_pick_cleanup(&git_dir);
    }

    #[test]
    fn test_cherry_pick_new_file() {
        let (_base, git_dir) = setup_repo("cherrypick_new_file");
        let sig = test_sig();

        // Base: single file
        let blob_a = write_loose_object(&git_dir, ObjectType::Blob, b"a\n").unwrap();
        let tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "a.txt".into(),
            oid: blob_a.clone(),
        }]);
        let tree = write_loose_object(&git_dir, ObjectType::Tree, &tree_data).unwrap();
        let c0 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree, &[], &sig, &sig, "initial", None),
        )
        .unwrap();
        write_reference(&git_dir, "refs/heads/main", &c0).unwrap();

        // Other branch: adds b.txt
        let blob_b = write_loose_object(&git_dir, ObjectType::Blob, b"b\n").unwrap();
        let tree2_data = serialize_tree(&[
            TreeEntry {
                mode: 0o100644,
                name: "a.txt".into(),
                oid: blob_a.clone(),
            },
            TreeEntry {
                mode: 0o100644,
                name: "b.txt".into(),
                oid: blob_b.clone(),
            },
        ]);
        let tree2 = write_loose_object(&git_dir, ObjectType::Tree, &tree2_data).unwrap();
        let c1 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree2, &[c0.clone()], &sig, &sig, "add b", None),
        )
        .unwrap();

        let result = cherry_pick(&git_dir, &c1, &CherryPickOptions::default()).unwrap();
        assert!(!result.has_conflicts);
        // Should have both files
        let names: Vec<_> = result.files.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"b.txt"));
        cherry_pick_cleanup(&git_dir);
    }
}
