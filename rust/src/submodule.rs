//! Git submodule support
//! Parity: libgit2 src/libgit2/submodule.c

use std::path::Path;

use crate::config::Config;
use crate::error::MuonGitError;

/// A parsed submodule entry from .gitmodules.
#[derive(Debug, Clone, PartialEq)]
pub struct Submodule {
    /// Submodule name (from the section header).
    pub name: String,
    /// Path relative to the repository root.
    pub path: String,
    /// Remote URL.
    pub url: String,
    /// Branch to track (if specified).
    pub branch: Option<String>,
    /// Whether the submodule should be fetched shallowly.
    pub shallow: bool,
    /// Update strategy (checkout, rebase, merge, none).
    pub update: Option<String>,
    /// Whether fetchRecurseSubmodules is set.
    pub fetch_recurse: Option<bool>,
}

/// Parse a .gitmodules file and return all submodule entries.
pub fn parse_gitmodules(content: &str) -> Vec<Submodule> {
    let config = Config::parse(content);
    extract_submodules(&config)
}

/// Load submodules from a repository's .gitmodules file.
pub fn load_submodules(workdir: &Path) -> Result<Vec<Submodule>, MuonGitError> {
    let gitmodules_path = workdir.join(".gitmodules");
    if !gitmodules_path.exists() {
        return Ok(vec![]);
    }
    let config = Config::load(&gitmodules_path)?;
    Ok(extract_submodules(&config))
}

/// Get a specific submodule by name.
pub fn get_submodule(workdir: &Path, name: &str) -> Result<Submodule, MuonGitError> {
    let submodules = load_submodules(workdir)?;
    submodules
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| MuonGitError::NotFound(format!("submodule '{}'", name)))
}

/// Initialize submodule config in .git/config from .gitmodules.
/// Copies submodule entries from .gitmodules into the repo config.
pub fn submodule_init(
    git_dir: &Path,
    workdir: &Path,
    names: &[&str],
) -> Result<usize, MuonGitError> {
    let submodules = load_submodules(workdir)?;
    let config_path = git_dir.join("config");
    let mut config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        Config::new()
    };

    let mut count = 0;
    for sub in &submodules {
        if !names.is_empty() && !names.contains(&sub.name.as_str()) {
            continue;
        }
        let section = format!("submodule.{}", sub.name);
        // Only set URL if not already configured
        if config.get(&section, "url").is_none() {
            config.set(&section, "url", &sub.url);
            config.set(&section, "active", "true");
            count += 1;
        }
    }

    if count > 0 {
        // Save with path
        let mut config_with_path = Config::load(&config_path).unwrap_or_default();
        for sub in &submodules {
            if !names.is_empty() && !names.contains(&sub.name.as_str()) {
                continue;
            }
            let section = format!("submodule.{}", sub.name);
            if config_with_path.get(&section, "url").is_none() {
                config_with_path.set(&section, "url", &sub.url);
                config_with_path.set(&section, "active", "true");
            }
        }
        config_with_path.save()?;
    }

    Ok(count)
}

/// Write a .gitmodules file from a list of submodules.
pub fn write_gitmodules(workdir: &Path, submodules: &[Submodule]) -> Result<(), MuonGitError> {
    let mut content = String::new();
    for sub in submodules {
        content.push_str(&format!("[submodule \"{}\"]\n", sub.name));
        content.push_str(&format!("\tpath = {}\n", sub.path));
        content.push_str(&format!("\turl = {}\n", sub.url));
        if let Some(ref branch) = sub.branch {
            content.push_str(&format!("\tbranch = {}\n", branch));
        }
        if sub.shallow {
            content.push_str("\tshallow = true\n");
        }
        if let Some(ref update) = sub.update {
            content.push_str(&format!("\tupdate = {}\n", update));
        }
        if let Some(fetch_recurse) = sub.fetch_recurse {
            content.push_str(&format!(
                "\tfetchRecurseSubmodules = {}\n",
                if fetch_recurse { "true" } else { "false" }
            ));
        }
    }
    let path = workdir.join(".gitmodules");
    std::fs::write(&path, content)?;
    Ok(())
}

/// Extract submodule entries from a parsed Config.
fn extract_submodules(config: &Config) -> Vec<Submodule> {
    // Collect unique submodule names from sections like "submodule.NAME"
    let mut names: Vec<String> = Vec::new();
    for (section, _, _) in config.all_entries() {
        if let Some(rest) = section.strip_prefix("submodule.") {
            if !rest.is_empty() && !names.contains(&rest.to_string()) {
                names.push(rest.to_string());
            }
        }
    }

    let mut submodules = Vec::new();
    for name in &names {
        let section = format!("submodule.{}", name);
        let path = config.get(&section, "path").unwrap_or("").to_string();
        let url = config.get(&section, "url").unwrap_or("").to_string();

        if path.is_empty() && url.is_empty() {
            continue;
        }

        let branch = config.get(&section, "branch").map(|s| s.to_string());
        let shallow = config
            .get_bool(&section, "shallow")
            .unwrap_or(false);
        let update = config.get(&section, "update").map(|s| s.to_string());
        let fetch_recurse = config.get_bool(&section, "fetchRecurseSubmodules");

        submodules.push(Submodule {
            name: name.clone(),
            path: if path.is_empty() { name.clone() } else { path },
            url,
            branch,
            shallow,
            update,
            fetch_recurse,
        });
    }

    submodules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Repository;

    #[test]
    fn test_parse_gitmodules() {
        let content = r#"[submodule "lib/foo"]
	path = lib/foo
	url = https://github.com/example/foo.git
[submodule "lib/bar"]
	path = lib/bar
	url = https://github.com/example/bar.git
	branch = develop
"#;
        let subs = parse_gitmodules(content);
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].name, "lib/foo");
        assert_eq!(subs[0].path, "lib/foo");
        assert_eq!(subs[0].url, "https://github.com/example/foo.git");
        assert_eq!(subs[0].branch, None);
        assert_eq!(subs[1].name, "lib/bar");
        assert_eq!(subs[1].path, "lib/bar");
        assert_eq!(subs[1].url, "https://github.com/example/bar.git");
        assert_eq!(subs[1].branch, Some("develop".into()));
    }

    #[test]
    fn test_parse_gitmodules_with_options() {
        let content = r#"[submodule "vendor/lib"]
	path = vendor/lib
	url = git@github.com:example/lib.git
	shallow = true
	update = rebase
	fetchRecurseSubmodules = false
"#;
        let subs = parse_gitmodules(content);
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].name, "vendor/lib");
        assert!(subs[0].shallow);
        assert_eq!(subs[0].update, Some("rebase".into()));
        assert_eq!(subs[0].fetch_recurse, Some(false));
    }

    #[test]
    fn test_parse_empty_gitmodules() {
        let subs = parse_gitmodules("");
        assert!(subs.is_empty());
    }

    #[test]
    fn test_load_submodules_no_file() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_submod_nofile");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let subs = load_submodules(repo.workdir().unwrap()).unwrap();
        assert!(subs.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_write_and_load_gitmodules() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_submod_write");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let workdir = repo.workdir().unwrap();

        let subs = vec![
            Submodule {
                name: "libs/core".into(),
                path: "libs/core".into(),
                url: "https://example.com/core.git".into(),
                branch: Some("main".into()),
                shallow: false,
                update: None,
                fetch_recurse: None,
            },
            Submodule {
                name: "vendor/ext".into(),
                path: "vendor/ext".into(),
                url: "https://example.com/ext.git".into(),
                branch: None,
                shallow: true,
                update: Some("merge".into()),
                fetch_recurse: Some(true),
            },
        ];

        write_gitmodules(workdir, &subs).unwrap();
        let loaded = load_submodules(workdir).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "libs/core");
        assert_eq!(loaded[0].url, "https://example.com/core.git");
        assert_eq!(loaded[0].branch, Some("main".into()));
        assert_eq!(loaded[1].name, "vendor/ext");
        assert!(loaded[1].shallow);
        assert_eq!(loaded[1].update, Some("merge".into()));
        assert_eq!(loaded[1].fetch_recurse, Some(true));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_get_submodule() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_submod_get");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let workdir = repo.workdir().unwrap();

        let subs = vec![Submodule {
            name: "mylib".into(),
            path: "lib/mylib".into(),
            url: "https://example.com/mylib.git".into(),
            branch: None,
            shallow: false,
            update: None,
            fetch_recurse: None,
        }];
        write_gitmodules(workdir, &subs).unwrap();

        let sub = get_submodule(workdir, "mylib").unwrap();
        assert_eq!(sub.path, "lib/mylib");
        assert_eq!(sub.url, "https://example.com/mylib.git");

        assert!(get_submodule(workdir, "nonexistent").is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_submodule_init() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_submod_init");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let workdir = repo.workdir().unwrap();

        let subs = vec![
            Submodule {
                name: "foo".into(),
                path: "foo".into(),
                url: "https://example.com/foo.git".into(),
                branch: None,
                shallow: false,
                update: None,
                fetch_recurse: None,
            },
            Submodule {
                name: "bar".into(),
                path: "bar".into(),
                url: "https://example.com/bar.git".into(),
                branch: None,
                shallow: false,
                update: None,
                fetch_recurse: None,
            },
        ];
        write_gitmodules(workdir, &subs).unwrap();

        let count = submodule_init(repo.git_dir(), workdir, &[]).unwrap();
        assert_eq!(count, 2);

        let config = Config::load(&repo.git_dir().join("config")).unwrap();
        assert_eq!(
            config.get("submodule.foo", "url"),
            Some("https://example.com/foo.git")
        );
        assert_eq!(
            config.get("submodule.bar", "url"),
            Some("https://example.com/bar.git")
        );

        // Re-init should not re-add
        let count2 = submodule_init(repo.git_dir(), workdir, &[]).unwrap();
        assert_eq!(count2, 0);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_submodule_init_selective() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_submod_init_sel");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let workdir = repo.workdir().unwrap();

        let subs = vec![
            Submodule {
                name: "a".into(),
                path: "a".into(),
                url: "https://example.com/a.git".into(),
                branch: None,
                shallow: false,
                update: None,
                fetch_recurse: None,
            },
            Submodule {
                name: "b".into(),
                path: "b".into(),
                url: "https://example.com/b.git".into(),
                branch: None,
                shallow: false,
                update: None,
                fetch_recurse: None,
            },
        ];
        write_gitmodules(workdir, &subs).unwrap();

        let count = submodule_init(repo.git_dir(), workdir, &["a"]).unwrap();
        assert_eq!(count, 1);

        let config = Config::load(&repo.git_dir().join("config")).unwrap();
        assert_eq!(
            config.get("submodule.a", "url"),
            Some("https://example.com/a.git")
        );
        assert!(config.get("submodule.b", "url").is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
