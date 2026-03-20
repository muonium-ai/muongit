//! Checkout - materialize index entries into the working directory
//! Parity: libgit2 src/libgit2/checkout.c

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::branch::{lookup_branch, BranchType};
use crate::blob::read_blob;
use crate::error::MuonGitError;
use crate::index::{read_index, write_index, Index, IndexEntry};
use crate::object::read_object;
use crate::reflog::append_reflog;
use crate::refs::{read_reference, resolve_reference, write_reference, write_symbolic_reference};
use crate::repository::Repository;
use crate::revparse::{read_commit, resolve_revision};
use crate::tree;
use crate::types::Signature;
use crate::OID;

/// Options for checkout behavior.
#[derive(Debug, Clone, Default)]
pub struct CheckoutOptions {
    /// If true, overwrite existing files in the workdir.
    pub force: bool,
}

/// Result of a checkout operation.
#[derive(Debug, Clone, Default)]
pub struct CheckoutResult {
    /// Files written to the workdir.
    pub updated: Vec<String>,
    /// Files skipped because they already exist (when force is false).
    pub conflicts: Vec<String>,
}

/// Checkout the index to the working directory.
///
/// Reads the index, then for each entry reads the blob from the ODB
/// and writes it to the workdir at the entry's path.
pub fn checkout_index(
    git_dir: &Path,
    workdir: &Path,
    opts: &CheckoutOptions,
) -> Result<CheckoutResult, MuonGitError> {
    let index = read_index(git_dir)?;
    let mut result = CheckoutResult::default();

    for entry in &index.entries {
        checkout_entry(git_dir, workdir, entry, opts, &mut result)?;
    }

    Ok(result)
}

/// Checkout a single index entry to the working directory.
fn checkout_entry(
    git_dir: &Path,
    workdir: &Path,
    entry: &IndexEntry,
    opts: &CheckoutOptions,
    result: &mut CheckoutResult,
) -> Result<(), MuonGitError> {
    let target_path = workdir.join(&entry.path);

    // Check for existing file when not forcing
    if !opts.force && target_path.exists() {
        result.conflicts.push(entry.path.clone());
        return Ok(());
    }

    // Create parent directories
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read blob content
    let blob = read_blob(git_dir, &entry.oid)?;

    // Write file
    fs::write(&target_path, &blob.data)?;

    // Set file permissions based on mode
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = entry.mode & 0o777;
        // Git stores executable as 0o100755, regular as 0o100644
        // Extract just the permission bits
        let perms = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
        fs::set_permissions(&target_path, fs::Permissions::from_mode(perms))?;
    }

    result.updated.push(entry.path.clone());
    Ok(())
}

/// Checkout specific paths from the index to the working directory.
pub fn checkout_paths(
    git_dir: &Path,
    workdir: &Path,
    paths: &[&str],
    opts: &CheckoutOptions,
) -> Result<CheckoutResult, MuonGitError> {
    let index = read_index(git_dir)?;
    let mut result = CheckoutResult::default();

    for path in paths {
        if let Some(entry) = index.find(path) {
            let entry = entry.clone();
            checkout_entry(git_dir, workdir, &entry, opts, &mut result)?;
        } else {
            return Err(MuonGitError::NotFound(format!(
                "path '{}' not in index",
                path
            )));
        }
    }

    Ok(result)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SwitchOptions {
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchResult {
    pub previous_head: Option<OID>,
    pub head_oid: OID,
    pub head_ref: Option<String>,
    pub updated_paths: Vec<String>,
    pub removed_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetResult {
    pub previous_head: OID,
    pub head_oid: OID,
    pub moved_ref: Option<String>,
    pub updated_paths: Vec<String>,
    pub removed_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOptions {
    pub source: Option<String>,
    pub staged: bool,
    pub worktree: bool,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            source: None,
            staged: false,
            worktree: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreResult {
    pub staged_paths: Vec<String>,
    pub removed_from_index: Vec<String>,
    pub restored_paths: Vec<String>,
    pub removed_from_workdir: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WorkdirUpdate {
    updated_paths: Vec<String>,
    removed_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MaterializedEntry {
    oid: OID,
    mode: u32,
    data: Vec<u8>,
}

pub fn switch_branch(
    git_dir: &Path,
    workdir: &Path,
    name: &str,
    opts: &SwitchOptions,
) -> Result<SwitchResult, MuonGitError> {
    let branch = lookup_branch(git_dir, name, BranchType::Local)?;
    let target_oid = branch.target.clone().ok_or_else(|| {
        MuonGitError::InvalidSpec(format!("branch '{}' has no target commit", name))
    })?;
    let current_index = read_index(git_dir)?;
    let previous_head = current_head_oid(git_dir).ok();
    let current_desc = describe_head(git_dir)?;
    let target_entries = materialize_commit_tree(git_dir, &target_oid)?;

    if !opts.force {
        let conflicts = collect_switch_conflicts(git_dir, workdir, &current_index, &target_entries)?;
        if !conflicts.is_empty() {
            return Err(MuonGitError::Conflict(format!(
                "checkout would overwrite local changes: {}",
                conflicts.join(", ")
            )));
        }
    }

    let target_index = index_from_materialized(&target_entries);
    write_index(git_dir, &target_index)?;
    let update = apply_workdir_tree(workdir, &current_index, &target_entries)?;
    write_symbolic_reference(git_dir, "HEAD", &branch.reference_name)?;

    let sig = default_signature();
    let old_oid = previous_head.as_ref().unwrap_or(&OID::zero()).clone();
    append_reflog(
        git_dir,
        "HEAD",
        &old_oid,
        &target_oid,
        &sig,
        &format!("checkout: moving from {} to {}", current_desc, name),
    )?;

    Ok(SwitchResult {
        previous_head,
        head_oid: target_oid,
        head_ref: Some(branch.reference_name),
        updated_paths: update.updated_paths,
        removed_paths: update.removed_paths,
    })
}

pub fn checkout_revision(
    git_dir: &Path,
    workdir: &Path,
    spec: &str,
    opts: &SwitchOptions,
) -> Result<SwitchResult, MuonGitError> {
    let target_oid = resolve_revision(git_dir, spec)?;
    let current_index = read_index(git_dir)?;
    let previous_head = current_head_oid(git_dir).ok();
    let current_desc = describe_head(git_dir)?;
    let target_entries = materialize_commit_tree(git_dir, &target_oid)?;

    if !opts.force {
        let conflicts = collect_switch_conflicts(git_dir, workdir, &current_index, &target_entries)?;
        if !conflicts.is_empty() {
            return Err(MuonGitError::Conflict(format!(
                "checkout would overwrite local changes: {}",
                conflicts.join(", ")
            )));
        }
    }

    let target_index = index_from_materialized(&target_entries);
    write_index(git_dir, &target_index)?;
    let update = apply_workdir_tree(workdir, &current_index, &target_entries)?;
    write_reference(git_dir, "HEAD", &target_oid)?;

    let sig = default_signature();
    let old_oid = previous_head.as_ref().unwrap_or(&OID::zero()).clone();
    append_reflog(
        git_dir,
        "HEAD",
        &old_oid,
        &target_oid,
        &sig,
        &format!("checkout: moving from {} to {}", current_desc, spec),
    )?;

    Ok(SwitchResult {
        previous_head,
        head_oid: target_oid,
        head_ref: None,
        updated_paths: update.updated_paths,
        removed_paths: update.removed_paths,
    })
}

pub fn reset(
    git_dir: &Path,
    workdir: Option<&Path>,
    spec: &str,
    mode: ResetMode,
) -> Result<ResetResult, MuonGitError> {
    let target_oid = resolve_revision(git_dir, spec)?;
    let previous_head = current_head_oid(git_dir)?;
    let moved_ref = current_head_target_ref(git_dir)?;
    let current_index = read_index(git_dir)?;

    if let Some(ref_name) = moved_ref.as_deref() {
        write_reference(git_dir, ref_name, &target_oid)?;
    } else {
        write_reference(git_dir, "HEAD", &target_oid)?;
    }

    let mut update = WorkdirUpdate::default();
    if mode != ResetMode::Soft {
        let target_entries = materialize_commit_tree(git_dir, &target_oid)?;
        let target_index = index_from_materialized(&target_entries);
        write_index(git_dir, &target_index)?;

        if mode == ResetMode::Hard {
            let wd = workdir.ok_or(MuonGitError::BareRepo)?;
            update = apply_workdir_tree(wd, &current_index, &target_entries)?;
        }
    }

    let sig = default_signature();
    let message = format!("reset: moving to {}", spec);
    if let Some(ref_name) = moved_ref.as_deref() {
        append_reflog(git_dir, ref_name, &previous_head, &target_oid, &sig, &message)?;
    }
    append_reflog(git_dir, "HEAD", &previous_head, &target_oid, &sig, &message)?;

    Ok(ResetResult {
        previous_head,
        head_oid: target_oid,
        moved_ref,
        updated_paths: update.updated_paths,
        removed_paths: update.removed_paths,
    })
}

pub fn restore(
    git_dir: &Path,
    workdir: Option<&Path>,
    paths: &[&str],
    opts: &RestoreOptions,
) -> Result<RestoreResult, MuonGitError> {
    let worktree_requested = opts.worktree || !opts.staged;
    let source_spec = if opts.source.is_some() || opts.staged {
        Some(opts.source.as_deref().unwrap_or("HEAD"))
    } else {
        None
    };
    let source_entries = if let Some(spec) = source_spec {
        Some(materialize_revision_tree(git_dir, spec)?)
    } else {
        None
    };
    let original_index = read_index(git_dir)?;
    let mut index = original_index.clone();
    let mut result = RestoreResult::default();

    for path in paths {
        if opts.staged {
            let source = source_entries.as_ref().unwrap();
            if let Some(entry) = source.get(*path) {
                index.add(index_entry_from_materialized(path, entry));
                result.staged_paths.push((*path).to_string());
            } else if index.remove(path) {
                result.removed_from_index.push((*path).to_string());
            } else {
                return Err(MuonGitError::NotFound(format!(
                    "path '{}' not found in restore source",
                    path
                )));
            }
        }
    }

    if opts.staged {
        write_index(git_dir, &index)?;
    }

    if worktree_requested {
        let wd = workdir.ok_or(MuonGitError::BareRepo)?;
        let index_source = index.clone();
        for path in paths {
            if let Some(source) = source_entries.as_ref() {
                if opts.source.is_some() {
                    restore_path_from_materialized(
                        wd,
                        path,
                        source.get(*path),
                        original_index.find(path).is_some() || wd.join(path).exists(),
                        &mut result,
                    )?;
                    continue;
                }
            }

            restore_path_from_index(
                git_dir,
                wd,
                path,
                index_source.find(path),
                original_index.find(path).is_some() || wd.join(path).exists(),
                &mut result,
            )?;
        }
    }

    Ok(result)
}

impl Repository {
    pub fn checkout_index(&self, opts: &CheckoutOptions) -> Result<CheckoutResult, MuonGitError> {
        let workdir = self.workdir().ok_or(MuonGitError::BareRepo)?;
        checkout_index(self.git_dir(), workdir, opts)
    }

    pub fn checkout_paths(
        &self,
        paths: &[&str],
        opts: &CheckoutOptions,
    ) -> Result<CheckoutResult, MuonGitError> {
        let workdir = self.workdir().ok_or(MuonGitError::BareRepo)?;
        checkout_paths(self.git_dir(), workdir, paths, opts)
    }

    pub fn switch_branch(
        &self,
        name: &str,
        opts: &SwitchOptions,
    ) -> Result<SwitchResult, MuonGitError> {
        let workdir = self.workdir().ok_or(MuonGitError::BareRepo)?;
        switch_branch(self.git_dir(), workdir, name, opts)
    }

    pub fn checkout_revision(
        &self,
        spec: &str,
        opts: &SwitchOptions,
    ) -> Result<SwitchResult, MuonGitError> {
        let workdir = self.workdir().ok_or(MuonGitError::BareRepo)?;
        checkout_revision(self.git_dir(), workdir, spec, opts)
    }

    pub fn reset(
        &self,
        spec: &str,
        mode: ResetMode,
    ) -> Result<ResetResult, MuonGitError> {
        reset(self.git_dir(), self.workdir(), spec, mode)
    }

    pub fn restore(
        &self,
        paths: &[&str],
        opts: &RestoreOptions,
    ) -> Result<RestoreResult, MuonGitError> {
        restore(self.git_dir(), self.workdir(), paths, opts)
    }
}

fn current_head_target_ref(git_dir: &Path) -> Result<Option<String>, MuonGitError> {
    let head = read_reference(git_dir, "HEAD")?;
    Ok(head
        .strip_prefix("ref: ")
        .map(|target| target.trim().to_string()))
}

fn current_head_oid(git_dir: &Path) -> Result<OID, MuonGitError> {
    let head = read_reference(git_dir, "HEAD")?;
    if head.starts_with("ref: ") {
        resolve_reference(git_dir, "HEAD").map_err(|err| match err {
            MuonGitError::NotFound(_) => MuonGitError::UnbornBranch,
            other => other,
        })
    } else {
        OID::from_hex(head.trim())
    }
}

fn describe_head(git_dir: &Path) -> Result<String, MuonGitError> {
    let head = read_reference(git_dir, "HEAD")?;
    if let Some(target) = head.strip_prefix("ref: ") {
        let target = target.trim();
        Ok(target
            .strip_prefix("refs/heads/")
            .unwrap_or(target)
            .to_string())
    } else {
        let oid = OID::from_hex(head.trim())?;
        Ok(short_oid(&oid))
    }
}

fn short_oid(oid: &OID) -> String {
    oid.hex().chars().take(7).collect()
}

fn default_signature() -> Signature {
    Signature {
        name: "MuonGit".into(),
        email: "muongit@example.invalid".into(),
        time: 0,
        offset: 0,
    }
}

fn materialize_revision_tree(
    git_dir: &Path,
    spec: &str,
) -> Result<BTreeMap<String, MaterializedEntry>, MuonGitError> {
    let oid = resolve_revision(git_dir, spec)?;
    materialize_commit_tree(git_dir, &oid)
}

fn materialize_commit_tree(
    git_dir: &Path,
    commit_oid: &OID,
) -> Result<BTreeMap<String, MaterializedEntry>, MuonGitError> {
    let commit = read_commit(git_dir, commit_oid)?;
    let mut entries = BTreeMap::new();
    collect_tree_entries(git_dir, &commit.tree_id, "", &mut entries)?;
    Ok(entries)
}

fn collect_tree_entries(
    git_dir: &Path,
    tree_oid: &OID,
    prefix: &str,
    entries: &mut BTreeMap<String, MaterializedEntry>,
) -> Result<(), MuonGitError> {
    let tree_obj = read_object(git_dir, tree_oid)?.as_tree()?;
    for entry in tree_obj.entries {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", prefix, entry.name)
        };

        if entry.mode == tree::file_mode::TREE {
            collect_tree_entries(git_dir, &entry.oid, &path, entries)?;
        } else {
            let blob = read_blob(git_dir, &entry.oid)?;
            entries.insert(
                path,
                MaterializedEntry {
                    oid: entry.oid,
                    mode: entry.mode,
                    data: blob.data,
                },
            );
        }
    }
    Ok(())
}

fn index_from_materialized(entries: &BTreeMap<String, MaterializedEntry>) -> Index {
    let mut index = Index::new();
    for (path, entry) in entries {
        index.add(index_entry_from_materialized(path, entry));
    }
    index
}

fn index_entry_from_materialized(path: &str, entry: &MaterializedEntry) -> IndexEntry {
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
        file_size: entry.data.len() as u32,
        oid: entry.oid.clone(),
        flags: path.len().min(0x0FFF) as u16,
        path: path.to_string(),
    }
}

fn collect_switch_conflicts(
    git_dir: &Path,
    workdir: &Path,
    current_index: &Index,
    target_entries: &BTreeMap<String, MaterializedEntry>,
) -> Result<Vec<String>, MuonGitError> {
    let mut conflicts = BTreeSet::new();

    for path in staged_change_paths(git_dir, current_index)? {
        conflicts.insert(path);
    }

    for path in target_entries.keys() {
        match current_index.find(path) {
            Some(current) => {
                if !workdir_matches_entry(workdir, current)? {
                    conflicts.insert(path.clone());
                }
            }
            None => {
                if workdir.join(path).exists() {
                    conflicts.insert(path.clone());
                }
            }
        }
    }

    for entry in &current_index.entries {
        if !target_entries.contains_key(&entry.path) && !workdir_matches_entry(workdir, entry)? {
            conflicts.insert(entry.path.clone());
        }
    }

    Ok(conflicts.into_iter().collect())
}

fn staged_change_paths(git_dir: &Path, current_index: &Index) -> Result<Vec<String>, MuonGitError> {
    let current_head = match current_head_oid(git_dir) {
        Ok(oid) => Some(oid),
        Err(MuonGitError::UnbornBranch) => None,
        Err(err) => return Err(err),
    };

    let mut changes = BTreeSet::new();
    let head_entries = if let Some(head_oid) = current_head {
        materialize_commit_tree(git_dir, &head_oid)?
    } else {
        BTreeMap::new()
    };

    let current_paths: BTreeSet<&str> = current_index.entries.iter().map(|entry| entry.path.as_str()).collect();
    let head_paths: BTreeSet<&str> = head_entries.keys().map(String::as_str).collect();

    for entry in &current_index.entries {
        match head_entries.get(&entry.path) {
            Some(head_entry) => {
                if head_entry.oid != entry.oid || head_entry.mode != entry.mode {
                    changes.insert(entry.path.clone());
                }
            }
            None => {
                changes.insert(entry.path.clone());
            }
        }
    }

    for path in head_paths.difference(&current_paths) {
        changes.insert((*path).to_string());
    }

    Ok(changes.into_iter().collect())
}

fn workdir_matches_entry(workdir: &Path, entry: &IndexEntry) -> Result<bool, MuonGitError> {
    let path = workdir.join(&entry.path);
    if !path.exists() {
        return Ok(false);
    }
    let metadata = fs::metadata(&path)?;
    if metadata.len() as u32 != entry.file_size {
        return Ok(false);
    }
    let content = fs::read(&path)?;
    if OID::hash_object(crate::ObjectType::Blob, &content) != entry.oid {
        return Ok(false);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let is_executable = metadata.permissions().mode() & 0o111 != 0;
        let expected = entry.mode & 0o111 != 0;
        if is_executable != expected {
            return Ok(false);
        }
    }

    Ok(true)
}

fn apply_workdir_tree(
    workdir: &Path,
    current_index: &Index,
    target_entries: &BTreeMap<String, MaterializedEntry>,
) -> Result<WorkdirUpdate, MuonGitError> {
    let mut update = WorkdirUpdate::default();
    let current_paths: BTreeSet<&str> = current_index.entries.iter().map(|entry| entry.path.as_str()).collect();
    let target_paths: BTreeSet<&str> = target_entries.keys().map(String::as_str).collect();

    for path in current_paths.difference(&target_paths) {
        let file_path = workdir.join(path);
        if file_path.exists() {
            remove_workdir_path(workdir, &file_path)?;
            update.removed_paths.push((*path).to_string());
        }
    }

    for (path, entry) in target_entries {
        write_materialized_to_workdir(workdir, path, entry)?;
        update.updated_paths.push(path.clone());
    }

    Ok(update)
}

fn restore_path_from_materialized(
    workdir: &Path,
    path: &str,
    entry: Option<&MaterializedEntry>,
    known_path: bool,
    result: &mut RestoreResult,
) -> Result<(), MuonGitError> {
    match entry {
        Some(entry) => {
            write_materialized_to_workdir(workdir, path, entry)?;
            result.restored_paths.push(path.to_string());
            Ok(())
        }
        None => {
            let target = workdir.join(path);
            if target.exists() {
                remove_workdir_path(workdir, &target)?;
                result.removed_from_workdir.push(path.to_string());
                Ok(())
            } else if known_path {
                Ok(())
            } else {
                Err(MuonGitError::NotFound(format!("path '{}' not found", path)))
            }
        }
    }
}

fn restore_path_from_index(
    git_dir: &Path,
    workdir: &Path,
    path: &str,
    entry: Option<&IndexEntry>,
    known_path: bool,
    result: &mut RestoreResult,
) -> Result<(), MuonGitError> {
    match entry {
        Some(entry) => {
            write_index_entry_to_workdir(git_dir, workdir, entry)?;
            result.restored_paths.push(path.to_string());
            Ok(())
        }
        None => {
            let target = workdir.join(path);
            if target.exists() {
                remove_workdir_path(workdir, &target)?;
                result.removed_from_workdir.push(path.to_string());
                Ok(())
            } else if known_path {
                Ok(())
            } else {
                Err(MuonGitError::NotFound(format!("path '{}' not found", path)))
            }
        }
    }
}

fn write_materialized_to_workdir(
    workdir: &Path,
    path: &str,
    entry: &MaterializedEntry,
) -> Result<(), MuonGitError> {
    let target = workdir.join(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&target, &entry.data)?;
    set_mode(&target, entry.mode)?;
    Ok(())
}

fn write_index_entry_to_workdir(
    git_dir: &Path,
    workdir: &Path,
    entry: &IndexEntry,
) -> Result<(), MuonGitError> {
    let target = workdir.join(&entry.path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let blob = read_blob(git_dir, &entry.oid)?;
    fs::write(&target, &blob.data)?;
    set_mode(&target, entry.mode)?;
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<(), MuonGitError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
        fs::set_permissions(path, fs::Permissions::from_mode(perms))?;
    }
    Ok(())
}

fn remove_workdir_path(root: &Path, path: &Path) -> Result<(), MuonGitError> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    let mut current = path.parent();
    while let Some(parent) = current {
        if parent == root {
            break;
        }
        let is_empty = fs::read_dir(parent)?.next().is_none();
        if !is_empty {
            break;
        }
        fs::remove_dir(parent)?;
        current = parent.parent();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::serialize_commit;
    use crate::index::{write_index, Index, IndexEntry};
    use crate::odb::write_loose_object;
    use crate::refs::{read_reference, resolve_reference, write_reference, write_symbolic_reference};
    use crate::repository::Repository;
    use crate::tree::{file_mode, serialize_tree, TreeEntry};
    use crate::types::ObjectType;

    fn setup_repo(name: &str) -> (std::path::PathBuf, Repository) {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../tmp/{}", name));
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        (tmp, repo)
    }

    fn add_blob_to_index(
        git_dir: &Path,
        index: &mut Index,
        path: &str,
        content: &[u8],
        executable: bool,
    ) {
        let oid = write_loose_object(git_dir, ObjectType::Blob, content).unwrap();
        let mode = if executable { 0o100755 } else { 0o100644 };
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
            file_size: content.len() as u32,
            oid,
            flags: path.len().min(0xFFF) as u16,
            path: path.to_string(),
        });
    }

    fn write_tree_with_files(git_dir: &Path, files: &[(&str, &[u8])]) -> OID {
        let entries = files
            .iter()
            .map(|(name, content)| TreeEntry {
                mode: file_mode::BLOB,
                name: (*name).to_string(),
                oid: write_loose_object(git_dir, ObjectType::Blob, content).unwrap(),
            })
            .collect::<Vec<_>>();
        let tree_data = serialize_tree(&entries);
        write_loose_object(git_dir, ObjectType::Tree, &tree_data).unwrap()
    }

    fn write_commit_with_tree(
        git_dir: &Path,
        tree_oid: &OID,
        parents: &[OID],
        message: &str,
        time: i64,
    ) -> OID {
        let sig = Signature {
            name: "Muon Test".into(),
            email: "test@muon.ai".into(),
            time,
            offset: 0,
        };
        let data = serialize_commit(tree_oid, parents, &sig, &sig, &format!("{message}\n"), None);
        write_loose_object(git_dir, ObjectType::Commit, &data).unwrap()
    }

    fn clear_workdir(workdir: &Path) {
        if let Ok(entries) = fs::read_dir(workdir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.file_name().is_some_and(|name| name == ".git") {
                    continue;
                }
                if path.is_dir() {
                    fs::remove_dir_all(path).unwrap();
                } else {
                    fs::remove_file(path).unwrap();
                }
            }
        }
    }

    fn seed_worktree_from_commit(repo: &Repository, commit_oid: &OID) {
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();
        let entries = materialize_commit_tree(git_dir, commit_oid).unwrap();
        clear_workdir(workdir);
        write_index(git_dir, &index_from_materialized(&entries)).unwrap();
        checkout_index(git_dir, workdir, &CheckoutOptions { force: true }).unwrap();
    }

    #[test]
    fn test_checkout_basic() {
        let (tmp, repo) = setup_repo("test_checkout_basic");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let mut index = Index::new();
        add_blob_to_index(git_dir, &mut index, "hello.txt", b"Hello, world!\n", false);
        add_blob_to_index(git_dir, &mut index, "src/main.rs", b"fn main() {}\n", false);
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        let result = checkout_index(git_dir, workdir, &opts).unwrap();

        assert_eq!(result.updated.len(), 2);
        assert!(result.conflicts.is_empty());
        assert_eq!(
            fs::read_to_string(workdir.join("hello.txt")).unwrap(),
            "Hello, world!\n"
        );
        assert_eq!(
            fs::read_to_string(workdir.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_creates_directories() {
        let (tmp, repo) = setup_repo("test_checkout_dirs");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let mut index = Index::new();
        add_blob_to_index(
            git_dir,
            &mut index,
            "a/b/c/deep.txt",
            b"deep content",
            false,
        );
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        let result = checkout_index(git_dir, workdir, &opts).unwrap();

        assert_eq!(result.updated.len(), 1);
        assert!(workdir.join("a/b/c/deep.txt").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_conflict_detection() {
        let (tmp, repo) = setup_repo("test_checkout_conflict");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        // Create existing file
        fs::write(workdir.join("existing.txt"), "local changes").unwrap();

        let mut index = Index::new();
        add_blob_to_index(git_dir, &mut index, "existing.txt", b"index content", false);
        write_index(git_dir, &index).unwrap();

        // Without force: should detect conflict
        let opts = CheckoutOptions { force: false };
        let result = checkout_index(git_dir, workdir, &opts).unwrap();

        assert!(result.updated.is_empty());
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0], "existing.txt");
        // Original content preserved
        assert_eq!(
            fs::read_to_string(workdir.join("existing.txt")).unwrap(),
            "local changes"
        );

        // With force: should overwrite
        let opts = CheckoutOptions { force: true };
        let result = checkout_index(git_dir, workdir, &opts).unwrap();

        assert_eq!(result.updated.len(), 1);
        assert_eq!(
            fs::read_to_string(workdir.join("existing.txt")).unwrap(),
            "index content"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_executable_mode() {
        let (tmp, repo) = setup_repo("test_checkout_exec");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let mut index = Index::new();
        add_blob_to_index(git_dir, &mut index, "script.sh", b"#!/bin/sh\necho hi\n", true);
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        checkout_index(git_dir, workdir, &opts).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(workdir.join("script.sh")).unwrap().permissions();
            assert!(perms.mode() & 0o111 != 0, "file should be executable");
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_paths() {
        let (tmp, repo) = setup_repo("test_checkout_paths");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let mut index = Index::new();
        add_blob_to_index(git_dir, &mut index, "a.txt", b"aaa", false);
        add_blob_to_index(git_dir, &mut index, "b.txt", b"bbb", false);
        add_blob_to_index(git_dir, &mut index, "c.txt", b"ccc", false);
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        let result = checkout_paths(git_dir, workdir, &["a.txt", "c.txt"], &opts).unwrap();

        assert_eq!(result.updated.len(), 2);
        assert!(workdir.join("a.txt").exists());
        assert!(!workdir.join("b.txt").exists());
        assert!(workdir.join("c.txt").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_path_not_in_index() {
        let (tmp, repo) = setup_repo("test_checkout_notfound");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let index = Index::new();
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        let result = checkout_paths(git_dir, workdir, &["nonexistent.txt"], &opts);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_switch_branch_updates_head_and_worktree() {
        let (tmp, repo) = setup_repo("test_switch_branch_updates_head_and_worktree");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let main_tree = write_tree_with_files(
            git_dir,
            &[("shared.txt", b"main\n"), ("only-main.txt", b"remove me\n")],
        );
        let main_commit = write_commit_with_tree(git_dir, &main_tree, &[], "main", 1);
        let feature_tree = write_tree_with_files(
            git_dir,
            &[("shared.txt", b"feature\n"), ("only-feature.txt", b"add me\n")],
        );
        let feature_commit =
            write_commit_with_tree(git_dir, &feature_tree, &[main_commit.clone()], "feature", 2);

        write_reference(git_dir, "refs/heads/main", &main_commit).unwrap();
        write_reference(git_dir, "refs/heads/feature", &feature_commit).unwrap();
        write_symbolic_reference(git_dir, "HEAD", "refs/heads/main").unwrap();
        seed_worktree_from_commit(&repo, &main_commit);

        let result = repo
            .switch_branch("feature", &SwitchOptions::default())
            .unwrap();

        assert_eq!(result.previous_head, Some(main_commit.clone()));
        assert_eq!(result.head_oid, feature_commit);
        assert_eq!(result.head_ref.as_deref(), Some("refs/heads/feature"));
        assert_eq!(read_reference(git_dir, "HEAD").unwrap(), "ref: refs/heads/feature");
        assert_eq!(
            fs::read_to_string(workdir.join("shared.txt")).unwrap(),
            "feature\n"
        );
        assert!(!workdir.join("only-main.txt").exists());
        assert_eq!(
            fs::read_to_string(workdir.join("only-feature.txt")).unwrap(),
            "add me\n"
        );
        assert!(result.updated_paths.contains(&"shared.txt".to_string()));
        assert!(result.removed_paths.contains(&"only-main.txt".to_string()));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_revision_detaches_head() {
        let (tmp, repo) = setup_repo("test_checkout_revision_detaches_head");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let main_tree = write_tree_with_files(git_dir, &[("shared.txt", b"main\n")]);
        let main_commit = write_commit_with_tree(git_dir, &main_tree, &[], "main", 1);
        let feature_tree = write_tree_with_files(git_dir, &[("shared.txt", b"detached\n")]);
        let feature_commit =
            write_commit_with_tree(git_dir, &feature_tree, &[main_commit.clone()], "feature", 2);

        write_reference(git_dir, "refs/heads/main", &main_commit).unwrap();
        write_reference(git_dir, "refs/heads/feature", &feature_commit).unwrap();
        write_symbolic_reference(git_dir, "HEAD", "refs/heads/main").unwrap();
        seed_worktree_from_commit(&repo, &main_commit);

        let result = repo
            .checkout_revision(&feature_commit.hex(), &SwitchOptions::default())
            .unwrap();

        assert_eq!(result.previous_head, Some(main_commit));
        assert_eq!(result.head_oid, feature_commit.clone());
        assert_eq!(result.head_ref, None);
        assert_eq!(read_reference(git_dir, "HEAD").unwrap(), feature_commit.hex());
        assert_eq!(
            fs::read_to_string(workdir.join("shared.txt")).unwrap(),
            "detached\n"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_switch_branch_rejects_local_changes() {
        let (tmp, repo) = setup_repo("test_switch_branch_rejects_local_changes");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let main_tree = write_tree_with_files(git_dir, &[("shared.txt", b"main\n")]);
        let main_commit = write_commit_with_tree(git_dir, &main_tree, &[], "main", 1);
        let feature_tree = write_tree_with_files(git_dir, &[("shared.txt", b"feature\n")]);
        let feature_commit =
            write_commit_with_tree(git_dir, &feature_tree, &[main_commit.clone()], "feature", 2);

        write_reference(git_dir, "refs/heads/main", &main_commit).unwrap();
        write_reference(git_dir, "refs/heads/feature", &feature_commit).unwrap();
        write_symbolic_reference(git_dir, "HEAD", "refs/heads/main").unwrap();
        seed_worktree_from_commit(&repo, &main_commit);
        fs::write(workdir.join("shared.txt"), "dirty\n").unwrap();

        let err = repo
            .switch_branch("feature", &SwitchOptions::default())
            .unwrap_err();

        match err {
            MuonGitError::Conflict(msg) => assert!(msg.contains("shared.txt")),
            other => panic!("expected conflict, got {other:?}"),
        }
        assert_eq!(read_reference(git_dir, "HEAD").unwrap(), "ref: refs/heads/main");
        assert_eq!(fs::read_to_string(workdir.join("shared.txt")).unwrap(), "dirty\n");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_reset_modes_update_refs_index_and_worktree() {
        let (tmp, repo) = setup_repo("test_reset_modes_update_refs_index_and_worktree");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let base_tree = write_tree_with_files(git_dir, &[("file.txt", b"base\n")]);
        let base_commit = write_commit_with_tree(git_dir, &base_tree, &[], "base", 1);
        let changed_tree = write_tree_with_files(
            git_dir,
            &[("file.txt", b"changed\n"), ("new.txt", b"new\n")],
        );
        let changed_commit =
            write_commit_with_tree(git_dir, &changed_tree, &[base_commit.clone()], "changed", 2);

        write_reference(git_dir, "refs/heads/main", &changed_commit).unwrap();
        write_symbolic_reference(git_dir, "HEAD", "refs/heads/main").unwrap();
        seed_worktree_from_commit(&repo, &changed_commit);

        let base_entries = materialize_commit_tree(git_dir, &base_commit).unwrap();
        let changed_entries = materialize_commit_tree(git_dir, &changed_commit).unwrap();

        fs::write(workdir.join("file.txt"), "dirty soft\n").unwrap();
        repo.reset(&base_commit.hex(), ResetMode::Soft).unwrap();
        assert_eq!(resolve_reference(git_dir, "HEAD").unwrap(), base_commit);
        assert_eq!(
            read_index(git_dir)
                .unwrap()
                .find("file.txt")
                .unwrap()
                .oid,
            changed_entries.get("file.txt").unwrap().oid
        );
        assert_eq!(fs::read_to_string(workdir.join("file.txt")).unwrap(), "dirty soft\n");

        write_reference(git_dir, "refs/heads/main", &changed_commit).unwrap();
        seed_worktree_from_commit(&repo, &changed_commit);
        fs::write(workdir.join("file.txt"), "dirty mixed\n").unwrap();
        repo.reset(&base_commit.hex(), ResetMode::Mixed).unwrap();
        assert_eq!(resolve_reference(git_dir, "HEAD").unwrap(), base_commit);
        assert_eq!(
            read_index(git_dir)
                .unwrap()
                .find("file.txt")
                .unwrap()
                .oid,
            base_entries.get("file.txt").unwrap().oid
        );
        assert_eq!(fs::read_to_string(workdir.join("file.txt")).unwrap(), "dirty mixed\n");
        assert!(workdir.join("new.txt").exists());

        write_reference(git_dir, "refs/heads/main", &changed_commit).unwrap();
        seed_worktree_from_commit(&repo, &changed_commit);
        fs::write(workdir.join("file.txt"), "dirty hard\n").unwrap();
        let hard = repo.reset(&base_commit.hex(), ResetMode::Hard).unwrap();
        assert_eq!(resolve_reference(git_dir, "HEAD").unwrap(), base_commit);
        assert_eq!(
            fs::read_to_string(workdir.join("file.txt")).unwrap(),
            "base\n"
        );
        assert!(!workdir.join("new.txt").exists());
        assert!(hard.removed_paths.contains(&"new.txt".to_string()));
        assert_eq!(
            read_index(git_dir)
                .unwrap()
                .find("file.txt")
                .unwrap()
                .oid,
            base_entries.get("file.txt").unwrap().oid
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_restore_staged_and_worktree_paths() {
        let (tmp, repo) = setup_repo("test_restore_staged_and_worktree_paths");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let commit_tree = write_tree_with_files(git_dir, &[("file.txt", b"committed\n")]);
        let commit = write_commit_with_tree(git_dir, &commit_tree, &[], "commit", 1);

        write_reference(git_dir, "refs/heads/main", &commit).unwrap();
        write_symbolic_reference(git_dir, "HEAD", "refs/heads/main").unwrap();
        seed_worktree_from_commit(&repo, &commit);

        let head_entry = materialize_commit_tree(git_dir, &commit)
            .unwrap()
            .get("file.txt")
            .unwrap()
            .clone();

        fs::write(workdir.join("file.txt"), "worktree\n").unwrap();
        let staged_oid = write_loose_object(git_dir, ObjectType::Blob, b"staged\n").unwrap();
        let mut index = read_index(git_dir).unwrap();
        let existing = index.find("file.txt").unwrap().clone();
        index.add(IndexEntry {
            file_size: 7,
            oid: staged_oid,
            ..existing
        });
        write_index(git_dir, &index).unwrap();

        let result = repo
            .restore(
                &["file.txt"],
                &RestoreOptions {
                    source: None,
                    staged: true,
                    worktree: true,
                },
            )
            .unwrap();

        assert_eq!(
            read_index(git_dir)
                .unwrap()
                .find("file.txt")
                .unwrap()
                .oid,
            head_entry.oid
        );
        assert_eq!(
            fs::read_to_string(workdir.join("file.txt")).unwrap(),
            "committed\n"
        );
        assert_eq!(result.staged_paths, vec!["file.txt".to_string()]);
        assert_eq!(result.restored_paths, vec!["file.txt".to_string()]);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_restore_from_source_updates_worktree_only() {
        let (tmp, repo) = setup_repo("test_restore_from_source_updates_worktree_only");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let base_tree = write_tree_with_files(git_dir, &[("file.txt", b"base\n")]);
        let base_commit = write_commit_with_tree(git_dir, &base_tree, &[], "base", 1);
        let changed_tree = write_tree_with_files(git_dir, &[("file.txt", b"changed\n")]);
        let changed_commit =
            write_commit_with_tree(git_dir, &changed_tree, &[base_commit.clone()], "changed", 2);

        write_reference(git_dir, "refs/heads/main", &changed_commit).unwrap();
        write_symbolic_reference(git_dir, "HEAD", "refs/heads/main").unwrap();
        seed_worktree_from_commit(&repo, &changed_commit);
        fs::write(workdir.join("file.txt"), "dirty\n").unwrap();

        repo.restore(
            &["file.txt"],
            &RestoreOptions {
                source: Some(base_commit.hex()),
                staged: false,
                worktree: true,
            },
        )
        .unwrap();

        assert_eq!(fs::read_to_string(workdir.join("file.txt")).unwrap(), "base\n");
        assert_eq!(
            read_index(git_dir)
                .unwrap()
                .find("file.txt")
                .unwrap()
                .oid,
            materialize_commit_tree(git_dir, &changed_commit)
                .unwrap()
                .get("file.txt")
                .unwrap()
                .oid
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_restore_missing_path_fails() {
        let (tmp, repo) = setup_repo("test_restore_missing_path_fails");
        let git_dir = repo.git_dir();

        let commit_tree = write_tree_with_files(git_dir, &[("file.txt", b"committed\n")]);
        let commit = write_commit_with_tree(git_dir, &commit_tree, &[], "commit", 1);
        write_reference(git_dir, "refs/heads/main", &commit).unwrap();
        write_symbolic_reference(git_dir, "HEAD", "refs/heads/main").unwrap();
        seed_worktree_from_commit(&repo, &commit);

        let err = repo
            .restore(&["missing.txt"], &RestoreOptions::default())
            .unwrap_err();

        match err {
            MuonGitError::NotFound(msg) => assert!(msg.contains("missing.txt")),
            other => panic!("expected not found, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
