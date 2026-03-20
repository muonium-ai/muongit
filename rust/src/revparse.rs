//! Commit-oriented revision parsing for common Git revision expressions.
//! Parity target: libgit2 `git_revparse_single` / `git_revparse`

use std::path::Path;

use crate::commit::Commit;
use crate::error::MuonGitError;
use crate::object::{read_object, GitObject};
use crate::refs::resolve_reference;
use crate::OID;

/// A parsed revision expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevSpec {
    pub from: Option<OID>,
    pub to: Option<OID>,
    pub is_range: bool,
    pub uses_merge_base: bool,
}

/// Resolve a common revision expression to a commit OID.
///
/// Supported subset:
/// - full OIDs
/// - refs and short refs like `main`, `tags/v1`, `origin/main`
/// - `HEAD^`, `HEAD^N`, `HEAD~N`
pub fn resolve_revision(git_dir: &Path, spec: &str) -> Result<OID, MuonGitError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(MuonGitError::InvalidSpec("empty revision spec".into()));
    }
    if trimmed.contains("...") || is_two_dot_range(trimmed) {
        return Err(MuonGitError::InvalidSpec(format!(
            "range '{}' does not resolve to a single revision",
            trimmed
        )));
    }

    let (base_spec, suffix) = split_base_and_suffix(trimmed)?;
    let mut current = read_object(git_dir, &resolve_base_oid(git_dir, base_spec)?)?;

    let mut idx = 0usize;
    let suffix_bytes = suffix.as_bytes();
    while idx < suffix_bytes.len() {
        match suffix_bytes[idx] {
            b'~' => {
                idx += 1;
                let start = idx;
                while idx < suffix_bytes.len() && suffix_bytes[idx].is_ascii_digit() {
                    idx += 1;
                }
                let count = if start == idx {
                    1usize
                } else {
                    suffix[start..idx].parse::<usize>().map_err(|_| {
                        MuonGitError::InvalidSpec(format!(
                            "invalid ancestry operator in '{}'",
                            trimmed
                        ))
                    })?
                };
                for _ in 0..count {
                    let commit = peel_to_commit(git_dir, &current, trimmed)?;
                    let parent = commit.parent_ids.first().ok_or_else(|| {
                        MuonGitError::InvalidSpec(format!(
                            "revision '{}' has no first parent",
                            trimmed
                        ))
                    })?;
                    current = read_object(git_dir, parent)?;
                }
            }
            b'^' => {
                idx += 1;
                let start = idx;
                while idx < suffix_bytes.len() && suffix_bytes[idx].is_ascii_digit() {
                    idx += 1;
                }
                let parent_index = if start == idx {
                    1usize
                } else {
                    suffix[start..idx].parse::<usize>().map_err(|_| {
                        MuonGitError::InvalidSpec(format!(
                            "invalid parent selector in '{}'",
                            trimmed
                        ))
                    })?
                };
                let commit = peel_to_commit(git_dir, &current, trimmed)?;
                if parent_index == 0 {
                    current = read_object(git_dir, &commit.oid)?;
                    continue;
                }
                let parent = commit.parent_ids.get(parent_index - 1).ok_or_else(|| {
                    MuonGitError::InvalidSpec(format!(
                        "revision '{}' has no parent {}",
                        trimmed, parent_index
                    ))
                })?;
                current = read_object(git_dir, parent)?;
            }
            _ => {
                return Err(MuonGitError::InvalidSpec(format!(
                    "unsupported revision syntax '{}'",
                    trimmed
                )));
            }
        }
    }

    Ok(peel_to_commit(git_dir, &current, trimmed)?.oid)
}

/// Resolve a common revision expression to a commit object.
pub fn revparse_single(git_dir: &Path, spec: &str) -> Result<GitObject, MuonGitError> {
    let oid = resolve_revision(git_dir, spec)?;
    read_object(git_dir, &oid)
}

/// Parse a common revision expression into either a single commit or a range.
pub fn revparse(git_dir: &Path, spec: &str) -> Result<RevSpec, MuonGitError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(MuonGitError::InvalidSpec("empty revision spec".into()));
    }

    if let Some((from, to)) = split_range(trimmed, "...") {
        return Ok(RevSpec {
            from: Some(resolve_revision(git_dir, from)?),
            to: Some(resolve_revision(git_dir, to)?),
            is_range: true,
            uses_merge_base: true,
        });
    }

    if let Some((from, to)) = split_two_dot_range(trimmed) {
        return Ok(RevSpec {
            from: Some(resolve_revision(git_dir, from)?),
            to: Some(resolve_revision(git_dir, to)?),
            is_range: true,
            uses_merge_base: false,
        });
    }

    Ok(RevSpec {
        from: None,
        to: Some(resolve_revision(git_dir, trimmed)?),
        is_range: false,
        uses_merge_base: false,
    })
}

pub(crate) fn read_commit(git_dir: &Path, oid: &OID) -> Result<Commit, MuonGitError> {
    read_object(git_dir, oid)?.as_commit().map_err(|_| {
        MuonGitError::InvalidSpec(format!("revision '{}' is not a commit", oid.hex()))
    })
}

fn resolve_base_oid(git_dir: &Path, spec: &str) -> Result<OID, MuonGitError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(MuonGitError::InvalidSpec("missing base revision".into()));
    }

    if looks_like_full_oid(trimmed) {
        if let Ok(oid) = OID::from_hex(trimmed) {
            if read_object(git_dir, &oid).is_ok() {
                return Ok(oid);
            }
        }
    }

    for candidate in reference_candidates(trimmed) {
        if let Ok(oid) = resolve_reference(git_dir, &candidate) {
            return Ok(oid);
        }
    }

    Err(MuonGitError::NotFound(format!(
        "could not resolve revision '{}'",
        trimmed
    )))
}

fn peel_to_commit(
    git_dir: &Path,
    object: &GitObject,
    spec: &str,
) -> Result<Commit, MuonGitError> {
    let peeled = if object.obj_type == crate::ObjectType::Tag {
        object.peel(git_dir)?
    } else {
        object.clone()
    };

    peeled.as_commit().map_err(|_| {
        MuonGitError::InvalidSpec(format!(
            "revision '{}' does not resolve to a commit",
            spec
        ))
    })
}

fn split_base_and_suffix(spec: &str) -> Result<(&str, &str), MuonGitError> {
    let first_suffix = spec.find(['^', '~']).unwrap_or(spec.len());
    let base = &spec[..first_suffix];
    if base.is_empty() {
        return Err(MuonGitError::InvalidSpec(format!(
            "missing base revision in '{}'",
            spec
        )));
    }
    Ok((base, &spec[first_suffix..]))
}

fn split_range<'a>(spec: &'a str, operator: &str) -> Option<(&'a str, &'a str)> {
    let idx = spec.find(operator)?;
    let left = spec[..idx].trim();
    let right = spec[idx + operator.len()..].trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((left, right))
}

fn split_two_dot_range(spec: &str) -> Option<(&str, &str)> {
    if spec.contains("...") {
        return None;
    }
    split_range(spec, "..")
}

fn is_two_dot_range(spec: &str) -> bool {
    split_two_dot_range(spec).is_some()
}

fn looks_like_full_oid(spec: &str) -> bool {
    spec.len() == 40 && spec.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

fn reference_candidates(spec: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    candidates.push(spec.to_string());

    if !spec.starts_with("refs/") {
        candidates.push(format!("refs/{}", spec));
        candidates.push(format!("refs/heads/{}", spec));
        candidates.push(format!("refs/tags/{}", spec));
        candidates.push(format!("refs/remotes/{}", spec));
    }

    candidates
}
