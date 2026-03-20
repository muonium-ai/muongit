//! Reflog read/write
//! Parity: libgit2 src/libgit2/reflog.c

use std::fs;
use std::path::Path;

use crate::error::MuonGitError;
use crate::oid::OID;
use crate::types::Signature;

/// A single reflog entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    pub old_oid: OID,
    pub new_oid: OID,
    pub committer: Signature,
    pub message: String,
}

/// Read the reflog for a given reference name.
pub fn read_reflog(git_dir: &Path, ref_name: &str) -> Result<Vec<ReflogEntry>, MuonGitError> {
    let log_path = git_dir.join("logs").join(ref_name);
    if !log_path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&log_path)?;
    Ok(parse_reflog(&content))
}

/// Parse reflog file content into entries
fn parse_reflog(content: &str) -> Vec<ReflogEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let tab_idx = match trimmed.find('\t') {
            Some(i) => i,
            None => continue,
        };

        let sig_part = &trimmed[..tab_idx];
        let message = &trimmed[tab_idx + 1..];

        let mut parts = sig_part.splitn(3, ' ');
        let old_hex = match parts.next() {
            Some(s) => s,
            None => continue,
        };
        let new_hex = match parts.next() {
            Some(s) => s,
            None => continue,
        };
        let sig_str = match parts.next() {
            Some(s) => s,
            None => continue,
        };

        let old_oid = match OID::from_hex(old_hex) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let new_oid = match OID::from_hex(new_hex) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let committer = parse_sig(sig_str);

        entries.push(ReflogEntry {
            old_oid,
            new_oid,
            committer,
            message: message.to_string(),
        });
    }

    entries
}

/// Append an entry to the reflog for a given reference.
pub fn append_reflog(
    git_dir: &Path,
    ref_name: &str,
    old_oid: &OID,
    new_oid: &OID,
    committer: &Signature,
    message: &str,
) -> Result<(), MuonGitError> {
    let log_path = git_dir.join("logs").join(ref_name);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let line = format_reflog_entry(old_oid, new_oid, committer, message);

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Drop a reflog entry by index (0 = oldest). Returns the remaining entries.
/// If no entries remain, the reflog file is deleted.
pub fn drop_reflog_entry(
    git_dir: &Path,
    ref_name: &str,
    index: usize,
) -> Result<Vec<ReflogEntry>, MuonGitError> {
    let log_path = git_dir.join("logs").join(ref_name);
    let mut entries = read_reflog(git_dir, ref_name)?;

    if index >= entries.len() {
        return Err(MuonGitError::NotFound(format!(
            "reflog entry {} not found for {}",
            index, ref_name
        )));
    }

    entries.remove(index);

    if entries.is_empty() {
        let _ = fs::remove_file(&log_path);
    } else {
        let mut content = String::new();
        for entry in &entries {
            content.push_str(&format_reflog_entry(
                &entry.old_oid,
                &entry.new_oid,
                &entry.committer,
                &entry.message,
            ));
        }
        fs::write(&log_path, content)?;
    }

    Ok(entries)
}

fn format_reflog_entry(old_oid: &OID, new_oid: &OID, committer: &Signature, message: &str) -> String {
    let sign = if committer.offset >= 0 { "+" } else { "-" };
    let abs = committer.offset.unsigned_abs();
    let hours = abs / 60;
    let minutes = abs % 60;
    format!(
        "{} {} {} <{}> {} {}{:02}{:02}\t{}\n",
        old_oid.hex(),
        new_oid.hex(),
        committer.name,
        committer.email,
        committer.time,
        sign,
        hours,
        minutes,
        message
    )
}

fn parse_sig(s: &str) -> Signature {
    let email_start = match s.find('<') {
        Some(i) => i,
        None => return Signature { name: s.to_string(), email: String::new(), time: 0, offset: 0 },
    };
    let email_end = match s.find('>') {
        Some(i) => i,
        None => return Signature { name: s.to_string(), email: String::new(), time: 0, offset: 0 },
    };

    let name = s[..email_start].trim().to_string();
    let email = s[email_start + 1..email_end].to_string();
    let remainder = s[email_end + 1..].trim();
    let parts: Vec<&str> = remainder.split_whitespace().collect();

    let time = parts.first().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let offset = parts.get(1).map(|s| parse_tz(s)).unwrap_or(0);

    Signature { name, email, time, offset }
}

fn parse_tz(s: &str) -> i32 {
    if s.len() < 5 { return 0; }
    let sign: i32 = if s.starts_with('-') { -1 } else { 1 };
    let digits = &s[1..];
    if digits.len() != 4 { return 0; }
    let hours: i32 = digits[..2].parse().unwrap_or(0);
    let minutes: i32 = digits[2..].parse().unwrap_or(0);
    sign * (hours * 60 + minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_reflog_entry() {
        let content = "0000000000000000000000000000000000000000 aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d Test <test@test.com> 1234567890 +0000\tcommit (initial): first commit\n";
        let entries = parse_reflog(content);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].old_oid.is_zero());
        assert_eq!(entries[0].new_oid.hex(), "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
        assert_eq!(entries[0].committer.name, "Test");
        assert_eq!(entries[0].message, "commit (initial): first commit");
    }

    #[test]
    fn test_append_and_read_reflog() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_reflog_rw");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let zero = OID::zero();
        let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let sig = Signature { name: "Test".into(), email: "t@t.com".into(), time: 100, offset: 0 };

        append_reflog(repo.git_dir(), "HEAD", &zero, &oid1, &sig, "commit (initial): first").unwrap();
        append_reflog(repo.git_dir(), "HEAD", &oid1, &oid2, &sig, "commit: second").unwrap();

        let entries = read_reflog(repo.git_dir(), "HEAD").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].old_oid.is_zero());
        assert_eq!(entries[0].new_oid, oid1);
        assert_eq!(entries[0].message, "commit (initial): first");
        assert_eq!(entries[1].old_oid, oid1);
        assert_eq!(entries[1].new_oid, oid2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_nonexistent_reflog() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_reflog_empty");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let entries = read_reflog(repo.git_dir(), "HEAD").unwrap();
        assert!(entries.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_reflog_for_branch() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_reflog_branch");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let sig = Signature { name: "T".into(), email: "t@t".into(), time: 0, offset: 0 };

        append_reflog(repo.git_dir(), "refs/heads/main", &OID::zero(), &oid, &sig, "branch: Created").unwrap();

        let entries = read_reflog(repo.git_dir(), "refs/heads/main").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "branch: Created");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
