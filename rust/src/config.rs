//! Git config file read/write
//! Parity: libgit2 src/libgit2/config_file.c

use std::path::{Path, PathBuf};

use crate::error::MuonGitError;

/// A parsed git config file
pub struct Config {
    entries: Vec<(String, String, String)>, // (section, key, value)
    path: Option<PathBuf>,
}

impl Config {
    /// Create an empty in-memory config
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            path: None,
        }
    }

    /// Load a config file from disk
    pub fn load(path: &Path) -> Result<Self, MuonGitError> {
        let content = std::fs::read_to_string(path)?;
        let entries = parse_config(&content);
        Ok(Self {
            entries,
            path: Some(path.to_path_buf()),
        })
    }

    /// Get a config value by section and key
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let s_lower = section.to_lowercase();
        let k_lower = key.to_lowercase();
        self.entries
            .iter()
            .rev()
            .find(|(s, k, _)| s.to_lowercase() == s_lower && k.to_lowercase() == k_lower)
            .map(|(_, _, v)| v.as_str())
    }

    /// Get a boolean config value
    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        let value = self.get(section, key)?;
        match value.to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" | "" => Some(false),
            _ => None,
        }
    }

    /// Get an integer config value (supports k/m/g suffixes)
    pub fn get_int(&self, section: &str, key: &str) -> Option<i64> {
        let value = self.get(section, key)?;
        parse_config_int(value)
    }

    /// Set a config value. Updates existing entry or appends new one.
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let s_lower = section.to_lowercase();
        let k_lower = key.to_lowercase();
        if let Some(idx) = self
            .entries
            .iter()
            .rposition(|(s, k, _)| s.to_lowercase() == s_lower && k.to_lowercase() == k_lower)
        {
            self.entries[idx] = (section.to_string(), key.to_string(), value.to_string());
        } else {
            self.entries
                .push((section.to_string(), key.to_string(), value.to_string()));
        }
    }

    /// Remove all entries matching section and key
    pub fn unset(&mut self, section: &str, key: &str) {
        let s_lower = section.to_lowercase();
        let k_lower = key.to_lowercase();
        self.entries
            .retain(|(s, k, _)| !(s.to_lowercase() == s_lower && k.to_lowercase() == k_lower));
    }

    /// Get all entries
    pub fn all_entries(&self) -> &[(String, String, String)] {
        &self.entries
    }

    /// Get entries in a given section
    pub fn entries_in_section(&self, section: &str) -> Vec<(&str, &str)> {
        let s_lower = section.to_lowercase();
        self.entries
            .iter()
            .filter(|(s, _, _)| s.to_lowercase() == s_lower)
            .map(|(_, k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    /// Serialize and write back to disk
    pub fn save(&self) -> Result<(), MuonGitError> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| MuonGitError::Invalid("config has no file path".into()))?;
        let content = serialize_config(&self.entries);
        std::fs::write(path, content)?;
        Ok(())
    }
}

fn parse_config(content: &str) -> Vec<(String, String, String)> {
    let mut result = Vec::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];
            if let Some(quote_idx) = inner.find('"') {
                let section_name = inner[..quote_idx].trim();
                let subsection = inner[quote_idx + 1..].replace('"', "");
                let subsection = subsection.trim();
                current_section = format!("{}.{}", section_name, subsection);
            } else {
                current_section = inner.trim().to_string();
            }
            continue;
        }

        if let Some(eq_idx) = trimmed.find('=') {
            let key = trimmed[..eq_idx].trim();
            let value = trimmed[eq_idx + 1..].trim();
            if !key.is_empty() {
                result.push((current_section.clone(), key.to_string(), value.to_string()));
            }
        } else if !trimmed.is_empty() {
            result.push((current_section.clone(), trimmed.to_string(), "true".to_string()));
        }
    }

    result
}

fn serialize_config(entries: &[(String, String, String)]) -> String {
    let mut buf = String::new();
    let mut current_section = String::new();

    for (section, key, value) in entries {
        if *section != current_section {
            current_section = section.clone();
            if let Some(dot_idx) = current_section.find('.') {
                let sec = &current_section[..dot_idx];
                let sub = &current_section[dot_idx + 1..];
                buf.push_str(&format!("[{} \"{}\"]\n", sec, sub));
            } else {
                buf.push_str(&format!("[{}]\n", current_section));
            }
        }
        buf.push_str(&format!("\t{} = {}\n", key, value));
    }

    buf
}

fn parse_config_int(s: &str) -> Option<i64> {
    let trimmed = s.trim().to_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    let last = trimmed.as_bytes().last()?;
    match last {
        b'k' => trimmed[..trimmed.len() - 1].parse::<i64>().ok().map(|n| n * 1024),
        b'm' => trimmed[..trimmed.len() - 1]
            .parse::<i64>()
            .ok()
            .map(|n| n * 1024 * 1024),
        b'g' => trimmed[..trimmed.len() - 1]
            .parse::<i64>()
            .ok()
            .map(|n| n * 1024 * 1024 * 1024),
        _ => trimmed.parse::<i64>().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_config() {
        let content = "[core]\n\tbare = false\n\trepositoryformatversion = 0\n";
        let config = Config {
            entries: parse_config(content),
            path: None,
        };
        assert_eq!(config.get("core", "bare"), Some("false"));
        assert_eq!(config.get_bool("core", "bare"), Some(false));
        assert_eq!(config.get_int("core", "repositoryformatversion"), Some(0));
    }

    #[test]
    fn test_parse_subsection() {
        let content = "[remote \"origin\"]\n\turl = https://example.com/repo.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n";
        let config = Config {
            entries: parse_config(content),
            path: None,
        };
        assert_eq!(
            config.get("remote.origin", "url"),
            Some("https://example.com/repo.git")
        );
        assert_eq!(
            config.get("remote.origin", "fetch"),
            Some("+refs/heads/*:refs/remotes/origin/*")
        );
    }

    #[test]
    fn test_set_and_unset() {
        let mut config = Config::new();
        config.set("core", "bare", "true");
        assert_eq!(config.get("core", "bare"), Some("true"));

        config.set("core", "bare", "false");
        assert_eq!(config.get("core", "bare"), Some("false"));

        config.unset("core", "bare");
        assert_eq!(config.get("core", "bare"), None);
    }

    #[test]
    fn test_case_insensitive() {
        let mut config = Config::new();
        config.set("Core", "Bare", "true");
        assert_eq!(config.get("core", "bare"), Some("true"));
        assert_eq!(config.get("CORE", "BARE"), Some("true"));
    }

    #[test]
    fn test_config_int_suffixes() {
        assert_eq!(parse_config_int("42"), Some(42));
        assert_eq!(parse_config_int("1k"), Some(1024));
        assert_eq!(parse_config_int("2m"), Some(2 * 1024 * 1024));
        assert_eq!(parse_config_int("1g"), Some(1024 * 1024 * 1024));
    }

    #[test]
    fn test_roundtrip_through_file() {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_config_rt");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let config_path = tmp.join("config");
        let mut config = Config {
            entries: Vec::new(),
            path: Some(config_path.clone()),
        };
        config.set("core", "bare", "false");
        config.set("core", "repositoryformatversion", "0");
        config.set("remote.origin", "url", "https://example.com/repo.git");
        config.save().unwrap();

        let loaded = Config::load(&config_path).unwrap();
        assert_eq!(loaded.get("core", "bare"), Some("false"));
        assert_eq!(
            loaded.get("remote.origin", "url"),
            Some("https://example.com/repo.git")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_repo_config() {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_config_repo");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let config_path = repo.git_dir().join("config");
        let config = Config::load(&config_path).unwrap();
        assert_eq!(config.get_bool("core", "bare"), Some(false));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
