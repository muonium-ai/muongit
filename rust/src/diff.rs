//! Tree-to-tree, index-to-workdir diff and diff formatting
//! Parity: libgit2 src/libgit2/diff.c, diff_print.c

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::error::MuonGitError;
use crate::index::{read_index, IndexEntry};
use crate::oid::OID;
use crate::tree::{file_mode, TreeEntry};

/// The kind of change for a diff entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Added,
    Deleted,
    Modified,
}

/// A single diff delta between two trees
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffDelta {
    pub status: DiffStatus,
    pub old_entry: Option<TreeEntry>,
    pub new_entry: Option<TreeEntry>,
    pub path: String,
}

/// Compute the diff between two trees.
/// Both entry lists should be sorted by name (as git trees are).
pub fn diff_trees(old_entries: &[TreeEntry], new_entries: &[TreeEntry]) -> Vec<DiffDelta> {
    let mut deltas = Vec::new();
    let mut oi = 0;
    let mut ni = 0;

    while oi < old_entries.len() && ni < new_entries.len() {
        let old = &old_entries[oi];
        let new = &new_entries[ni];

        match old.name.cmp(&new.name) {
            std::cmp::Ordering::Less => {
                // Entry only in old tree — deleted
                deltas.push(DiffDelta {
                    status: DiffStatus::Deleted,
                    old_entry: Some(old.clone()),
                    new_entry: None,
                    path: old.name.clone(),
                });
                oi += 1;
            }
            std::cmp::Ordering::Greater => {
                // Entry only in new tree — added
                deltas.push(DiffDelta {
                    status: DiffStatus::Added,
                    old_entry: None,
                    new_entry: Some(new.clone()),
                    path: new.name.clone(),
                });
                ni += 1;
            }
            std::cmp::Ordering::Equal => {
                // Same name — check if modified
                if old.oid != new.oid || old.mode != new.mode {
                    deltas.push(DiffDelta {
                        status: DiffStatus::Modified,
                        old_entry: Some(old.clone()),
                        new_entry: Some(new.clone()),
                        path: old.name.clone(),
                    });
                }
                oi += 1;
                ni += 1;
            }
        }
    }

    // Remaining old entries are deletions
    while oi < old_entries.len() {
        let old = &old_entries[oi];
        deltas.push(DiffDelta {
            status: DiffStatus::Deleted,
            old_entry: Some(old.clone()),
            new_entry: None,
            path: old.name.clone(),
        });
        oi += 1;
    }

    // Remaining new entries are additions
    while ni < new_entries.len() {
        let new = &new_entries[ni];
        deltas.push(DiffDelta {
            status: DiffStatus::Added,
            old_entry: None,
            new_entry: Some(new.clone()),
            path: new.name.clone(),
        });
        ni += 1;
    }

    deltas
}

/// Compute the diff between the index (staging area) and the working directory.
/// Returns deltas for modified, deleted, and new (untracked) files.
pub fn diff_index_to_workdir(git_dir: &Path, workdir: &Path) -> Result<Vec<DiffDelta>, MuonGitError> {
    let index = read_index(git_dir)?;
    let mut deltas = Vec::new();

    let indexed_paths: BTreeSet<&str> = index.entries.iter().map(|e| e.path.as_str()).collect();

    // Check each index entry against the working directory
    for entry in &index.entries {
        let file_path = workdir.join(&entry.path);
        if !file_path.exists() {
            deltas.push(DiffDelta {
                status: DiffStatus::Deleted,
                old_entry: Some(index_entry_to_tree_entry(entry)),
                new_entry: None,
                path: entry.path.clone(),
            });
        } else {
            let metadata = fs::metadata(&file_path)?;
            let file_size = metadata.len() as u32;

            // Quick size check, then content hash
            let modified = if file_size != entry.file_size {
                true
            } else {
                let content = fs::read(&file_path)?;
                let oid = OID::hash_object(crate::ObjectType::Blob, &content);
                oid != entry.oid
            };

            if modified {
                let content = fs::read(&file_path)?;
                let workdir_oid = OID::hash_object(crate::ObjectType::Blob, &content);
                let workdir_mode = if is_executable(&file_path) {
                    file_mode::BLOB_EXE
                } else {
                    file_mode::BLOB
                };
                deltas.push(DiffDelta {
                    status: DiffStatus::Modified,
                    old_entry: Some(index_entry_to_tree_entry(entry)),
                    new_entry: Some(TreeEntry {
                        mode: workdir_mode,
                        name: entry.path.clone(),
                        oid: workdir_oid,
                    }),
                    path: entry.path.clone(),
                });
            }
        }
    }

    // Find new (untracked) files
    let mut new_files = Vec::new();
    collect_workdir_files(workdir, workdir, git_dir, &indexed_paths, &mut new_files)?;
    new_files.sort();

    for rel_path in new_files {
        let file_path = workdir.join(&rel_path);
        let content = fs::read(&file_path)?;
        let oid = OID::hash_object(crate::ObjectType::Blob, &content);
        let mode = if is_executable(&file_path) {
            file_mode::BLOB_EXE
        } else {
            file_mode::BLOB
        };
        deltas.push(DiffDelta {
            status: DiffStatus::Added,
            old_entry: None,
            new_entry: Some(TreeEntry {
                mode,
                name: rel_path.clone(),
                oid,
            }),
            path: rel_path,
        });
    }

    Ok(deltas)
}

/// Convert an IndexEntry to a TreeEntry for diff results.
fn index_entry_to_tree_entry(entry: &IndexEntry) -> TreeEntry {
    TreeEntry {
        mode: entry.mode,
        name: entry.path.clone(),
        oid: entry.oid.clone(),
    }
}

/// Check if a file is executable.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// Recursively collect untracked files in the working directory.
fn collect_workdir_files(
    dir: &Path,
    workdir: &Path,
    git_dir: &Path,
    indexed: &BTreeSet<&str>,
    result: &mut Vec<String>,
) -> Result<(), MuonGitError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Skip .git directory
        if path == git_dir || path.file_name().map(|n| n == ".git").unwrap_or(false) {
            continue;
        }

        if path.is_dir() {
            collect_workdir_files(&path, workdir, git_dir, indexed, result)?;
        } else {
            let relative = path.strip_prefix(workdir)
                .map_err(|_| MuonGitError::InvalidObject("path prefix error".into()))?;
            let rel_str = relative.to_string_lossy().to_string();
            if !indexed.contains(rel_str.as_str()) {
                result.push(rel_str);
            }
        }
    }

    Ok(())
}

// --- Diff formatting (patch and stat) ---

/// A single edit operation in a line-level diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditKind {
    Equal,
    Insert,
    Delete,
}

/// A line-level edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub kind: EditKind,
    pub old_line: usize, // 1-based, 0 if insert
    pub new_line: usize, // 1-based, 0 if delete
    pub text: String,
}

/// A unified diff hunk.
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub edits: Vec<Edit>,
}

/// Compute a Myers-style line diff between two texts.
/// Returns a list of edits (equal, insert, delete).
pub fn diff_lines(old_text: &str, new_text: &str) -> Vec<Edit> {
    let old_lines: Vec<&str> = if old_text.is_empty() {
        Vec::new()
    } else {
        old_text.split('\n').collect()
    };
    let new_lines: Vec<&str> = if new_text.is_empty() {
        Vec::new()
    } else {
        new_text.split('\n').collect()
    };

    // Compute LCS using classic DP
    let n = old_lines.len();
    let m = new_lines.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];

    for i in 1..=n {
        for j in 1..=m {
            if old_lines[i - 1] == new_lines[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = std::cmp::max(dp[i - 1][j], dp[i][j - 1]);
            }
        }
    }

    // Backtrack to produce edits
    let mut edits = Vec::new();
    let mut i = n;
    let mut j = m;

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_lines[i - 1] == new_lines[j - 1] {
            edits.push(Edit {
                kind: EditKind::Equal,
                old_line: i,
                new_line: j,
                text: old_lines[i - 1].to_string(),
            });
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            edits.push(Edit {
                kind: EditKind::Insert,
                old_line: 0,
                new_line: j,
                text: new_lines[j - 1].to_string(),
            });
            j -= 1;
        } else {
            edits.push(Edit {
                kind: EditKind::Delete,
                old_line: i,
                new_line: 0,
                text: old_lines[i - 1].to_string(),
            });
            i -= 1;
        }
    }

    edits.reverse();
    edits
}

/// Group edits into unified diff hunks with the given context lines (default 3).
pub fn make_hunks(edits: &[Edit], context: usize) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let change_indices: Vec<usize> = edits
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind != EditKind::Equal)
        .map(|(i, _)| i)
        .collect();

    if change_indices.is_empty() {
        return hunks;
    }

    let mut groups: Vec<(usize, usize)> = Vec::new(); // (first_change_idx, last_change_idx)

    let mut ci = 0;
    while ci < change_indices.len() {
        let start = change_indices[ci];
        let mut end = start;
        while ci + 1 < change_indices.len()
            && change_indices[ci + 1] <= end + 2 * context + 1
        {
            ci += 1;
            end = change_indices[ci];
        }
        groups.push((start, end));
        ci += 1;
    }

    for (first_change, last_change) in groups {
        let hunk_start = if first_change > context {
            first_change - context
        } else {
            0
        };
        let hunk_end = std::cmp::min(last_change + context + 1, edits.len());

        let hunk_edits: Vec<Edit> = edits[hunk_start..hunk_end].to_vec();

        // Compute old/new line ranges
        let mut old_start = 0;
        let mut new_start = 0;
        let mut old_count = 0;
        let mut new_count = 0;

        for (i, edit) in hunk_edits.iter().enumerate() {
            if i == 0 {
                match edit.kind {
                    EditKind::Equal | EditKind::Delete => old_start = edit.old_line,
                    EditKind::Insert => {
                        old_start = edit.new_line; // approximation
                        // find first old_line in this hunk
                        for e in &hunk_edits {
                            if e.old_line > 0 {
                                old_start = e.old_line;
                                break;
                            }
                        }
                    }
                }
                match edit.kind {
                    EditKind::Equal | EditKind::Insert => new_start = edit.new_line,
                    EditKind::Delete => {
                        new_start = edit.old_line;
                        for e in &hunk_edits {
                            if e.new_line > 0 {
                                new_start = e.new_line;
                                break;
                            }
                        }
                    }
                }
            }

            match edit.kind {
                EditKind::Equal => {
                    old_count += 1;
                    new_count += 1;
                }
                EditKind::Delete => old_count += 1,
                EditKind::Insert => new_count += 1,
            }
        }

        hunks.push(DiffHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            edits: hunk_edits,
        });
    }

    hunks
}

/// Format a diff as a unified patch string.
/// `old_path` and `new_path` are the file paths for the `---`/`+++` header lines.
pub fn format_patch(old_path: &str, new_path: &str, old_text: &str, new_text: &str, context: usize) -> String {
    let edits = diff_lines(old_text, new_text);
    let hunks = make_hunks(&edits, context);

    if hunks.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str(&format!("--- a/{}\n", old_path));
    out.push_str(&format!("+++ b/{}\n", new_path));

    for hunk in &hunks {
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        ));
        for edit in &hunk.edits {
            match edit.kind {
                EditKind::Equal => {
                    out.push(' ');
                    out.push_str(&edit.text);
                    out.push('\n');
                }
                EditKind::Delete => {
                    out.push('-');
                    out.push_str(&edit.text);
                    out.push('\n');
                }
                EditKind::Insert => {
                    out.push('+');
                    out.push_str(&edit.text);
                    out.push('\n');
                }
            }
        }
    }

    out
}

/// A stat entry for a single file.
#[derive(Debug, Clone)]
pub struct DiffStatEntry {
    pub path: String,
    pub insertions: usize,
    pub deletions: usize,
}

/// Compute diff stats for a single file.
pub fn diff_stat(path: &str, old_text: &str, new_text: &str) -> DiffStatEntry {
    let edits = diff_lines(old_text, new_text);
    let insertions = edits.iter().filter(|e| e.kind == EditKind::Insert).count();
    let deletions = edits.iter().filter(|e| e.kind == EditKind::Delete).count();
    DiffStatEntry {
        path: path.to_string(),
        insertions,
        deletions,
    }
}

/// Format stat entries as a diffstat string (like `git diff --stat`).
pub fn format_stat(stats: &[DiffStatEntry]) -> String {
    if stats.is_empty() {
        return String::new();
    }

    let max_path_len = stats.iter().map(|s| s.path.len()).max().unwrap_or(0);
    let max_changes = stats.iter().map(|s| s.insertions + s.deletions).max().unwrap_or(0);
    let bar_width = 40usize;

    let mut out = String::new();
    let mut total_insertions = 0;
    let mut total_deletions = 0;

    for stat in stats {
        let changes = stat.insertions + stat.deletions;
        total_insertions += stat.insertions;
        total_deletions += stat.deletions;

        let (plus_count, minus_count) = if max_changes > 0 && changes > 0 {
            let total_bars = std::cmp::min(changes, bar_width);
            let plus_bars = if changes > 0 {
                (stat.insertions as f64 / changes as f64 * total_bars as f64).round() as usize
            } else {
                0
            };
            let minus_bars = total_bars - plus_bars;
            (plus_bars, minus_bars)
        } else {
            (0, 0)
        };

        out.push_str(&format!(
            " {:width$} | {:>5} {}{}\n",
            stat.path,
            changes,
            "+".repeat(plus_count),
            "-".repeat(minus_count),
            width = max_path_len,
        ));
    }

    let file_word = if stats.len() == 1 { "file" } else { "files" };
    out.push_str(&format!(
        " {} {} changed, {} insertions(+), {} deletions(-)\n",
        stats.len(),
        file_word,
        total_insertions,
        total_deletions,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oid::OID;
    use crate::tree::file_mode;

    fn entry(name: &str, oid_hex: &str, mode: u32) -> TreeEntry {
        TreeEntry {
            mode,
            name: name.to_string(),
            oid: OID::from_hex(oid_hex).unwrap(),
        }
    }

    #[test]
    fn test_diff_identical_trees() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let entries = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let deltas = diff_trees(&entries, &entries);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_diff_added_file() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![entry("a.txt", oid, file_mode::BLOB)];
        let new = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Added);
        assert_eq!(deltas[0].path, "b.txt");
        assert!(deltas[0].old_entry.is_none());
        assert!(deltas[0].new_entry.is_some());
    }

    #[test]
    fn test_diff_deleted_file() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let new = vec![entry("a.txt", oid, file_mode::BLOB)];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Deleted);
        assert_eq!(deltas[0].path, "b.txt");
        assert!(deltas[0].old_entry.is_some());
        assert!(deltas[0].new_entry.is_none());
    }

    #[test]
    fn test_diff_modified_file() {
        let oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![entry("a.txt", oid1, file_mode::BLOB)];
        let new = vec![entry("a.txt", oid2, file_mode::BLOB)];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Modified);
        assert_eq!(deltas[0].path, "a.txt");
        assert!(deltas[0].old_entry.is_some());
        assert!(deltas[0].new_entry.is_some());
    }

    #[test]
    fn test_diff_mode_change() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![entry("script.sh", oid, file_mode::BLOB)];
        let new = vec![entry("script.sh", oid, file_mode::BLOB_EXE)];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Modified);
    }

    #[test]
    fn test_diff_empty_to_full() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let new = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let deltas = diff_trees(&[], &new);
        assert_eq!(deltas.len(), 2);
        assert!(deltas.iter().all(|d| d.status == DiffStatus::Added));
    }

    #[test]
    fn test_diff_full_to_empty() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let deltas = diff_trees(&old, &[]);
        assert_eq!(deltas.len(), 2);
        assert!(deltas.iter().all(|d| d.status == DiffStatus::Deleted));
    }

    #[test]
    fn test_diff_mixed_changes() {
        let oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![
            entry("a.txt", oid1, file_mode::BLOB),
            entry("b.txt", oid1, file_mode::BLOB),
            entry("c.txt", oid1, file_mode::BLOB),
        ];
        let new = vec![
            entry("a.txt", oid1, file_mode::BLOB), // unchanged
            entry("b.txt", oid2, file_mode::BLOB), // modified
            entry("d.txt", oid1, file_mode::BLOB), // added
        ];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 3);
        assert_eq!(deltas[0].status, DiffStatus::Modified);
        assert_eq!(deltas[0].path, "b.txt");
        assert_eq!(deltas[1].status, DiffStatus::Deleted);
        assert_eq!(deltas[1].path, "c.txt");
        assert_eq!(deltas[2].status, DiffStatus::Added);
        assert_eq!(deltas[2].path, "d.txt");
    }

    // --- Diff formatting tests ---

    #[test]
    fn test_diff_lines_identical() {
        let edits = diff_lines("a\nb\nc\n", "a\nb\nc\n");
        assert!(edits.iter().all(|e| e.kind == EditKind::Equal));
    }

    #[test]
    fn test_diff_lines_insert() {
        let edits = diff_lines("a\nc\n", "a\nb\nc\n");
        let inserts: Vec<_> = edits.iter().filter(|e| e.kind == EditKind::Insert).collect();
        assert_eq!(inserts.len(), 1);
        assert_eq!(inserts[0].text, "b");
    }

    #[test]
    fn test_diff_lines_delete() {
        let edits = diff_lines("a\nb\nc\n", "a\nc\n");
        let deletes: Vec<_> = edits.iter().filter(|e| e.kind == EditKind::Delete).collect();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].text, "b");
    }

    #[test]
    fn test_diff_lines_modify() {
        let edits = diff_lines("a\nb\nc\n", "a\nB\nc\n");
        let deletes: Vec<_> = edits.iter().filter(|e| e.kind == EditKind::Delete).collect();
        let inserts: Vec<_> = edits.iter().filter(|e| e.kind == EditKind::Insert).collect();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].text, "b");
        assert_eq!(inserts.len(), 1);
        assert_eq!(inserts[0].text, "B");
    }

    #[test]
    fn test_format_patch_basic() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nmodified\nline3\n";
        let patch = format_patch("file.txt", "file.txt", old, new, 3);
        assert!(patch.contains("--- a/file.txt"));
        assert!(patch.contains("+++ b/file.txt"));
        assert!(patch.contains("@@"));
        assert!(patch.contains("-line2"));
        assert!(patch.contains("+modified"));
    }

    #[test]
    fn test_format_patch_no_changes() {
        let text = "same\n";
        let patch = format_patch("f.txt", "f.txt", text, text, 3);
        assert!(patch.is_empty());
    }

    #[test]
    fn test_format_patch_added_file() {
        let patch = format_patch("new.txt", "new.txt", "", "hello\nworld\n", 3);
        assert!(patch.contains("+hello"));
        assert!(patch.contains("+world"));
    }

    #[test]
    fn test_format_patch_deleted_file() {
        let patch = format_patch("old.txt", "old.txt", "goodbye\nworld\n", "", 3);
        assert!(patch.contains("-goodbye"));
        assert!(patch.contains("-world"));
    }

    #[test]
    fn test_diff_stat_basic() {
        let stat = diff_stat("file.txt", "a\nb\nc\n", "a\nB\nc\nd\n");
        assert_eq!(stat.path, "file.txt");
        assert_eq!(stat.deletions, 1); // "b" deleted
        assert_eq!(stat.insertions, 2); // "B" and "d" inserted
    }

    #[test]
    fn test_format_stat_output() {
        let stats = vec![
            DiffStatEntry { path: "file.txt".into(), insertions: 3, deletions: 1 },
            DiffStatEntry { path: "other.rs".into(), insertions: 0, deletions: 5 },
        ];
        let output = format_stat(&stats);
        assert!(output.contains("file.txt"));
        assert!(output.contains("other.rs"));
        assert!(output.contains("2 files changed"));
        assert!(output.contains("3 insertions(+)"));
        assert!(output.contains("6 deletions(-)"));
    }

    #[test]
    fn test_format_stat_empty() {
        let output = format_stat(&[]);
        assert!(output.is_empty());
    }

    // --- Index-to-workdir diff tests ---

    use crate::index::{Index, IndexEntry, write_index};

    fn make_index_entry(path: &str, oid: &OID, file_size: u32) -> IndexEntry {
        IndexEntry {
            ctime_secs: 0, ctime_nanos: 0,
            mtime_secs: 0, mtime_nanos: 0,
            dev: 0, ino: 0,
            mode: 0o100644, uid: 0, gid: 0,
            file_size,
            oid: oid.clone(),
            flags: 0,
            path: path.to_string(),
        }
    }

    #[test]
    fn test_diff_workdir_clean() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_clean");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"hello\n";
        let file_path = repo.workdir().unwrap().join("hello.txt");
        std::fs::write(&file_path, content).unwrap();

        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        let deltas = diff_index_to_workdir(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert!(deltas.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_diff_workdir_modified() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_mod");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"hello\n";
        let file_path = repo.workdir().unwrap().join("hello.txt");
        std::fs::write(&file_path, content).unwrap();

        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        // Modify the file
        std::fs::write(&file_path, b"changed\n").unwrap();

        let deltas = diff_index_to_workdir(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Modified);
        assert_eq!(deltas[0].path, "hello.txt");
        assert!(deltas[0].old_entry.is_some());
        assert!(deltas[0].new_entry.is_some());
        // New entry OID should differ from old
        assert_ne!(deltas[0].old_entry.as_ref().unwrap().oid, deltas[0].new_entry.as_ref().unwrap().oid);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_diff_workdir_deleted() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_del");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"hello\n";
        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        // Don't create the file — it's deleted
        let deltas = diff_index_to_workdir(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Deleted);
        assert_eq!(deltas[0].path, "hello.txt");
        assert!(deltas[0].old_entry.is_some());
        assert!(deltas[0].new_entry.is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_diff_workdir_new_file() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_new");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        // Empty index
        let index = Index::new();
        write_index(repo.git_dir(), &index).unwrap();

        // Create a file not in the index
        std::fs::write(repo.workdir().unwrap().join("new.txt"), b"new\n").unwrap();

        let deltas = diff_index_to_workdir(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Added);
        assert_eq!(deltas[0].path, "new.txt");
        assert!(deltas[0].old_entry.is_none());
        assert!(deltas[0].new_entry.is_some());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_diff_workdir_mixed() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_mixed");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content_a = b"aaa\n";
        let content_b = b"bbb\n";
        let oid_a = OID::hash_object(crate::ObjectType::Blob, content_a);
        let oid_b = OID::hash_object(crate::ObjectType::Blob, content_b);

        let mut index = Index::new();
        index.add(make_index_entry("a.txt", &oid_a, content_a.len() as u32));
        index.add(make_index_entry("b.txt", &oid_b, content_b.len() as u32));
        index.add(make_index_entry("c.txt", &oid_a, content_a.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        let wd = repo.workdir().unwrap();
        // a.txt: unchanged
        std::fs::write(wd.join("a.txt"), content_a).unwrap();
        // b.txt: modified
        std::fs::write(wd.join("b.txt"), b"modified\n").unwrap();
        // c.txt: deleted (not created)
        // d.txt: new
        std::fs::write(wd.join("d.txt"), b"new\n").unwrap();

        let deltas = diff_index_to_workdir(repo.git_dir(), wd).unwrap();

        let modified: Vec<_> = deltas.iter().filter(|d| d.status == DiffStatus::Modified).collect();
        let deleted: Vec<_> = deltas.iter().filter(|d| d.status == DiffStatus::Deleted).collect();
        let added: Vec<_> = deltas.iter().filter(|d| d.status == DiffStatus::Added).collect();

        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].path, "b.txt");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].path, "c.txt");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].path, "d.txt");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
