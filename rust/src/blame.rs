//! Line-by-line attribution of file contents to commits
//! Parity: libgit2 src/libgit2/blame.c, blame_git.c

use std::path::Path;

use crate::commit::{parse_commit, Commit};
use crate::diff::{diff_lines, EditKind};
use crate::error::MuonGitError;
use crate::odb::read_loose_object;
use crate::oid::OID;
use crate::refs::resolve_reference;
use crate::tree::{parse_tree, TreeEntry};
use crate::types::{ObjectType, Signature};

/// Options controlling blame behavior
#[derive(Debug, Clone, Default)]
pub struct BlameOptions {
    /// Restrict blame to this commit range (newest). Default: HEAD.
    pub newest_commit: Option<OID>,
    /// Stop blaming at this commit. Default: root.
    pub oldest_commit: Option<OID>,
    /// Only blame lines in [min_line, max_line] (1-based, inclusive). 0 = all.
    pub min_line: usize,
    pub max_line: usize,
}

/// A hunk of lines attributed to a single commit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameHunk {
    /// Number of lines in this hunk
    pub lines_in_hunk: usize,
    /// The commit that introduced these lines
    pub final_commit_id: OID,
    /// 1-based start line in the final file
    pub final_start_line_number: usize,
    /// Author signature from the blamed commit
    pub final_signature: Option<Signature>,
    /// The original commit (same as final unless tracking copies)
    pub orig_commit_id: OID,
    /// 1-based start line in the original file
    pub orig_start_line_number: usize,
    /// Original path if different from blamed path
    pub orig_path: Option<String>,
    /// True if this hunk goes past the oldest_commit boundary
    pub boundary: bool,
}

/// Result of a blame operation
#[derive(Debug, Clone)]
pub struct BlameResult {
    /// The path that was blamed
    pub path: String,
    /// Blame hunks covering all lines
    pub hunks: Vec<BlameHunk>,
    /// Total line count in the file
    pub line_count: usize,
}

impl BlameResult {
    /// Number of hunks
    pub fn hunk_count(&self) -> usize {
        self.hunks.len()
    }

    /// Get hunk by 0-based index
    pub fn hunk_by_index(&self, index: usize) -> Option<&BlameHunk> {
        self.hunks.get(index)
    }

    /// Get the hunk that covers a specific 1-based line number
    pub fn hunk_by_line(&self, line: usize) -> Option<&BlameHunk> {
        if line == 0 || line > self.line_count {
            return None;
        }
        for hunk in &self.hunks {
            let end = hunk.final_start_line_number + hunk.lines_in_hunk;
            if line >= hunk.final_start_line_number && line < end {
                return Some(hunk);
            }
        }
        None
    }
}

/// Blame a file, attributing each line to the commit that last changed it.
///
/// Walks the commit history from `newest_commit` (default HEAD) backwards,
/// diffing each commit against its first parent to track which lines changed.
pub fn blame_file(
    git_dir: &Path,
    path: &str,
    options: Option<&BlameOptions>,
) -> Result<BlameResult, MuonGitError> {
    let opts = options.cloned().unwrap_or_default();

    // Resolve the starting commit
    let start_oid = match &opts.newest_commit {
        Some(oid) => oid.clone(),
        None => resolve_reference(git_dir, "HEAD")?
    };

    // Read the file content at the starting commit
    let file_content = read_blob_at_commit(git_dir, &start_oid, path)?;
    let lines: Vec<&str> = if file_content.is_empty() {
        Vec::new()
    } else {
        file_content.split('\n').collect()
    };

    let total_lines = lines.len();
    if total_lines == 0 {
        return Ok(BlameResult {
            path: path.to_string(),
            hunks: Vec::new(),
            line_count: 0,
        });
    }

    // Determine line range to blame
    let min_line = if opts.min_line > 0 { opts.min_line } else { 1 };
    let max_line = if opts.max_line > 0 {
        opts.max_line.min(total_lines)
    } else {
        total_lines
    };

    // Initialize: all lines attributed to start commit, per-line tracking
    // line_owners[i] = (commit_oid, original_line_1based)
    let mut line_owners: Vec<Option<(OID, usize)>> = vec![None; total_lines];

    // Walk history
    let mut current_oid = start_oid.clone();
    let mut current_content = file_content.clone();
    let mut remaining = max_line - min_line + 1;

    // Limit walk depth to avoid infinite loops
    let max_depth = 10000;
    let mut depth = 0;

    while remaining > 0 && depth < max_depth {
        depth += 1;

        // Read the commit
        let commit = read_commit(git_dir, &current_oid)?;

        if commit.parent_ids.is_empty() {
            // Root commit — attribute all remaining lines to this commit
            for (i, owner) in line_owners.iter_mut().enumerate() {
                let line_1 = i + 1;
                if owner.is_none() && line_1 >= min_line && line_1 <= max_line {
                    *owner = Some((current_oid.clone(), line_1));
                }
            }
            break;
        }

        // Check oldest_commit boundary
        if let Some(ref oldest) = opts.oldest_commit {
            if &current_oid == oldest {
                for (i, owner) in line_owners.iter_mut().enumerate() {
                    let line_1 = i + 1;
                    if owner.is_none() && line_1 >= min_line && line_1 <= max_line {
                        *owner = Some((current_oid.clone(), line_1));
                    }
                }
                break;
            }
        }

        let parent_oid = &commit.parent_ids[0];

        // Try reading the file at the parent commit
        let parent_content = match read_blob_at_commit(git_dir, parent_oid, path) {
            Ok(content) => content,
            Err(_) => {
                // File didn't exist in parent — all remaining lines belong to current commit
                for (i, owner) in line_owners.iter_mut().enumerate() {
                    let line_1 = i + 1;
                    if owner.is_none() && line_1 >= min_line && line_1 <= max_line {
                        *owner = Some((current_oid.clone(), line_1));
                    }
                }
                break;
            }
        };

        if parent_content == current_content {
            // File unchanged — move blame to parent
            current_oid = parent_oid.clone();
            continue;
        }

        // Diff parent content vs current content
        let edits = diff_lines(&parent_content, &current_content);

        // Lines that are Equal (present in both) are NOT introduced by current commit.
        // Lines that are Insert-only in new are introduced by current commit.
        for edit in &edits {
            if edit.kind == EditKind::Insert && edit.new_line > 0 {
                let line_idx = edit.new_line - 1;
                if line_idx < total_lines {
                    let line_1 = line_idx + 1;
                    if line_owners[line_idx].is_none()
                        && line_1 >= min_line
                        && line_1 <= max_line
                    {
                        line_owners[line_idx] = Some((current_oid.clone(), line_1));
                        remaining -= 1;
                    }
                }
            }
        }

        current_oid = parent_oid.clone();
        current_content = parent_content;
    }

    // Any still-unowned lines get attributed to the start commit
    for (i, owner) in line_owners.iter_mut().enumerate() {
        let line_1 = i + 1;
        if owner.is_none() && line_1 >= min_line && line_1 <= max_line {
            *owner = Some((start_oid.clone(), line_1));
        }
    }

    // Build hunks from consecutive lines with same commit
    let mut hunks = Vec::new();
    let mut i = min_line - 1; // 0-based index

    while i < max_line {
        let (commit_id, orig_line) = line_owners[i]
            .clone()
            .unwrap_or((start_oid.clone(), i + 1));

        let start_line = i + 1; // 1-based
        let mut count = 1;

        // Extend hunk while next lines have same commit
        while i + count < max_line {
            if let Some((ref next_oid, _)) = line_owners[i + count] {
                if *next_oid == commit_id {
                    count += 1;
                    continue;
                }
            }
            break;
        }

        // Load signature from the commit
        let sig = read_commit(git_dir, &commit_id)
            .ok()
            .map(|c| c.author);

        let is_boundary = opts
            .oldest_commit
            .as_ref()
            .is_some_and(|oldest| commit_id == *oldest);

        hunks.push(BlameHunk {
            lines_in_hunk: count,
            final_commit_id: commit_id.clone(),
            final_start_line_number: start_line,
            final_signature: sig,
            orig_commit_id: commit_id,
            orig_start_line_number: orig_line,
            orig_path: None,
            boundary: is_boundary,
        });

        i += count;
    }

    Ok(BlameResult {
        path: path.to_string(),
        hunks,
        line_count: total_lines,
    })
}

// --- Internal helpers ---

/// Read and parse a commit from the ODB
fn read_commit(git_dir: &Path, oid: &OID) -> Result<Commit, MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject(format!(
            "expected commit, got {:?}",
            obj_type
        )));
    }
    parse_commit(oid.clone(), &data)
}

/// Read the blob content of a file at a given commit
fn read_blob_at_commit(
    git_dir: &Path,
    commit_oid: &OID,
    path: &str,
) -> Result<String, MuonGitError> {
    let commit = read_commit(git_dir, commit_oid)?;
    let (tree_type, tree_data) = read_loose_object(git_dir, &commit.tree_id)?;
    if tree_type != ObjectType::Tree {
        return Err(MuonGitError::InvalidObject("expected tree".into()));
    }
    let tree = parse_tree(commit.tree_id.clone(), &tree_data)?;

    // Find the entry for the given path (supports simple single-level paths)
    let entry = find_tree_entry_by_path(git_dir, &tree.entries, path)?;

    let (blob_type, blob_data) = read_loose_object(git_dir, &entry.oid)?;
    if blob_type != ObjectType::Blob {
        return Err(MuonGitError::InvalidObject("expected blob".into()));
    }
    Ok(String::from_utf8_lossy(&blob_data).to_string())
}

/// Find a tree entry by path, supporting nested paths like "dir/file.txt"
fn find_tree_entry_by_path(
    git_dir: &Path,
    entries: &[TreeEntry],
    path: &str,
) -> Result<TreeEntry, MuonGitError> {
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let name = parts[0];

    let entry = entries
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| MuonGitError::NotFound(format!("path not found: {}", path)))?;

    if parts.len() == 1 {
        // Leaf entry
        return Ok(entry.clone());
    }

    // It's a subdirectory — recurse
    let (sub_type, sub_data) = read_loose_object(git_dir, &entry.oid)?;
    if sub_type != ObjectType::Tree {
        return Err(MuonGitError::InvalidObject(format!(
            "expected tree for directory {}",
            name
        )));
    }
    let sub_tree = parse_tree(entry.oid.clone(), &sub_data)?;
    find_tree_entry_by_path(git_dir, &sub_tree.entries, parts[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::serialize_commit;
    use crate::odb::write_loose_object;
    use crate::repository::Repository;
    use crate::tree::serialize_tree;
    use crate::types::Signature;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        std::fs::create_dir_all(&base).unwrap();
        let p = base.join(format!("test_blame_{}", name));
        if p.exists() {
            std::fs::remove_dir_all(&p).unwrap();
        }
        p
    }

    fn make_sig(name: &str) -> Signature {
        Signature {
            name: name.to_string(),
            email: format!("{}@test.tt", name),
            time: 1700000000,
            offset: 0,
        }
    }

    fn write_blob(git_dir: &Path, content: &str) -> OID {
        write_loose_object(git_dir, ObjectType::Blob, content.as_bytes()).unwrap()
    }

    fn write_tree_with_blob(git_dir: &Path, filename: &str, blob_oid: &OID) -> OID {
        let entries = vec![TreeEntry {
            mode: crate::tree::file_mode::BLOB,
            name: filename.to_string(),
            oid: blob_oid.clone(),
        }];
        let data = serialize_tree(&entries);
        write_loose_object(git_dir, ObjectType::Tree, &data).unwrap()
    }

    fn write_commit(
        git_dir: &Path,
        tree_oid: &OID,
        parents: &[OID],
        author: &Signature,
        msg: &str,
    ) -> OID {
        let data = serialize_commit(tree_oid, parents, author, author, msg, None);
        write_loose_object(git_dir, ObjectType::Commit, &data).unwrap()
    }

    #[test]
    fn test_blame_single_commit() {
        let tmp = test_dir("single");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();

        let blob = write_blob(&git_dir, "line1\nline2\nline3");
        let tree = write_tree_with_blob(&git_dir, "file.txt", &blob);
        let sig = make_sig("alice");
        let commit = write_commit(&git_dir, &tree, &[], &sig, "initial");

        // Update HEAD
        crate::refs::write_reference(&git_dir, "refs/heads/main", &commit).unwrap();
        crate::refs::write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        let result = blame_file(&git_dir, "file.txt", None).unwrap();
        assert_eq!(result.line_count, 3);
        assert_eq!(result.hunk_count(), 1);
        let hunk = result.hunk_by_index(0).unwrap();
        assert_eq!(hunk.lines_in_hunk, 3);
        assert_eq!(hunk.final_commit_id, commit);
        assert_eq!(hunk.final_start_line_number, 1);
    }

    #[test]
    fn test_blame_two_commits() {
        let tmp = test_dir("two_commits");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();

        // First commit: line1\nline2
        let blob1 = write_blob(&git_dir, "line1\nline2");
        let tree1 = write_tree_with_blob(&git_dir, "file.txt", &blob1);
        let sig1 = make_sig("alice");
        let c1 = write_commit(&git_dir, &tree1, &[], &sig1, "first");

        // Second commit: line1\ninserted\nline2
        let blob2 = write_blob(&git_dir, "line1\ninserted\nline2");
        let tree2 = write_tree_with_blob(&git_dir, "file.txt", &blob2);
        let sig2 = make_sig("bob");
        let c2 = write_commit(&git_dir, &tree2, &[c1.clone()], &sig2, "second");

        crate::refs::write_reference(&git_dir, "refs/heads/main", &c2).unwrap();
        crate::refs::write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        let result = blame_file(&git_dir, "file.txt", None).unwrap();
        assert_eq!(result.line_count, 3);

        // line1 -> c1, inserted -> c2, line2 -> c1
        let h1 = result.hunk_by_line(1).unwrap();
        assert_eq!(h1.final_commit_id, c1);

        let h2 = result.hunk_by_line(2).unwrap();
        assert_eq!(h2.final_commit_id, c2);

        let h3 = result.hunk_by_line(3).unwrap();
        assert_eq!(h3.final_commit_id, c1);
    }

    #[test]
    fn test_blame_line_range() {
        let tmp = test_dir("line_range");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();

        let blob = write_blob(&git_dir, "a\nb\nc\nd\ne");
        let tree = write_tree_with_blob(&git_dir, "file.txt", &blob);
        let sig = make_sig("alice");
        let commit = write_commit(&git_dir, &tree, &[], &sig, "init");

        crate::refs::write_reference(&git_dir, "refs/heads/main", &commit).unwrap();
        crate::refs::write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        let opts = BlameOptions {
            min_line: 2,
            max_line: 4,
            ..Default::default()
        };
        let result = blame_file(&git_dir, "file.txt", Some(&opts)).unwrap();
        assert_eq!(result.line_count, 5); // Total lines in file
        // Only lines 2-4 should be blamed
        let total_blamed: usize = result.hunks.iter().map(|h| h.lines_in_hunk).sum();
        assert_eq!(total_blamed, 3);
    }

    #[test]
    fn test_blame_hunk_by_line() {
        let tmp = test_dir("hunk_by_line");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();

        let blob = write_blob(&git_dir, "hello\nworld");
        let tree = write_tree_with_blob(&git_dir, "test.txt", &blob);
        let sig = make_sig("carol");
        let commit = write_commit(&git_dir, &tree, &[], &sig, "init");

        crate::refs::write_reference(&git_dir, "refs/heads/main", &commit).unwrap();
        crate::refs::write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        let result = blame_file(&git_dir, "test.txt", None).unwrap();
        assert!(result.hunk_by_line(0).is_none()); // 0 is invalid
        assert!(result.hunk_by_line(1).is_some());
        assert!(result.hunk_by_line(2).is_some());
        assert!(result.hunk_by_line(3).is_none()); // past end
    }

    #[test]
    fn test_blame_empty_file() {
        let tmp = test_dir("empty_file");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();

        let blob = write_blob(&git_dir, "");
        let tree = write_tree_with_blob(&git_dir, "empty.txt", &blob);
        let sig = make_sig("dave");
        let commit = write_commit(&git_dir, &tree, &[], &sig, "empty");

        crate::refs::write_reference(&git_dir, "refs/heads/main", &commit).unwrap();
        crate::refs::write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        let result = blame_file(&git_dir, "empty.txt", None).unwrap();
        assert_eq!(result.line_count, 0);
        assert_eq!(result.hunk_count(), 0);
    }

    #[test]
    fn test_blame_subdirectory() {
        let tmp = test_dir("subdir");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();

        // Create blob
        let blob = write_blob(&git_dir, "nested content");

        // Create inner tree with file
        let inner_entries = vec![TreeEntry {
            mode: crate::tree::file_mode::BLOB,
            name: "deep.txt".to_string(),
            oid: blob.clone(),
        }];
        let inner_data = serialize_tree(&inner_entries);
        let inner_tree = write_loose_object(&git_dir, ObjectType::Tree, &inner_data).unwrap();

        // Create outer tree with directory
        let outer_entries = vec![TreeEntry {
            mode: crate::tree::file_mode::TREE,
            name: "subdir".to_string(),
            oid: inner_tree,
        }];
        let outer_data = serialize_tree(&outer_entries);
        let outer_tree = write_loose_object(&git_dir, ObjectType::Tree, &outer_data).unwrap();

        let sig = make_sig("eve");
        let commit = write_commit(&git_dir, &outer_tree, &[], &sig, "nested");

        crate::refs::write_reference(&git_dir, "refs/heads/main", &commit).unwrap();
        crate::refs::write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        let result = blame_file(&git_dir, "subdir/deep.txt", None).unwrap();
        assert_eq!(result.line_count, 1);
        assert_eq!(result.hunk_count(), 1);
        let hunk = &result.hunks[0];
        assert_eq!(hunk.final_commit_id, commit);
    }

    #[test]
    fn test_blame_file_not_found() {
        let tmp = test_dir("not_found");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();

        let blob = write_blob(&git_dir, "content");
        let tree = write_tree_with_blob(&git_dir, "exists.txt", &blob);
        let sig = make_sig("frank");
        let commit = write_commit(&git_dir, &tree, &[], &sig, "init");

        crate::refs::write_reference(&git_dir, "refs/heads/main", &commit).unwrap();
        crate::refs::write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        let result = blame_file(&git_dir, "nonexistent.txt", None);
        assert!(result.is_err());
    }
}
