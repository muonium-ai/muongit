//! First-class branch API built on top of refs/refdb.

use std::path::Path;

use crate::config::Config;
use crate::error::MuonGitError;
use crate::oid::OID;
use crate::refdb::RefDb;
use crate::repository::Repository;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchType {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchUpstream {
    pub remote_name: String,
    pub merge_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub reference_name: String,
    pub target: Option<OID>,
    pub kind: BranchType,
    pub is_head: bool,
    pub upstream: Option<BranchUpstream>,
}

pub fn create_branch(
    git_dir: &Path,
    name: &str,
    target: Option<&OID>,
    force: bool,
) -> Result<Branch, MuonGitError> {
    let ref_name = local_branch_ref(name);
    let refdb = RefDb::open(git_dir);

    if reference_exists(git_dir, &ref_name) {
        if !force {
            return Err(MuonGitError::Conflict(format!(
                "branch '{}' already exists",
                name
            )));
        }
        refdb.delete(&ref_name)?;
    }

    let target_oid = match target {
        Some(oid) => oid.clone(),
        None => head_target_oid(git_dir)?,
    };
    refdb.write(&ref_name, &target_oid)?;
    lookup_branch(git_dir, name, BranchType::Local)
}

pub fn lookup_branch(
    git_dir: &Path,
    name: &str,
    kind: BranchType,
) -> Result<Branch, MuonGitError> {
    let ref_name = branch_ref_name(name, kind);
    build_branch(git_dir, &ref_name, kind)
}

pub fn list_branches(
    git_dir: &Path,
    kind: Option<BranchType>,
) -> Result<Vec<Branch>, MuonGitError> {
    let refs = crate::refs::list_references(git_dir)?;
    let mut branches = Vec::new();

    for (ref_name, _) in refs {
        if let Some((branch_name, branch_type)) = branch_name_and_type(&ref_name) {
            if kind.is_none() || kind == Some(branch_type) {
                branches.push(build_branch(git_dir, &ref_name, branch_type)?);
                if branches.last().is_some() {
                    let _ = &branch_name;
                }
            }
        }
    }

    branches.sort_by(|a, b| a.reference_name.cmp(&b.reference_name));
    Ok(branches)
}

pub fn rename_branch(
    git_dir: &Path,
    old_name: &str,
    new_name: &str,
    force: bool,
) -> Result<Branch, MuonGitError> {
    let old_ref = local_branch_ref(old_name);
    let new_ref = local_branch_ref(new_name);
    if old_ref == new_ref {
        return lookup_branch(git_dir, new_name, BranchType::Local);
    }

    let refdb = RefDb::open(git_dir);
    let old_branch = refdb.read(&old_ref)?;
    if reference_exists(git_dir, &new_ref) {
        if !force {
            return Err(MuonGitError::Conflict(format!(
                "branch '{}' already exists",
                new_name
            )));
        }
        if current_head_ref(git_dir)?.as_deref() == Some(new_ref.as_str()) {
            return Err(MuonGitError::Conflict(format!(
                "cannot replace checked out branch '{}'",
                new_name
            )));
        }
        delete_branch(git_dir, new_name, BranchType::Local)?;
    }

    if let Some(symbolic_target) = old_branch.symbolic_target {
        refdb.write_symbolic(&new_ref, &symbolic_target)?;
    } else if let Some(target) = old_branch.target {
        refdb.write(&new_ref, &target)?;
    } else {
        return Err(MuonGitError::InvalidObject(format!(
            "branch '{}' has no target",
            old_name
        )));
    }
    refdb.delete(&old_ref)?;
    move_branch_upstream(git_dir, old_name, new_name)?;

    if current_head_ref(git_dir)?.as_deref() == Some(old_ref.as_str()) {
        refdb.write_symbolic("HEAD", &new_ref)?;
    }

    lookup_branch(git_dir, new_name, BranchType::Local)
}

pub fn delete_branch(
    git_dir: &Path,
    name: &str,
    kind: BranchType,
) -> Result<bool, MuonGitError> {
    let ref_name = branch_ref_name(name, kind);
    if kind == BranchType::Local && current_head_ref(git_dir)?.as_deref() == Some(ref_name.as_str()) {
        return Err(MuonGitError::Conflict(format!(
            "cannot delete checked out branch '{}'",
            name
        )));
    }

    let deleted = RefDb::open(git_dir).delete(&ref_name)?;
    if deleted && kind == BranchType::Local {
        clear_branch_upstream(git_dir, name)?;
    }
    Ok(deleted)
}

pub fn branch_upstream(git_dir: &Path, name: &str) -> Result<Option<BranchUpstream>, MuonGitError> {
    let config = load_repo_config(git_dir)?;
    let section = branch_section(name);
    let remote = config.get(&section, "remote").map(|value| value.to_string());
    let merge = config.get(&section, "merge").map(|value| value.to_string());

    match (remote, merge) {
        (Some(remote_name), Some(merge_ref)) => Ok(Some(BranchUpstream {
            remote_name,
            merge_ref,
        })),
        (None, None) => Ok(None),
        _ => Err(MuonGitError::InvalidSpec(format!(
            "branch '{}' has incomplete upstream config",
            name
        ))),
    }
}

pub fn set_branch_upstream(
    git_dir: &Path,
    name: &str,
    upstream: Option<BranchUpstream>,
) -> Result<(), MuonGitError> {
    let branch_ref = local_branch_ref(name);
    if !reference_exists(git_dir, &branch_ref) {
        return Err(MuonGitError::NotFound(format!(
            "branch '{}' not found",
            name
        )));
    }

    let config_path = git_dir.join("config");
    let mut config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        Config::with_path(&config_path)
    };
    let section = branch_section(name);

    if let Some(upstream) = upstream {
        config.set(&section, "remote", &upstream.remote_name);
        config.set(&section, "merge", &upstream.merge_ref);
    } else {
        config.unset(&section, "remote");
        config.unset(&section, "merge");
    }
    config.save()
}

impl Repository {
    pub fn create_branch(
        &self,
        name: &str,
        target: Option<&OID>,
        force: bool,
    ) -> Result<Branch, MuonGitError> {
        create_branch(self.git_dir(), name, target, force)
    }

    pub fn lookup_branch(
        &self,
        name: &str,
        kind: BranchType,
    ) -> Result<Branch, MuonGitError> {
        lookup_branch(self.git_dir(), name, kind)
    }

    pub fn list_branches(
        &self,
        kind: Option<BranchType>,
    ) -> Result<Vec<Branch>, MuonGitError> {
        list_branches(self.git_dir(), kind)
    }
}

fn build_branch(git_dir: &Path, ref_name: &str, kind: BranchType) -> Result<Branch, MuonGitError> {
    let reference = RefDb::open(git_dir).read(ref_name)?;
    let target = if reference.is_symbolic() {
        crate::refs::resolve_reference(git_dir, ref_name).ok()
    } else {
        reference.target.clone()
    };
    let name = short_branch_name(ref_name, kind).ok_or_else(|| {
        MuonGitError::InvalidSpec(format!("not a branch reference: {}", ref_name))
    })?;

    Ok(Branch {
        name: name.to_string(),
        reference_name: ref_name.to_string(),
        target,
        kind,
        is_head: current_head_ref(git_dir)?.as_deref() == Some(ref_name),
        upstream: if kind == BranchType::Local {
            branch_upstream(git_dir, name)?
        } else {
            None
        },
    })
}

fn branch_ref_name(name: &str, kind: BranchType) -> String {
    match kind {
        BranchType::Local => local_branch_ref(name),
        BranchType::Remote => format!("refs/remotes/{}", name),
    }
}

fn local_branch_ref(name: &str) -> String {
    format!("refs/heads/{}", name)
}

fn short_branch_name(ref_name: &str, kind: BranchType) -> Option<&str> {
    match kind {
        BranchType::Local => ref_name.strip_prefix("refs/heads/"),
        BranchType::Remote => ref_name.strip_prefix("refs/remotes/"),
    }
}

fn branch_name_and_type(ref_name: &str) -> Option<(&str, BranchType)> {
    if let Some(name) = ref_name.strip_prefix("refs/heads/") {
        Some((name, BranchType::Local))
    } else {
        ref_name
            .strip_prefix("refs/remotes/")
            .map(|name| (name, BranchType::Remote))
    }
}

fn current_head_ref(git_dir: &Path) -> Result<Option<String>, MuonGitError> {
    let head = crate::refs::read_reference(git_dir, "HEAD")?;
    Ok(head
        .strip_prefix("ref: ")
        .map(|value| value.trim().to_string())
        .filter(|value| value.starts_with("refs/heads/")))
}

fn head_target_oid(git_dir: &Path) -> Result<OID, MuonGitError> {
    let head = crate::refs::read_reference(git_dir, "HEAD")?;
    if head.starts_with("ref: ") {
        crate::refs::resolve_reference(git_dir, "HEAD").map_err(|err| match err {
            MuonGitError::NotFound(_) => MuonGitError::UnbornBranch,
            other => other,
        })
    } else {
        OID::from_hex(head.trim())
    }
}

fn branch_section(name: &str) -> String {
    format!("branch.{}", name)
}

fn load_repo_config(git_dir: &Path) -> Result<Config, MuonGitError> {
    let config_path = git_dir.join("config");
    if config_path.exists() {
        Config::load(&config_path)
    } else {
        Ok(Config::with_path(&config_path))
    }
}

fn clear_branch_upstream(git_dir: &Path, name: &str) -> Result<(), MuonGitError> {
    let config_path = git_dir.join("config");
    if !config_path.exists() {
        return Ok(());
    }

    let mut config = Config::load(&config_path)?;
    let section = branch_section(name);
    config.unset(&section, "remote");
    config.unset(&section, "merge");
    config.save()
}

fn move_branch_upstream(git_dir: &Path, old_name: &str, new_name: &str) -> Result<(), MuonGitError> {
    let upstream = branch_upstream(git_dir, old_name)?;
    clear_branch_upstream(git_dir, old_name)?;
    if let Some(upstream) = upstream {
        set_branch_upstream(git_dir, new_name, Some(upstream))?;
    }
    Ok(())
}

fn reference_exists(git_dir: &Path, name: &str) -> bool {
    crate::refs::read_reference(git_dir, name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use crate::refdb::packed_references;

    fn test_dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name)
    }

    #[test]
    fn test_branch_create_lookup_list_and_upstream() {
        let tmp = test_dir("test_branch_create_lookup_list_and_upstream");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let main_oid = OID::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        crate::refs::write_reference(git_dir, "refs/heads/main", &main_oid).unwrap();

        let branch = create_branch(git_dir, "feature", None, false).unwrap();
        assert_eq!(branch.name, "feature");
        assert_eq!(branch.reference_name, "refs/heads/feature");
        assert_eq!(branch.target.as_ref(), Some(&main_oid));
        assert!(!branch.is_head);

        set_branch_upstream(
            git_dir,
            "feature",
            Some(BranchUpstream {
                remote_name: "origin".into(),
                merge_ref: "refs/heads/main".into(),
            }),
        )
        .unwrap();
        let looked_up = lookup_branch(git_dir, "feature", BranchType::Local).unwrap();
        assert_eq!(
            looked_up.upstream,
            Some(BranchUpstream {
                remote_name: "origin".into(),
                merge_ref: "refs/heads/main".into(),
            })
        );

        let branches = list_branches(git_dir, Some(BranchType::Local)).unwrap();
        assert!(branches.iter().any(|entry| entry.name == "main" && entry.is_head));
        assert!(branches.iter().any(|entry| entry.name == "feature"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_branch_create_from_detached_head() {
        let tmp = test_dir("test_branch_create_from_detached_head");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let detached_oid = OID::from_hex("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        fs::write(git_dir.join("HEAD"), format!("{}\n", detached_oid.hex())).unwrap();

        let branch = create_branch(git_dir, "detached-copy", None, false).unwrap();
        assert_eq!(branch.target.as_ref(), Some(&detached_oid));
        assert!(!branch.is_head);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_branch_rename_updates_head_and_tracks_packed_refs() {
        let tmp = test_dir("test_branch_rename_updates_head_and_tracks_packed_refs");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let topic_oid = OID::from_hex("cccccccccccccccccccccccccccccccccccccccc").unwrap();
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/heads/topic\n", topic_oid.hex()),
        )
        .unwrap();
        crate::refs::write_symbolic_reference(git_dir, "HEAD", "refs/heads/topic").unwrap();
        set_branch_upstream(
            git_dir,
            "topic",
            Some(BranchUpstream {
                remote_name: "origin".into(),
                merge_ref: "refs/heads/main".into(),
            }),
        )
        .unwrap();

        let renamed = rename_branch(git_dir, "topic", "renamed", false).unwrap();
        assert_eq!(renamed.name, "renamed");
        assert_eq!(crate::refs::read_reference(git_dir, "HEAD").unwrap(), "ref: refs/heads/renamed");
        assert!(crate::refs::read_reference(git_dir, "refs/heads/topic").is_err());
        assert_eq!(renamed.target.as_ref(), Some(&topic_oid));
        assert_eq!(
            branch_upstream(git_dir, "renamed").unwrap(),
            Some(BranchUpstream {
                remote_name: "origin".into(),
                merge_ref: "refs/heads/main".into(),
            })
        );
        assert!(packed_references(git_dir).unwrap().get("refs/heads/topic").is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_branch_delete_rejects_checked_out_branch() {
        let tmp = test_dir("test_branch_delete_rejects_checked_out_branch");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let main_oid = OID::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        crate::refs::write_reference(git_dir, "refs/heads/main", &main_oid).unwrap();

        let result = delete_branch(git_dir, "main", BranchType::Local);
        assert!(matches!(result, Err(MuonGitError::Conflict(_))));

        let _ = fs::remove_dir_all(&tmp);
    }
}
