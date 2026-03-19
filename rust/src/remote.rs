//! Remote management
//! Parity: libgit2 src/libgit2/remote.c

use std::path::Path;

use crate::config::Config;
use crate::error::MuonGitError;

/// A git remote (e.g. "origin").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
    pub push_url: Option<String>,
    pub fetch_refspecs: Vec<String>,
}

/// List all remote names from the repository config.
pub fn list_remotes(git_dir: &Path) -> Result<Vec<String>, MuonGitError> {
    let config_path = git_dir.join("config");
    let config = Config::load(&config_path)?;
    let mut names = Vec::new();

    for (section, key, _) in config.all_entries() {
        let s_lower = section.to_lowercase();
        if s_lower.starts_with("remote.") && key.to_lowercase() == "url" {
            let name = &section["remote.".len()..];
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }

    names.dedup();
    Ok(names)
}

/// Get a remote by name from the repository config.
pub fn get_remote(git_dir: &Path, name: &str) -> Result<Remote, MuonGitError> {
    let config_path = git_dir.join("config");
    let config = Config::load(&config_path)?;
    let section = format!("remote.{}", name);

    let url = config
        .get(&section, "url")
        .ok_or_else(|| MuonGitError::NotFound(format!("remote '{}' not found", name)))?
        .to_string();

    let push_url = config.get(&section, "pushurl").map(|s| s.to_string());

    let mut fetch_refspecs = Vec::new();
    for (s, k, v) in config.all_entries() {
        if s.to_lowercase() == section.to_lowercase() && k.to_lowercase() == "fetch" {
            fetch_refspecs.push(v.clone());
        }
    }

    Ok(Remote {
        name: name.to_string(),
        url,
        push_url,
        fetch_refspecs,
    })
}

/// Add a new remote to the repository config.
pub fn add_remote(git_dir: &Path, name: &str, url: &str) -> Result<Remote, MuonGitError> {
    let config_path = git_dir.join("config");
    let mut config = Config::load(&config_path)?;
    let section = format!("remote.{}", name);

    // Check if remote already exists
    if config.get(&section, "url").is_some() {
        return Err(MuonGitError::Invalid(format!(
            "remote '{}' already exists",
            name
        )));
    }

    let fetch_refspec = format!("+refs/heads/*:refs/remotes/{}/*", name);
    config.set(&section, "url", url);
    config.set(&section, "fetch", &fetch_refspec);
    config.save()?;

    Ok(Remote {
        name: name.to_string(),
        url: url.to_string(),
        push_url: None,
        fetch_refspecs: vec![fetch_refspec],
    })
}

/// Remove a remote from the repository config.
pub fn remove_remote(git_dir: &Path, name: &str) -> Result<(), MuonGitError> {
    let config_path = git_dir.join("config");
    let mut config = Config::load(&config_path)?;
    let section = format!("remote.{}", name);

    if config.get(&section, "url").is_none() {
        return Err(MuonGitError::NotFound(format!(
            "remote '{}' not found",
            name
        )));
    }

    config.unset(&section, "url");
    config.unset(&section, "pushurl");
    config.unset(&section, "fetch");
    config.save()?;

    Ok(())
}

/// Rename a remote in the repository config.
pub fn rename_remote(git_dir: &Path, old_name: &str, new_name: &str) -> Result<(), MuonGitError> {
    let remote = get_remote(git_dir, old_name)?;

    let config_path = git_dir.join("config");
    let mut config = Config::load(&config_path)?;

    let old_section = format!("remote.{}", old_name);
    let new_section = format!("remote.{}", new_name);

    // Check new name doesn't exist
    if config.get(&new_section, "url").is_some() {
        return Err(MuonGitError::Invalid(format!(
            "remote '{}' already exists",
            new_name
        )));
    }

    // Remove old entries
    config.unset(&old_section, "url");
    config.unset(&old_section, "pushurl");
    config.unset(&old_section, "fetch");

    // Add new entries
    config.set(&new_section, "url", &remote.url);
    if let Some(push_url) = &remote.push_url {
        config.set(&new_section, "pushurl", push_url);
    }
    // Update refspec to use new remote name
    let new_fetch = format!("+refs/heads/*:refs/remotes/{}/*", new_name);
    config.set(&new_section, "fetch", &new_fetch);
    config.save()?;

    Ok(())
}

/// Parse a refspec string into its components.
/// Format: [+]<src>:<dst>
/// Returns (force, src, dst).
pub fn parse_refspec(refspec: &str) -> Option<(bool, &str, &str)> {
    let (force, rest) = if let Some(stripped) = refspec.strip_prefix('+') {
        (true, stripped)
    } else {
        (false, refspec)
    };

    let colon = rest.find(':')?;
    let src = &rest[..colon];
    let dst = &rest[colon + 1..];
    Some((force, src, dst))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Repository;

    #[test]
    fn test_add_and_get_remote() {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_remote_add");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let remote = add_remote(
            repo.git_dir(),
            "origin",
            "https://example.com/repo.git",
        )
        .unwrap();
        assert_eq!(remote.name, "origin");
        assert_eq!(remote.url, "https://example.com/repo.git");
        assert_eq!(remote.fetch_refspecs.len(), 1);
        assert_eq!(
            remote.fetch_refspecs[0],
            "+refs/heads/*:refs/remotes/origin/*"
        );

        let loaded = get_remote(repo.git_dir(), "origin").unwrap();
        assert_eq!(loaded.url, "https://example.com/repo.git");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_list_remotes() {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_remote_list");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        add_remote(repo.git_dir(), "origin", "https://example.com/repo.git").unwrap();
        add_remote(repo.git_dir(), "upstream", "https://example.com/upstream.git").unwrap();

        let names = list_remotes(repo.git_dir()).unwrap();
        assert!(names.contains(&"origin".to_string()));
        assert!(names.contains(&"upstream".to_string()));
        assert_eq!(names.len(), 2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_remove_remote() {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_remote_rm");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        add_remote(repo.git_dir(), "origin", "https://example.com/repo.git").unwrap();
        assert!(get_remote(repo.git_dir(), "origin").is_ok());

        remove_remote(repo.git_dir(), "origin").unwrap();
        assert!(get_remote(repo.git_dir(), "origin").is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_rename_remote() {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_remote_rename");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        add_remote(repo.git_dir(), "origin", "https://example.com/repo.git").unwrap();
        rename_remote(repo.git_dir(), "origin", "upstream").unwrap();

        assert!(get_remote(repo.git_dir(), "origin").is_err());
        let remote = get_remote(repo.git_dir(), "upstream").unwrap();
        assert_eq!(remote.url, "https://example.com/repo.git");
        assert_eq!(
            remote.fetch_refspecs[0],
            "+refs/heads/*:refs/remotes/upstream/*"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_add_duplicate_remote() {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_remote_dup");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        add_remote(repo.git_dir(), "origin", "https://example.com/repo.git").unwrap();
        let result = add_remote(repo.git_dir(), "origin", "https://other.com/repo.git");
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_refspec() {
        let (force, src, dst) = parse_refspec("+refs/heads/*:refs/remotes/origin/*").unwrap();
        assert!(force);
        assert_eq!(src, "refs/heads/*");
        assert_eq!(dst, "refs/remotes/origin/*");

        let (force, src, dst) = parse_refspec("refs/heads/main:refs/heads/main").unwrap();
        assert!(!force);
        assert_eq!(src, "refs/heads/main");
        assert_eq!(dst, "refs/heads/main");

        assert!(parse_refspec("no-colon").is_none());
    }

    #[test]
    fn test_get_nonexistent_remote() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_remote_noexist");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        assert!(get_remote(repo.git_dir(), "nope").is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
