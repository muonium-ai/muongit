//! High-level staging and commit workflows.
//! Parity: libgit2 index/commit porcelain APIs

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::commit::serialize_commit;
use crate::filter::{FilterList, FilterMode};
use crate::index::{read_index, write_index, Index, IndexEntry};
use crate::ignore::Ignore;
use crate::odb::write_loose_object;
use crate::pathspec::{Pathspec, PathspecFlags};
use crate::refs::{read_reference, resolve_reference, write_reference};
use crate::reflog::append_reflog;
use crate::repository::Repository;
use crate::revparse::read_commit;
use crate::tree::{self, serialize_tree, TreeEntry};
use crate::types::{ObjectType, Signature};
use crate::{MuonGitError, OID};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddOptions {
    pub include_ignored: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddResult {
    pub staged_paths: Vec<String>,
    pub removed_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveResult {
    pub removed_from_index: Vec<String>,
    pub removed_from_workdir: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnstageResult {
    pub restored_paths: Vec<String>,
    pub removed_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitOptions {
    pub author: Option<Signature>,
    pub committer: Option<Signature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResult {
    pub oid: OID,
    pub tree_id: OID,
    pub parent_ids: Vec<OID>,
    pub reference: String,
    pub summary: String,
}

pub fn add_paths(
    git_dir: &Path,
    workdir: &Path,
    patterns: &[&str],
    opts: &AddOptions,
) -> Result<AddResult, MuonGitError> {
    let mut index = read_index(git_dir)?;
    let mut candidates = collect_workdir_paths(git_dir, workdir, opts.include_ignored)?;
    candidates.extend(index.entries.iter().map(|entry| entry.path.clone()));
    let matched = match_patterns(&candidates, patterns)?;
    let mut result = AddResult::default();

    for path in matched {
        let full_path = workdir.join(&path);
        if full_path.is_file() {
            stage_path(git_dir, workdir, &mut index, &path)?;
            result.staged_paths.push(path);
        } else if index.remove(&path) {
            result.removed_paths.push(path);
        }
    }

    write_index(git_dir, &index)?;
    Ok(result)
}

pub fn remove_paths(
    git_dir: &Path,
    workdir: &Path,
    patterns: &[&str],
) -> Result<RemoveResult, MuonGitError> {
    let mut index = read_index(git_dir)?;
    let mut candidates = collect_workdir_paths(git_dir, workdir, true)?;
    candidates.extend(index.entries.iter().map(|entry| entry.path.clone()));
    let matched = match_patterns(&candidates, patterns)?;
    let mut result = RemoveResult::default();

    for path in matched {
        if index.remove(&path) {
            result.removed_from_index.push(path.clone());
        }

        let full_path = workdir.join(&path);
        if full_path.exists() {
            remove_workdir_path(workdir, &full_path)?;
            result.removed_from_workdir.push(path);
        }
    }

    write_index(git_dir, &index)?;
    Ok(result)
}

pub fn unstage_paths(git_dir: &Path, patterns: &[&str]) -> Result<UnstageResult, MuonGitError> {
    let mut index = read_index(git_dir)?;
    let head_entries = read_head_index_entries(git_dir)?;
    let mut candidates: BTreeSet<String> = index.entries.iter().map(|entry| entry.path.clone()).collect();
    candidates.extend(head_entries.keys().cloned());
    let matched = match_patterns(&candidates, patterns)?;
    let mut result = UnstageResult::default();

    for path in matched {
        if let Some(entry) = head_entries.get(&path) {
            index.add(entry.clone());
            result.restored_paths.push(path);
        } else if index.remove(&path) {
            result.removed_paths.push(path);
        }
    }

    write_index(git_dir, &index)?;
    Ok(result)
}

pub fn create_commit(
    git_dir: &Path,
    message: &str,
    opts: &CommitOptions,
) -> Result<CommitResult, MuonGitError> {
    let head_ref = current_head_ref(git_dir)?.ok_or_else(|| {
        MuonGitError::InvalidSpec("cannot commit on detached HEAD".into())
    })?;
    let parent = match resolve_reference(git_dir, "HEAD") {
        Ok(oid) => Some(oid),
        Err(MuonGitError::NotFound(_)) => None,
        Err(err) => return Err(err),
    };
    let index = read_index(git_dir)?;
    let tree_id = write_tree_from_index(git_dir, &index)?;
    let author = opts.author.clone().unwrap_or_else(default_signature);
    let committer = opts
        .committer
        .clone()
        .unwrap_or_else(|| author.clone());
    let normalized_message = normalize_commit_message(message);
    let summary = commit_summary(&normalized_message);
    let parent_ids: Vec<OID> = parent.iter().cloned().collect();
    let data = serialize_commit(
        &tree_id,
        &parent_ids,
        &author,
        &committer,
        &normalized_message,
        None,
    );
    let oid = write_loose_object(git_dir, ObjectType::Commit, &data)?;
    write_reference(git_dir, &head_ref, &oid)?;

    let old_oid = parent.unwrap_or_else(OID::zero);
    let reflog_message = if old_oid.is_zero() {
        format!("commit (initial): {}", summary)
    } else {
        format!("commit: {}", summary)
    };
    append_reflog(git_dir, &head_ref, &old_oid, &oid, &committer, &reflog_message)?;
    append_reflog(git_dir, "HEAD", &old_oid, &oid, &committer, &reflog_message)?;

    Ok(CommitResult {
        oid,
        tree_id,
        parent_ids,
        reference: head_ref,
        summary,
    })
}

impl Repository {
    pub fn add(&self, patterns: &[&str], opts: &AddOptions) -> Result<AddResult, MuonGitError> {
        let workdir = self.workdir().ok_or(MuonGitError::BareRepo)?;
        add_paths(self.git_dir(), workdir, patterns, opts)
    }

    pub fn remove(&self, patterns: &[&str]) -> Result<RemoveResult, MuonGitError> {
        let workdir = self.workdir().ok_or(MuonGitError::BareRepo)?;
        remove_paths(self.git_dir(), workdir, patterns)
    }

    pub fn unstage(&self, patterns: &[&str]) -> Result<UnstageResult, MuonGitError> {
        unstage_paths(self.git_dir(), patterns)
    }

    pub fn commit(&self, message: &str, opts: &CommitOptions) -> Result<CommitResult, MuonGitError> {
        create_commit(self.git_dir(), message, opts)
    }
}

fn match_patterns(candidates: &BTreeSet<String>, patterns: &[&str]) -> Result<Vec<String>, MuonGitError> {
    if candidates.is_empty() {
        return Err(MuonGitError::NotFound("no paths available".into()));
    }

    if patterns.is_empty() {
        return Ok(candidates.iter().cloned().collect());
    }

    let pathspec = Pathspec::new(patterns);
    let candidate_refs: Vec<&str> = candidates.iter().map(String::as_str).collect();
    let flags = PathspecFlags {
        find_failures: true,
        ..PathspecFlags::default()
    };
    let result = pathspec.match_paths(&candidate_refs, &flags);
    if !result.failures.is_empty() {
        return Err(MuonGitError::NotFound(format!(
            "pathspec did not match: {}",
            result.failures.join(", ")
        )));
    }

    Ok(result.matches)
}

fn collect_workdir_paths(
    git_dir: &Path,
    workdir: &Path,
    include_ignored: bool,
) -> Result<BTreeSet<String>, MuonGitError> {
    let ignore = Ignore::load(git_dir, workdir);
    let mut paths = BTreeSet::new();
    collect_workdir_paths_recursive(
        workdir,
        workdir,
        git_dir,
        include_ignored,
        &ignore,
        &mut paths,
    )?;
    Ok(paths)
}

fn collect_workdir_paths_recursive(
    dir: &Path,
    workdir: &Path,
    git_dir: &Path,
    include_ignored: bool,
    ignore: &Ignore,
    paths: &mut BTreeSet<String>,
) -> Result<(), MuonGitError> {
    let rel_dir = relative_path(dir, workdir)?;
    let mut scoped_ignore = ignore.clone();
    scoped_ignore.load_for_path(workdir, &rel_dir);

    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if path == git_dir || path.file_name().map(|name| name == ".git").unwrap_or(false) {
            continue;
        }

        let rel_path = relative_path(&path, workdir)?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !include_ignored && scoped_ignore.is_ignored(&rel_path, true) {
                continue;
            }
            collect_workdir_paths_recursive(
                &path,
                workdir,
                git_dir,
                include_ignored,
                &scoped_ignore,
                paths,
            )?;
        } else if file_type.is_file() {
            if !include_ignored && scoped_ignore.is_ignored(&rel_path, false) {
                continue;
            }
            paths.insert(rel_path);
        }
    }

    Ok(())
}

fn relative_path(path: &Path, workdir: &Path) -> Result<String, MuonGitError> {
    if path == workdir {
        return Ok(String::new());
    }

    let relative = path
        .strip_prefix(workdir)
        .map_err(|_| MuonGitError::Invalid("path is outside repository workdir".into()))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn stage_path(
    git_dir: &Path,
    workdir: &Path,
    index: &mut Index,
    path: &str,
) -> Result<(), MuonGitError> {
    let full_path = workdir.join(path);
    let metadata = fs::metadata(&full_path)?;
    let content = fs::read(&full_path)?;
    let filtered = FilterList::load(git_dir, Some(workdir), path, FilterMode::ToOdb, None).apply(&content);
    let oid = write_loose_object(git_dir, ObjectType::Blob, &filtered)?;
    let mode = file_mode_from_metadata(&metadata);

    index.add(IndexEntry {
        ctime_secs: 0,
        ctime_nanos: 0,
        mtime_secs: 0,
        mtime_nanos: 0,
        dev: 0,
        ino: 0,
        mode,
        uid: 0,
        gid: 0,
        file_size: metadata.len() as u32,
        oid,
        flags: path.len().min(0x0FFF) as u16,
        path: path.to_string(),
    });

    Ok(())
}

fn file_mode_from_metadata(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 != 0 {
            tree::file_mode::BLOB_EXE
        } else {
            tree::file_mode::BLOB
        }
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        tree::file_mode::BLOB
    }
}

fn read_head_index_entries(git_dir: &Path) -> Result<BTreeMap<String, IndexEntry>, MuonGitError> {
    let head_oid = match resolve_reference(git_dir, "HEAD") {
        Ok(oid) => oid,
        Err(MuonGitError::NotFound(_)) => return Ok(BTreeMap::new()),
        Err(err) => return Err(err),
    };
    let commit = read_commit(git_dir, &head_oid)?;
    let mut entries = BTreeMap::new();
    collect_head_tree_entries(git_dir, &commit.tree_id, "", &mut entries)?;
    Ok(entries)
}

fn collect_head_tree_entries(
    git_dir: &Path,
    tree_oid: &OID,
    prefix: &str,
    entries: &mut BTreeMap<String, IndexEntry>,
) -> Result<(), MuonGitError> {
    let tree = crate::object::read_object(git_dir, tree_oid)?.as_tree()?;
    for entry in tree.entries {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", prefix, entry.name)
        };

        if entry.mode == tree::file_mode::TREE {
            collect_head_tree_entries(git_dir, &entry.oid, &path, entries)?;
        } else {
            let blob = crate::blob::read_blob(git_dir, &entry.oid)?;
            entries.insert(
                path.clone(),
                IndexEntry {
                    ctime_secs: 0,
                    ctime_nanos: 0,
                    mtime_secs: 0,
                    mtime_nanos: 0,
                    dev: 0,
                    ino: 0,
                    mode: entry.mode,
                    uid: 0,
                    gid: 0,
                    file_size: blob.data.len() as u32,
                    oid: entry.oid,
                    flags: path.len().min(0x0FFF) as u16,
                    path,
                },
            );
        }
    }
    Ok(())
}

#[derive(Default)]
struct TreeNode {
    files: Vec<TreeEntry>,
    children: BTreeMap<String, TreeNode>,
}

fn write_tree_from_index(git_dir: &Path, index: &Index) -> Result<OID, MuonGitError> {
    let mut root = TreeNode::default();
    for entry in &index.entries {
        insert_tree_entry(&mut root, entry)?;
    }
    write_tree_node(git_dir, &root)
}

fn insert_tree_entry(node: &mut TreeNode, entry: &IndexEntry) -> Result<(), MuonGitError> {
    let mut parts = entry.path.split('/').peekable();
    insert_tree_entry_parts(node, entry, &mut parts)
}

fn insert_tree_entry_parts<'a, I>(
    node: &mut TreeNode,
    entry: &IndexEntry,
    parts: &mut std::iter::Peekable<I>,
) -> Result<(), MuonGitError>
where
    I: Iterator<Item = &'a str>,
{
    let Some(part) = parts.next() else {
        return Err(MuonGitError::Invalid("empty index path".into()));
    };

    if parts.peek().is_none() {
        node.files.push(TreeEntry {
            mode: entry.mode,
            name: part.to_string(),
            oid: entry.oid.clone(),
        });
        return Ok(());
    }

    let child = node.children.entry(part.to_string()).or_default();
    insert_tree_entry_parts(child, entry, parts)
}

fn write_tree_node(git_dir: &Path, node: &TreeNode) -> Result<OID, MuonGitError> {
    let mut entries = node.files.clone();
    for (name, child) in &node.children {
        let child_oid = write_tree_node(git_dir, child)?;
        entries.push(TreeEntry {
            mode: tree::file_mode::TREE,
            name: name.clone(),
            oid: child_oid,
        });
    }
    let data = serialize_tree(&entries);
    write_loose_object(git_dir, ObjectType::Tree, &data)
}

fn current_head_ref(git_dir: &Path) -> Result<Option<String>, MuonGitError> {
    let head = read_reference(git_dir, "HEAD")?;
    Ok(head
        .strip_prefix("ref: ")
        .map(|target| target.trim().to_string()))
}

fn normalize_commit_message(message: &str) -> String {
    if message.ends_with('\n') {
        message.to_string()
    } else {
        format!("{}\n", message)
    }
}

fn commit_summary(message: &str) -> String {
    message.lines().next().unwrap_or("").to_string()
}

fn default_signature() -> Signature {
    Signature {
        name: "MuonGit".into(),
        email: "muongit@example.invalid".into(),
        time: 0,
        offset: 0,
    }
}

fn remove_workdir_path(workdir: &Path, target: &Path) -> Result<(), MuonGitError> {
    if target.is_dir() {
        fs::remove_dir_all(target)?;
    } else {
        fs::remove_file(target)?;
    }
    prune_empty_parents(workdir, target.parent());
    Ok(())
}

fn prune_empty_parents(workdir: &Path, mut current: Option<&Path>) {
    while let Some(dir) = current {
        if dir == workdir {
            break;
        }
        let is_empty = match fs::read_dir(dir) {
            Ok(mut entries) => entries.next().is_none(),
            Err(_) => false,
        };
        if !is_empty {
            break;
        }
        let parent = dir.parent();
        let _ = fs::remove_dir(dir);
        current = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name)
    }

    fn write_workdir_file(workdir: &Path, path: &str, content: &str) {
        let full_path = workdir.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full_path, content).unwrap();
    }

    fn build_index(git_dir: &Path, files: &[(&str, &str)]) -> Index {
        let mut index = Index::new();
        for (path, content) in files {
            let oid = write_loose_object(git_dir, ObjectType::Blob, content.as_bytes()).unwrap();
            index.add(IndexEntry {
                ctime_secs: 0,
                ctime_nanos: 0,
                mtime_secs: 0,
                mtime_nanos: 0,
                dev: 0,
                ino: 0,
                mode: tree::file_mode::BLOB,
                uid: 0,
                gid: 0,
                file_size: content.len() as u32,
                oid,
                flags: path.len().min(0x0FFF) as u16,
                path: (*path).to_string(),
            });
        }
        index
    }

    fn write_commit_snapshot(
        repo: &Repository,
        files: &[(&str, &str)],
        parents: &[OID],
        message: &str,
        time: i64,
    ) -> OID {
        let index = build_index(repo.git_dir(), files);
        let tree_id = write_tree_from_index(repo.git_dir(), &index).unwrap();
        let signature = Signature {
            name: "Muon Test".into(),
            email: "test@muon.ai".into(),
            time,
            offset: 0,
        };
        let data = serialize_commit(&tree_id, parents, &signature, &signature, &format!("{}\n", message), None);
        write_loose_object(repo.git_dir(), ObjectType::Commit, &data).unwrap()
    }

    fn seed_head(repo: &Repository, files: &[(&str, &str)], message: &str) -> OID {
        let commit = write_commit_snapshot(repo, files, &[], message, 1);
        write_reference(repo.git_dir(), "refs/heads/main", &commit).unwrap();
        write_index(repo.git_dir(), &build_index(repo.git_dir(), files)).unwrap();
        for (path, content) in files {
            write_workdir_file(repo.workdir().unwrap(), path, content);
        }
        commit
    }

    #[test]
    fn test_add_stages_modified_and_untracked_pathspec_matches() {
        let tmp = test_dir("test_porcelain_add_pathspec");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let _base = seed_head(&repo, &[("src/one.txt", "base\n"), ("notes.md", "keep\n")], "base");

        write_workdir_file(repo.workdir().unwrap(), "src/one.txt", "changed\n");
        write_workdir_file(repo.workdir().unwrap(), "src/two.txt", "new\n");
        write_workdir_file(repo.workdir().unwrap(), "docs/readme.md", "skip\n");

        let result = repo
            .add(&["src/*.txt"], &AddOptions::default())
            .unwrap();

        assert_eq!(result.staged_paths, vec!["src/one.txt".to_string(), "src/two.txt".to_string()]);
        assert!(result.removed_paths.is_empty());

        let index = read_index(repo.git_dir()).unwrap();
        let one = index.find("src/one.txt").unwrap();
        let two = index.find("src/two.txt").unwrap();
        let docs = index.find("docs/readme.md");

        assert_eq!(crate::blob::read_blob(repo.git_dir(), &one.oid).unwrap().data, b"changed\n");
        assert_eq!(crate::blob::read_blob(repo.git_dir(), &two.oid).unwrap().data, b"new\n");
        assert!(docs.is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_remove_deletes_tracked_paths_from_index_and_workdir() {
        let tmp = test_dir("test_porcelain_remove");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let _base = seed_head(&repo, &[("tracked.txt", "tracked\n"), ("keep.txt", "keep\n")], "base");

        let result = repo.remove(&["tracked.txt"]).unwrap();

        assert_eq!(result.removed_from_index, vec!["tracked.txt".to_string()]);
        assert_eq!(result.removed_from_workdir, vec!["tracked.txt".to_string()]);
        assert!(read_index(repo.git_dir()).unwrap().find("tracked.txt").is_none());
        assert!(!repo.workdir().unwrap().join("tracked.txt").exists());
        assert!(repo.workdir().unwrap().join("keep.txt").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_unstage_restores_head_entries_and_drops_new_paths() {
        let tmp = test_dir("test_porcelain_unstage");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let _base = seed_head(&repo, &[("tracked.txt", "base\n")], "base");

        write_workdir_file(repo.workdir().unwrap(), "tracked.txt", "staged\n");
        write_workdir_file(repo.workdir().unwrap(), "new.txt", "new\n");
        repo.add(&["tracked.txt", "new.txt"], &AddOptions::default()).unwrap();

        let result = repo.unstage(&["tracked.txt", "new.txt"]).unwrap();

        assert_eq!(result.restored_paths, vec!["tracked.txt".to_string()]);
        assert_eq!(result.removed_paths, vec!["new.txt".to_string()]);

        let index = read_index(repo.git_dir()).unwrap();
        let tracked = index.find("tracked.txt").unwrap();
        assert_eq!(crate::blob::read_blob(repo.git_dir(), &tracked.oid).unwrap().data, b"base\n");
        assert!(index.find("new.txt").is_none());
        assert!(repo.workdir().unwrap().join("new.txt").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_unstage_on_unborn_branch_removes_new_entries() {
        let tmp = test_dir("test_porcelain_unstage_unborn");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        write_workdir_file(repo.workdir().unwrap(), "new.txt", "new\n");
        repo.add(&["new.txt"], &AddOptions::default()).unwrap();

        let result = repo.unstage(&["new.txt"]).unwrap();

        assert!(result.restored_paths.is_empty());
        assert_eq!(result.removed_paths, vec!["new.txt".to_string()]);
        assert!(read_index(repo.git_dir()).unwrap().entries.is_empty());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_commit_updates_branch_and_reflogs() {
        let tmp = test_dir("test_porcelain_commit");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let base = seed_head(
            &repo,
            &[("tracked.txt", "base\n"), ("remove.txt", "remove me\n")],
            "base",
        );

        write_workdir_file(repo.workdir().unwrap(), "tracked.txt", "changed\n");
        write_workdir_file(repo.workdir().unwrap(), "new.txt", "new\n");
        repo.add(&["tracked.txt", "new.txt"], &AddOptions::default()).unwrap();
        repo.remove(&["remove.txt"]).unwrap();

        let result = repo.commit("second", &CommitOptions::default()).unwrap();

        assert_eq!(result.reference, "refs/heads/main");
        assert_eq!(result.parent_ids, vec![base.clone()]);
        assert_eq!(result.summary, "second");
        assert_eq!(resolve_reference(repo.git_dir(), "HEAD").unwrap(), result.oid);
        assert_eq!(resolve_reference(repo.git_dir(), "refs/heads/main").unwrap(), result.oid);

        let commit = read_commit(repo.git_dir(), &result.oid).unwrap();
        assert_eq!(commit.parent_ids, vec![base]);
        let tree = crate::object::read_object(repo.git_dir(), &commit.tree_id).unwrap().as_tree().unwrap();
        assert_eq!(tree.entries.len(), 2);

        let head_log = crate::reflog::read_reflog(repo.git_dir(), "HEAD").unwrap();
        let branch_log = crate::reflog::read_reflog(repo.git_dir(), "refs/heads/main").unwrap();
        assert_eq!(head_log.last().unwrap().message, "commit: second");
        assert_eq!(branch_log.last().unwrap().message, "commit: second");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_commit_rejects_detached_head() {
        let tmp = test_dir("test_porcelain_commit_detached");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let base = seed_head(&repo, &[("tracked.txt", "base\n")], "base");
        write_reference(repo.git_dir(), "HEAD", &base).unwrap();

        let result = repo.commit("detached", &CommitOptions::default());
        assert!(matches!(result, Err(MuonGitError::InvalidSpec(_))));

        let _ = fs::remove_dir_all(&tmp);
    }
}
