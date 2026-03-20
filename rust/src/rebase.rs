//! Rebase — replay commits onto a new base
//! Parity: libgit2 src/libgit2/rebase.c

use std::fs;
use std::path::Path;

use crate::cherrypick::merge_trees_content;
use crate::commit::{parse_commit, serialize_commit, Commit};
use crate::error::MuonGitError;
use crate::odb::{read_loose_object, write_loose_object};
use crate::oid::OID;
use crate::refs::write_reference;
use crate::tree::{parse_tree, serialize_tree, TreeEntry};
use crate::types::{ObjectType, Signature};

/// Type of rebase operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebaseOperationType {
    /// Cherry-pick the commit
    Pick,
}

/// A single rebase operation
#[derive(Debug, Clone)]
pub struct RebaseOperation {
    pub op_type: RebaseOperationType,
    pub id: OID,
}

/// Options for rebase
#[derive(Debug, Clone, Default)]
pub struct RebaseOptions {
    /// Perform rebase in-memory (don't modify state files)
    pub inmemory: bool,
}

/// An in-progress rebase
#[derive(Debug)]
pub struct Rebase {
    git_dir: std::path::PathBuf,
    operations: Vec<RebaseOperation>,
    current: Option<usize>,
    onto_id: OID,
    orig_head_id: OID,
    orig_head_name: String,
    last_commit_id: Option<OID>,
    inmemory: bool,
}

impl Rebase {
    /// Start a new rebase.
    ///
    /// Replays all commits from `branch` that are not in `upstream` onto `onto`.
    /// If `onto` is None, uses `upstream` as the target base.
    pub fn init(
        git_dir: &Path,
        branch: &OID,
        upstream: &OID,
        onto: Option<&OID>,
        opts: &RebaseOptions,
    ) -> Result<Self, MuonGitError> {
        let onto_id = onto.unwrap_or(upstream).clone();

        // Collect commits from branch back to upstream (exclusive)
        let commits = collect_commits_to_rebase(git_dir, branch, upstream)?;

        if commits.is_empty() {
            return Err(MuonGitError::NotFound(
                "nothing to rebase".into(),
            ));
        }

        let operations: Vec<RebaseOperation> = commits
            .into_iter()
            .map(|id| RebaseOperation {
                op_type: RebaseOperationType::Pick,
                id,
            })
            .collect();

        // Read original HEAD for restore on abort
        let head_ref = fs::read_to_string(git_dir.join("HEAD"))
            .map_err(|_| MuonGitError::NotFound("cannot read HEAD".into()))?;
        let orig_head_name = head_ref.trim().to_string();

        if !opts.inmemory {
            // Write state files
            let state_dir = git_dir.join("rebase-merge");
            fs::create_dir_all(&state_dir)?;
            fs::write(state_dir.join("head-name"), &orig_head_name)?;
            fs::write(state_dir.join("orig-head"), branch.hex())?;
            fs::write(state_dir.join("onto"), onto_id.hex())?;
            fs::write(
                state_dir.join("end"),
                operations.len().to_string(),
            )?;
            fs::write(state_dir.join("msgnum"), "0")?;

            for (i, op) in operations.iter().enumerate() {
                fs::write(
                    state_dir.join(format!("cmt.{}", i + 1)),
                    op.id.hex(),
                )?;
            }
        }

        Ok(Rebase {
            git_dir: git_dir.to_path_buf(),
            operations,
            current: None,
            onto_id: onto_id.clone(),
            orig_head_id: branch.clone(),
            orig_head_name,
            last_commit_id: Some(onto_id),
            inmemory: opts.inmemory,
        })
    }

    /// Open an existing rebase in progress
    pub fn open(git_dir: &Path) -> Result<Self, MuonGitError> {
        let state_dir = git_dir.join("rebase-merge");
        if !state_dir.exists() {
            return Err(MuonGitError::NotFound("no rebase in progress".into()));
        }

        let orig_head_name = fs::read_to_string(state_dir.join("head-name"))
            .map_err(|_| MuonGitError::NotFound("missing head-name".into()))?
            .trim()
            .to_string();
        let orig_head_hex = fs::read_to_string(state_dir.join("orig-head"))
            .map_err(|_| MuonGitError::NotFound("missing orig-head".into()))?;
        let onto_hex = fs::read_to_string(state_dir.join("onto"))
            .map_err(|_| MuonGitError::NotFound("missing onto".into()))?;
        let end: usize = fs::read_to_string(state_dir.join("end"))
            .map_err(|_| MuonGitError::NotFound("missing end".into()))?
            .trim()
            .parse()
            .map_err(|_| MuonGitError::InvalidObject("invalid end".into()))?;
        let msgnum: usize = fs::read_to_string(state_dir.join("msgnum"))
            .map_err(|_| MuonGitError::NotFound("missing msgnum".into()))?
            .trim()
            .parse()
            .map_err(|_| MuonGitError::InvalidObject("invalid msgnum".into()))?;

        let mut operations = Vec::with_capacity(end);
        for i in 1..=end {
            let hex = fs::read_to_string(state_dir.join(format!("cmt.{}", i)))
                .map_err(|_| MuonGitError::NotFound(format!("missing cmt.{}", i)))?;
            operations.push(RebaseOperation {
                op_type: RebaseOperationType::Pick,
                id: OID::from_hex(hex.trim())?,
            });
        }

        let current = if msgnum > 0 { Some(msgnum - 1) } else { None };

        Ok(Rebase {
            git_dir: git_dir.to_path_buf(),
            operations,
            current,
            onto_id: OID::from_hex(onto_hex.trim())?,
            orig_head_id: OID::from_hex(orig_head_hex.trim())?,
            orig_head_name,
            last_commit_id: None,
            inmemory: false,
        })
    }

    /// Get the next operation and apply the patch.
    ///
    /// Returns the operation or None if all operations are done.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<&RebaseOperation>, MuonGitError> {
        let next_idx = match self.current {
            Some(i) => i + 1,
            None => 0,
        };

        if next_idx >= self.operations.len() {
            return Ok(None);
        }

        self.current = Some(next_idx);

        if !self.inmemory {
            let state_dir = self.git_dir.join("rebase-merge");
            fs::write(
                state_dir.join("msgnum"),
                (next_idx + 1).to_string(),
            )?;
        }

        Ok(Some(&self.operations[next_idx]))
    }

    /// Apply the current operation (cherry-pick the commit onto the current base).
    ///
    /// Returns merge result: (has_conflicts, files)
    #[allow(clippy::type_complexity)]
    pub fn apply_current(
        &self,
    ) -> Result<(bool, Vec<(String, String, bool)>), MuonGitError> {
        let idx = self.current.ok_or_else(|| {
            MuonGitError::NotFound("no current rebase operation".into())
        })?;

        let op = &self.operations[idx];
        let (obj_type, data) = read_loose_object(&self.git_dir, &op.id)?;
        if obj_type != ObjectType::Commit {
            return Err(MuonGitError::InvalidObject("not a commit".into()));
        }
        let commit = parse_commit(op.id.clone(), &data)?;

        if commit.parent_ids.is_empty() {
            return Err(MuonGitError::InvalidObject(
                "cannot rebase a root commit".into(),
            ));
        }

        // base = commit's parent, ours = current onto tip, theirs = commit
        let parent_tree = load_commit_tree(&self.git_dir, &commit.parent_ids[0])?;
        let onto_tip = self.last_commit_id.as_ref().unwrap_or(&self.onto_id);
        let ours_tree = load_commit_tree(&self.git_dir, onto_tip)?;
        let theirs_tree = load_commit_tree_direct(&self.git_dir, &commit)?;

        let result = merge_trees_content(&self.git_dir, &parent_tree, &ours_tree, &theirs_tree)?;

        Ok((result.has_conflicts, result.files))
    }

    /// Commit the current operation's result.
    ///
    /// Creates a new commit with the given tree entries on top of the current base.
    pub fn commit(
        &mut self,
        author: Option<&Signature>,
        committer: &Signature,
        message: Option<&str>,
    ) -> Result<OID, MuonGitError> {
        let idx = self.current.ok_or_else(|| {
            MuonGitError::NotFound("no current rebase operation".into())
        })?;

        let op = &self.operations[idx];

        // Read original commit for author/message defaults
        let (_, data) = read_loose_object(&self.git_dir, &op.id)?;
        let orig_commit = parse_commit(op.id.clone(), &data)?;

        let actual_author = author.unwrap_or(&orig_commit.author);
        let actual_message = message.unwrap_or(&orig_commit.message);

        // Apply the operation to get the merged tree
        let (has_conflicts, files) = self.apply_current()?;
        if has_conflicts {
            return Err(MuonGitError::Conflict(
                "cannot commit with conflicts".into(),
            ));
        }

        // Build a new tree from the merged files
        let mut entries = Vec::new();
        for (path, content, _) in &files {
            let blob_oid = write_loose_object(
                &self.git_dir,
                ObjectType::Blob,
                content.as_bytes(),
            )?;
            entries.push(TreeEntry {
                mode: 0o100644,
                name: path.clone(),
                oid: blob_oid,
            });
        }
        let tree_data = serialize_tree(&entries);
        let tree_oid = write_loose_object(&self.git_dir, ObjectType::Tree, &tree_data)?;

        // Create the new commit
        let parent = self.last_commit_id.as_ref().unwrap_or(&self.onto_id);
        let commit_data = serialize_commit(
            &tree_oid,
            std::slice::from_ref(parent),
            actual_author,
            committer,
            actual_message,
            orig_commit.message_encoding.as_deref(),
        );
        let new_oid = write_loose_object(&self.git_dir, ObjectType::Commit, &commit_data)?;

        self.last_commit_id = Some(new_oid.clone());
        Ok(new_oid)
    }

    /// Abort the rebase and restore original state.
    pub fn abort(&self) -> Result<(), MuonGitError> {
        if !self.inmemory {
            // Restore HEAD
            fs::write(
                self.git_dir.join("HEAD"),
                format!("{}\n", self.orig_head_name),
            )?;

            // If it was a symbolic ref, write the original commit back
            if self.orig_head_name.starts_with("ref: ") {
                let ref_name = self.orig_head_name.trim_start_matches("ref: ");
                write_reference(&self.git_dir, ref_name, &self.orig_head_id)?;
            }

            // Clean up state
            let state_dir = self.git_dir.join("rebase-merge");
            let _ = fs::remove_dir_all(&state_dir);
        }
        Ok(())
    }

    /// Finish the rebase — update the branch ref and clean up.
    pub fn finish(&self) -> Result<(), MuonGitError> {
        if !self.inmemory {
            // Update the branch ref to point at the last commit
            if let Some(ref new_head) = self.last_commit_id {
                if self.orig_head_name.starts_with("ref: ") {
                    let ref_name = self.orig_head_name.trim_start_matches("ref: ");
                    write_reference(&self.git_dir, ref_name, new_head)?;
                }
            }

            let state_dir = self.git_dir.join("rebase-merge");
            let _ = fs::remove_dir_all(&state_dir);
        }
        Ok(())
    }

    /// Number of operations
    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }

    /// Current operation index (0-based), or None if not started
    pub fn current_operation(&self) -> Option<usize> {
        self.current
    }

    /// Get operation by index
    pub fn operation_at(&self, idx: usize) -> Option<&RebaseOperation> {
        self.operations.get(idx)
    }
}

/// Collect commits from `branch` back to `upstream` (exclusive), in chronological order.
fn collect_commits_to_rebase(
    git_dir: &Path,
    branch: &OID,
    upstream: &OID,
) -> Result<Vec<OID>, MuonGitError> {
    let mut commits = Vec::new();
    let mut current = branch.clone();

    // Walk back from branch until we reach upstream
    for _ in 0..10000 {
        if current == *upstream {
            break;
        }

        let (obj_type, data) = match read_loose_object(git_dir, &current) {
            Ok(v) => v,
            Err(_) => break,
        };
        if obj_type != ObjectType::Commit {
            break;
        }
        let commit = parse_commit(current.clone(), &data)?;

        commits.push(current.clone());

        // Follow first parent only
        if let Some(parent) = commit.parent_ids.first() {
            current = parent.clone();
        } else {
            break;
        }
    }

    // Reverse to get chronological order
    commits.reverse();
    Ok(commits)
}

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

fn load_commit_tree_direct(
    git_dir: &Path,
    commit: &Commit,
) -> Result<Vec<TreeEntry>, MuonGitError> {
    load_tree_entries(git_dir, &commit.tree_id)
}

fn load_tree_entries(
    git_dir: &Path,
    tree_oid: &OID,
) -> Result<Vec<TreeEntry>, MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, tree_oid)?;
    if obj_type != ObjectType::Tree {
        return Err(MuonGitError::InvalidObject("expected tree".into()));
    }
    Ok(parse_tree(tree_oid.clone(), &data)?.entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::serialize_commit;
    use crate::odb::write_loose_object;
    use crate::refs::write_reference;
    use crate::tree::serialize_tree;

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

    /// Create a linear chain: c0 --c1-- c2 -- c3
    /// with file.txt changing at each step
    fn create_linear_chain(
        git_dir: &Path,
    ) -> (OID, OID, OID, OID) {
        let sig = test_sig();

        let blob0 = write_loose_object(git_dir, ObjectType::Blob, b"line1\n").unwrap();
        let tree0_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob0,
        }]);
        let tree0 = write_loose_object(git_dir, ObjectType::Tree, &tree0_data).unwrap();
        let c0 = write_loose_object(
            git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree0, &[], &sig, &sig, "c0", None),
        )
        .unwrap();

        let blob1 = write_loose_object(git_dir, ObjectType::Blob, b"line1\nline2\n").unwrap();
        let tree1_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob1,
        }]);
        let tree1 = write_loose_object(git_dir, ObjectType::Tree, &tree1_data).unwrap();
        let c1 = write_loose_object(
            git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree1, &[c0.clone()], &sig, &sig, "c1: add line2", None),
        )
        .unwrap();

        let blob2 = write_loose_object(git_dir, ObjectType::Blob, b"line1\nline2\nline3\n").unwrap();
        let tree2_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob2,
        }]);
        let tree2 = write_loose_object(git_dir, ObjectType::Tree, &tree2_data).unwrap();
        let c2 = write_loose_object(
            git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree2, &[c1.clone()], &sig, &sig, "c2: add line3", None),
        )
        .unwrap();

        let blob3 = write_loose_object(
            git_dir,
            ObjectType::Blob,
            b"line1\nline2\nline3\nline4\n",
        )
        .unwrap();
        let tree3_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob3,
        }]);
        let tree3 = write_loose_object(git_dir, ObjectType::Tree, &tree3_data).unwrap();
        let c3 = write_loose_object(
            git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree3, &[c2.clone()], &sig, &sig, "c3: add line4", None),
        )
        .unwrap();

        (c0, c1, c2, c3)
    }

    #[test]
    fn test_rebase_basic() {
        let (_base, git_dir) = setup_repo("rebase_basic");
        let sig = test_sig();

        // c0 -- c1 (upstream/main)
        //    \-- c2 -- c3 (topic)
        let blob0 = write_loose_object(&git_dir, ObjectType::Blob, b"base\n").unwrap();
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

        // Main branch: adds main.txt
        let blob_main = write_loose_object(&git_dir, ObjectType::Blob, b"main\n").unwrap();
        let tree1_data = serialize_tree(&[
            TreeEntry { mode: 0o100644, name: "file.txt".into(), oid: blob0.clone() },
            TreeEntry { mode: 0o100644, name: "main.txt".into(), oid: blob_main },
        ]);
        let tree1 = write_loose_object(&git_dir, ObjectType::Tree, &tree1_data).unwrap();
        let c1 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree1, &[c0.clone()], &sig, &sig, "main: add main.txt", None),
        )
        .unwrap();

        // Topic branch: modifies file.txt
        let blob_topic = write_loose_object(&git_dir, ObjectType::Blob, b"base\ntopic line\n").unwrap();
        let tree2_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "file.txt".into(),
            oid: blob_topic,
        }]);
        let tree2 = write_loose_object(&git_dir, ObjectType::Tree, &tree2_data).unwrap();
        let c2 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree2, &[c0.clone()], &sig, &sig, "topic: modify file", None),
        )
        .unwrap();

        write_reference(&git_dir, "refs/heads/main", &c1).unwrap();

        // Rebase c2 onto c1 (rebase topic onto main)
        let mut rebase = Rebase::init(
            &git_dir,
            &c2,
            &c0, // upstream = c0 (the fork point)
            Some(&c1), // onto = c1 (main tip)
            &RebaseOptions::default(),
        )
        .unwrap();

        assert_eq!(rebase.operation_count(), 1);

        // Apply the operation
        let op = rebase.next().unwrap();
        assert!(op.is_some());
        let (has_conflicts, _files) = rebase.apply_current().unwrap();
        assert!(!has_conflicts);

        // Commit
        let new_oid = rebase.commit(None, &sig, None).unwrap();
        assert!(!new_oid.is_zero());

        // No more operations
        let op = rebase.next().unwrap();
        assert!(op.is_none());

        // Finish
        rebase.finish().unwrap();

        // State dir should be cleaned up
        assert!(!git_dir.join("rebase-merge").exists());
    }

    #[test]
    fn test_rebase_multiple_commits() {
        let (_base, git_dir) = setup_repo("rebase_multi");
        let (c0, _c1, _c2, c3) = create_linear_chain(&git_dir);

        write_reference(&git_dir, "refs/heads/main", &c0).unwrap();

        // Rebase c1..c3 onto c0 (should replay c1, c2, c3)
        let mut rebase = Rebase::init(
            &git_dir,
            &c3,
            &c0,
            None,
            &RebaseOptions { inmemory: true },
        )
        .unwrap();

        assert_eq!(rebase.operation_count(), 3);
        let sig = test_sig();

        for _ in 0..3 {
            let op = rebase.next().unwrap();
            assert!(op.is_some());
            let (has_conflicts, _) = rebase.apply_current().unwrap();
            assert!(!has_conflicts);
            rebase.commit(None, &sig, None).unwrap();
        }

        assert!(rebase.next().unwrap().is_none());
    }

    #[test]
    fn test_rebase_abort() {
        let (_base, git_dir) = setup_repo("rebase_abort");
        let sig = test_sig();

        let blob = write_loose_object(&git_dir, ObjectType::Blob, b"data\n").unwrap();
        let tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "f.txt".into(),
            oid: blob,
        }]);
        let tree = write_loose_object(&git_dir, ObjectType::Tree, &tree_data).unwrap();
        let c0 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree, &[], &sig, &sig, "c0", None),
        )
        .unwrap();

        let blob2 = write_loose_object(&git_dir, ObjectType::Blob, b"data2\n").unwrap();
        let tree2_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "f.txt".into(),
            oid: blob2,
        }]);
        let tree2 = write_loose_object(&git_dir, ObjectType::Tree, &tree2_data).unwrap();
        let c1 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree2, &[c0.clone()], &sig, &sig, "c1", None),
        )
        .unwrap();

        write_reference(&git_dir, "refs/heads/main", &c0).unwrap();

        let rebase = Rebase::init(
            &git_dir,
            &c1,
            &c0,
            None,
            &RebaseOptions::default(),
        )
        .unwrap();

        assert!(git_dir.join("rebase-merge").exists());
        rebase.abort().unwrap();
        assert!(!git_dir.join("rebase-merge").exists());
    }

    #[test]
    fn test_rebase_open() {
        let (_base, git_dir) = setup_repo("rebase_open");
        let sig = test_sig();

        let blob = write_loose_object(&git_dir, ObjectType::Blob, b"data\n").unwrap();
        let tree_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "f.txt".into(),
            oid: blob,
        }]);
        let tree = write_loose_object(&git_dir, ObjectType::Tree, &tree_data).unwrap();
        let c0 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree, &[], &sig, &sig, "c0", None),
        )
        .unwrap();

        let blob2 = write_loose_object(&git_dir, ObjectType::Blob, b"more\n").unwrap();
        let tree2_data = serialize_tree(&[TreeEntry {
            mode: 0o100644,
            name: "f.txt".into(),
            oid: blob2,
        }]);
        let tree2 = write_loose_object(&git_dir, ObjectType::Tree, &tree2_data).unwrap();
        let c1 = write_loose_object(
            &git_dir,
            ObjectType::Commit,
            &serialize_commit(&tree2, &[c0.clone()], &sig, &sig, "c1", None),
        )
        .unwrap();

        write_reference(&git_dir, "refs/heads/main", &c0).unwrap();

        // Init rebase
        let _rebase = Rebase::init(
            &git_dir,
            &c1,
            &c0,
            None,
            &RebaseOptions::default(),
        )
        .unwrap();

        // Re-open
        let reopened = Rebase::open(&git_dir).unwrap();
        assert_eq!(reopened.operation_count(), 1);
        assert_eq!(reopened.operations[0].id, c1);

        // Clean up
        reopened.abort().unwrap();
    }
}
