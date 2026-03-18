use crate::MuonGitError;
use std::fs;
use std::path::{Path, PathBuf};

/// A Git repository
pub struct Repository {
    /// Path to the .git directory
    git_dir: PathBuf,
    /// Path to the working directory (None for bare repos)
    workdir: Option<PathBuf>,
    /// Whether this is a bare repository
    is_bare: bool,
}

impl Repository {
    /// Open an existing repository at the given path
    pub fn open(path: impl Into<String>) -> Result<Self, MuonGitError> {
        let path = PathBuf::from(path.into());

        // Check if path itself is a bare repo (has HEAD, objects/, refs/)
        if is_git_dir(&path) {
            return Ok(Self {
                git_dir: path,
                workdir: None,
                is_bare: true,
            });
        }

        // Check for .git directory
        let git_dir = path.join(".git");
        if is_git_dir(&git_dir) {
            return Ok(Self {
                git_dir,
                workdir: Some(path),
                is_bare: false,
            });
        }

        Err(MuonGitError::NotFound(format!(
            "could not find repository at '{}'",
            path.display()
        )))
    }

    /// Discover a repository by walking up from the given path
    pub fn discover(path: impl Into<String>) -> Result<Self, MuonGitError> {
        let mut current = PathBuf::from(path.into());

        loop {
            if let Ok(repo) = Self::open(current.to_string_lossy().into_owned()) {
                return Ok(repo);
            }

            if !current.pop() {
                break;
            }
        }

        Err(MuonGitError::NotFound(
            "could not find repository in any parent directory".into(),
        ))
    }

    /// Initialize a new repository
    pub fn init(path: impl Into<String>, bare: bool) -> Result<Self, MuonGitError> {
        let path = PathBuf::from(path.into());

        let git_dir = if bare {
            path.clone()
        } else {
            path.join(".git")
        };

        // Create directory structure
        fs::create_dir_all(&git_dir)?;
        fs::create_dir_all(git_dir.join("objects"))?;
        fs::create_dir_all(git_dir.join("refs").join("heads"))?;
        fs::create_dir_all(git_dir.join("refs").join("tags"))?;

        // Write HEAD
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n")?;

        // Write config
        let config = if bare {
            "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n"
        } else {
            "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n"
        };
        fs::write(git_dir.join("config"), config)?;

        Ok(Self {
            git_dir,
            workdir: if bare { None } else { Some(path) },
            is_bare: bare,
        })
    }

    /// Clone a repository from a URL
    pub fn clone(_url: &str, _path: impl Into<String>) -> Result<Self, MuonGitError> {
        todo!("implement clone - requires network transport")
    }

    /// Path to the .git directory
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Path to the working directory (None for bare repos)
    pub fn workdir(&self) -> Option<&Path> {
        self.workdir.as_deref()
    }

    /// Whether this is a bare repository
    pub fn is_bare(&self) -> bool {
        self.is_bare
    }

    /// Read HEAD reference
    pub fn head(&self) -> Result<String, MuonGitError> {
        let head_path = self.git_dir.join("HEAD");
        let content = fs::read_to_string(&head_path).map_err(|_| {
            MuonGitError::NotFound("HEAD not found".into())
        })?;
        Ok(content.trim().to_string())
    }

    /// Check if HEAD is unborn (points to a ref that doesn't exist yet)
    pub fn head_unborn(&self) -> bool {
        match self.head() {
            Ok(head) => {
                if let Some(refname) = head.strip_prefix("ref: ") {
                    !self.git_dir.join(refname).exists()
                } else {
                    false
                }
            }
            Err(_) => true,
        }
    }
}

/// Check if a directory looks like a .git directory
fn is_git_dir(path: &Path) -> bool {
    path.join("HEAD").exists()
        && path.join("objects").is_dir()
        && path.join("refs").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(crate::version::STRING, "0.9.0");
        assert_eq!(crate::version::LIBGIT2_PARITY, "1.9.0");
    }

    #[test]
    fn test_init_and_open() {
        let tmp = std::env::temp_dir().join("muongit_test_init");
        let _ = fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        assert!(!repo.is_bare());
        assert!(repo.workdir().is_some());
        assert!(repo.git_dir().join("HEAD").exists());
        assert!(repo.head_unborn());

        // Reopen
        let repo2 = Repository::open(tmp.to_string_lossy().into_owned()).unwrap();
        assert!(!repo2.is_bare());
        assert_eq!(
            repo2.head().unwrap(),
            "ref: refs/heads/main"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_init_bare() {
        let tmp = std::env::temp_dir().join("muongit_test_bare");
        let _ = fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), true).unwrap();
        assert!(repo.is_bare());
        assert!(repo.workdir().is_none());

        // Reopen as bare
        let repo2 = Repository::open(tmp.to_string_lossy().into_owned()).unwrap();
        assert!(repo2.is_bare());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_open_nonexistent() {
        let result = Repository::open("/tmp/muongit_does_not_exist_12345");
        assert!(result.is_err());
    }

    #[test]
    fn test_discover() {
        let tmp = std::env::temp_dir().join("muongit_test_discover");
        let _ = fs::remove_dir_all(&tmp);

        let _repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let subdir = tmp.join("a").join("b").join("c");
        fs::create_dir_all(&subdir).unwrap();

        let found = Repository::discover(subdir.to_string_lossy().into_owned()).unwrap();
        assert!(!found.is_bare());

        let _ = fs::remove_dir_all(&tmp);
    }
}
