//! Structured unified patch generation, parsing, and worktree apply.

use std::fs;
use std::path::Path;

use crate::diff::{diff_lines, make_hunks, EditKind};
use crate::error::MuonGitError;
use crate::repository::Repository;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchFileStatus {
    Added,
    Deleted,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchLineKind {
    Context,
    Add,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchLine {
    pub kind: PatchLineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<PatchLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFile {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub status: PatchFileStatus,
    pub hunks: Vec<PatchHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Patch {
    pub files: Vec<PatchFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchReject {
    pub old_start: usize,
    pub new_start: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFileApplyResult {
    pub path: String,
    pub applied: bool,
    pub rejected_hunks: Vec<PatchReject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchApplyResult {
    pub files: Vec<PatchFileApplyResult>,
    pub has_rejects: bool,
}

impl Patch {
    pub fn parse(text: &str) -> Result<Self, MuonGitError> {
        parse_patch(text)
    }

    pub fn from_text(
        old_path: Option<&str>,
        new_path: Option<&str>,
        old_text: &str,
        new_text: &str,
        context: usize,
    ) -> Self {
        Self {
            files: vec![PatchFile::from_text(old_path, new_path, old_text, new_text, context)],
        }
    }

    pub fn format(&self) -> String {
        format_patch(self)
    }
}

impl PatchFile {
    pub fn from_text(
        old_path: Option<&str>,
        new_path: Option<&str>,
        old_text: &str,
        new_text: &str,
        context: usize,
    ) -> Self {
        let status = match (old_path, new_path) {
            (None, Some(_)) => PatchFileStatus::Added,
            (Some(_), None) => PatchFileStatus::Deleted,
            _ => PatchFileStatus::Modified,
        };

        let hunks = make_hunks(&diff_lines(old_text, new_text), context)
            .into_iter()
            .map(|hunk| PatchHunk {
                old_start: hunk.old_start,
                old_count: hunk.old_count,
                new_start: hunk.new_start,
                new_count: hunk.new_count,
                lines: hunk
                    .edits
                    .into_iter()
                    .map(|edit| PatchLine {
                        kind: match edit.kind {
                            EditKind::Equal => PatchLineKind::Context,
                            EditKind::Insert => PatchLineKind::Add,
                            EditKind::Delete => PatchLineKind::Delete,
                        },
                        text: edit.text,
                    })
                    .collect(),
            })
            .collect();

        Self {
            old_path: old_path.map(str::to_string),
            new_path: new_path.map(str::to_string),
            status,
            hunks,
        }
    }

    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or("")
    }
}

impl Repository {
    pub fn apply_patch(&self, patch: &Patch) -> Result<PatchApplyResult, MuonGitError> {
        let workdir = self.workdir().ok_or(MuonGitError::BareRepo)?;
        apply_patch_to_workdir(workdir, patch)
    }
}

pub fn parse_patch(text: &str) -> Result<Patch, MuonGitError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut files = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        let old_header = lines[index];
        if !old_header.starts_with("--- ") {
            return Err(MuonGitError::InvalidSpec(format!(
                "expected file header at line {}",
                index + 1
            )));
        }
        index += 1;
        if index >= lines.len() || !lines[index].starts_with("+++ ") {
            return Err(MuonGitError::InvalidSpec(format!(
                "missing new-file header after line {}",
                index
            )));
        }

        let old_path = parse_patch_path(&old_header[4..]);
        let new_path = parse_patch_path(&lines[index][4..]);
        index += 1;

        let status = match (&old_path, &new_path) {
            (None, Some(_)) => PatchFileStatus::Added,
            (Some(_), None) => PatchFileStatus::Deleted,
            _ => PatchFileStatus::Modified,
        };

        let mut hunks = Vec::new();
        while index < lines.len() && lines[index].starts_with("@@ ") {
            let (old_start, old_count, new_start, new_count) =
                parse_hunk_header(lines[index])?;
            index += 1;

            let mut old_seen = 0usize;
            let mut new_seen = 0usize;
            let mut patch_lines = Vec::new();

            while old_seen < old_count || new_seen < new_count {
                if index >= lines.len() {
                    return Err(MuonGitError::InvalidSpec(
                        "unexpected end of patch while reading hunk".into(),
                    ));
                }

                let line = lines[index];
                if line == r"\ No newline at end of file" {
                    index += 1;
                    continue;
                }

                let mut chars = line.chars();
                let marker = chars.next().ok_or_else(|| {
                    MuonGitError::InvalidSpec("empty hunk line".into())
                })?;
                let text = chars.collect::<String>();
                match marker {
                    ' ' => {
                        old_seen += 1;
                        new_seen += 1;
                        patch_lines.push(PatchLine {
                            kind: PatchLineKind::Context,
                            text,
                        });
                    }
                    '-' => {
                        old_seen += 1;
                        patch_lines.push(PatchLine {
                            kind: PatchLineKind::Delete,
                            text,
                        });
                    }
                    '+' => {
                        new_seen += 1;
                        patch_lines.push(PatchLine {
                            kind: PatchLineKind::Add,
                            text,
                        });
                    }
                    _ => {
                        return Err(MuonGitError::InvalidSpec(format!(
                            "unsupported hunk marker '{}' at line {}",
                            marker,
                            index + 1
                        )));
                    }
                }
                index += 1;
            }

            hunks.push(PatchHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                lines: patch_lines,
            });
        }

        files.push(PatchFile {
            old_path,
            new_path,
            status,
            hunks,
        });
    }

    Ok(Patch { files })
}

pub fn format_patch(patch: &Patch) -> String {
    let mut sections = Vec::new();
    for file in &patch.files {
        if file.hunks.is_empty() {
            continue;
        }

        let old_header = file
            .old_path
            .as_deref()
            .map(|path| format!("a/{}", path))
            .unwrap_or_else(|| "/dev/null".into());
        let new_header = file
            .new_path
            .as_deref()
            .map(|path| format!("b/{}", path))
            .unwrap_or_else(|| "/dev/null".into());

        let mut section = format!("--- {}\n+++ {}\n", old_header, new_header);
        for hunk in &file.hunks {
            section.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
            ));
            for line in &hunk.lines {
                let marker = match line.kind {
                    PatchLineKind::Context => ' ',
                    PatchLineKind::Add => '+',
                    PatchLineKind::Delete => '-',
                };
                section.push(marker);
                section.push_str(&line.text);
                section.push('\n');
            }
        }
        sections.push(section);
    }

    sections.join("")
}

pub fn apply_patch_to_workdir(
    workdir: &Path,
    patch: &Patch,
) -> Result<PatchApplyResult, MuonGitError> {
    let mut files = Vec::new();
    let mut has_rejects = false;

    for file in &patch.files {
        let rel_path = file.path().to_string();
        let target_path = workdir.join(&rel_path);
        let mut rejects = Vec::new();

        let original = match file.status {
            PatchFileStatus::Added => {
                if target_path.exists() {
                    rejects.push(file_level_reject("target file already exists"));
                }
                String::new()
            }
            PatchFileStatus::Deleted | PatchFileStatus::Modified => {
                if !target_path.exists() {
                    rejects.push(file_level_reject("target file does not exist"));
                    String::new()
                } else {
                    fs::read_to_string(&target_path).map_err(|_| {
                        MuonGitError::InvalidObject(format!(
                            "patch target '{}' is not valid UTF-8",
                            rel_path
                        ))
                    })?
                }
            }
        };

        if rejects.is_empty() {
            match apply_file_patch_to_text(&original, file) {
                Ok(updated) => match file.status {
                    PatchFileStatus::Deleted => {
                        if !updated.is_empty() {
                            rejects.push(file_level_reject(
                                "delete patch did not consume full file content",
                            ));
                        } else {
                            fs::remove_file(&target_path)?;
                        }
                    }
                    PatchFileStatus::Added | PatchFileStatus::Modified => {
                        if let Some(parent) = target_path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(&target_path, updated)?;
                    }
                },
                Err(hunk_rejects) => rejects.extend(hunk_rejects),
            }
        }

        if !rejects.is_empty() {
            has_rejects = true;
        }
        files.push(PatchFileApplyResult {
            path: rel_path,
            applied: rejects.is_empty(),
            rejected_hunks: rejects,
        });
    }

    Ok(PatchApplyResult { files, has_rejects })
}

fn apply_file_patch_to_text(
    original: &str,
    file: &PatchFile,
) -> Result<String, Vec<PatchReject>> {
    let mut lines = split_text(original);
    let mut offset: isize = 0;
    let mut rejects = Vec::new();

    for hunk in &file.hunks {
        let expected_old: Vec<String> = hunk
            .lines
            .iter()
            .filter(|line| line.kind != PatchLineKind::Add)
            .map(|line| line.text.clone())
            .collect();
        let replacement: Vec<String> = hunk
            .lines
            .iter()
            .filter(|line| line.kind != PatchLineKind::Delete)
            .map(|line| line.text.clone())
            .collect();

        let base_index = (hunk.old_start as isize - 1 + offset).max(0) as usize;
        if !matches_slice(&lines, base_index, &expected_old) {
            rejects.push(PatchReject {
                old_start: hunk.old_start,
                new_start: hunk.new_start,
                reason: "hunk context mismatch".into(),
            });
            continue;
        }

        lines.splice(
            base_index..base_index + expected_old.len(),
            replacement.iter().cloned(),
        );
        offset += replacement.len() as isize - expected_old.len() as isize;
    }

    if rejects.is_empty() {
        Ok(join_text(&lines))
    } else {
        Err(rejects)
    }
}

fn split_text(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').map(str::to_string).collect()
    }
}

fn join_text(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n")
    }
}

fn matches_slice(lines: &[String], index: usize, expected: &[String]) -> bool {
    index + expected.len() <= lines.len() && lines[index..index + expected.len()] == *expected
}

fn parse_patch_path(raw: &str) -> Option<String> {
    let token = raw.split_whitespace().next().unwrap_or(raw);
    if token == "/dev/null" {
        None
    } else if let Some(path) = token.strip_prefix("a/") {
        Some(path.to_string())
    } else if let Some(path) = token.strip_prefix("b/") {
        Some(path.to_string())
    } else {
        Some(token.to_string())
    }
}

fn parse_hunk_header(line: &str) -> Result<(usize, usize, usize, usize), MuonGitError> {
    let inner = line
        .strip_prefix("@@ -")
        .and_then(|rest| rest.strip_suffix(" @@"))
        .ok_or_else(|| MuonGitError::InvalidSpec(format!("invalid hunk header '{}'", line)))?;
    let (old_part, new_part) = inner
        .split_once(" +")
        .ok_or_else(|| MuonGitError::InvalidSpec(format!("invalid hunk header '{}'", line)))?;
    let (old_start, old_count) = parse_range(old_part)?;
    let (new_start, new_count) = parse_range(new_part)?;
    Ok((old_start, old_count, new_start, new_count))
}

fn parse_range(spec: &str) -> Result<(usize, usize), MuonGitError> {
    if let Some((start, count)) = spec.split_once(',') {
        Ok((
            start
                .parse()
                .map_err(|_| MuonGitError::InvalidSpec(format!("invalid range '{}'", spec)))?,
            count
                .parse()
                .map_err(|_| MuonGitError::InvalidSpec(format!("invalid range '{}'", spec)))?,
        ))
    } else {
        Ok((
            spec.parse()
                .map_err(|_| MuonGitError::InvalidSpec(format!("invalid range '{}'", spec)))?,
            1,
        ))
    }
}

fn file_level_reject(reason: &str) -> PatchReject {
    PatchReject {
        old_start: 0,
        new_start: 0,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name)
    }

    fn setup_repo(name: &str) -> (PathBuf, Repository) {
        let tmp = test_dir(name);
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        (tmp, repo)
    }

    #[test]
    fn test_patch_roundtrip_parse_and_format() {
        let patch = Patch::from_text(
            Some("file.txt"),
            Some("file.txt"),
            "line1\nline2\n",
            "line1\nline2 changed\nline3\n",
            3,
        );

        let text = patch.format();
        let reparsed = Patch::parse(&text).unwrap();
        assert_eq!(reparsed, patch);
    }

    #[test]
    fn test_apply_patch_modifies_existing_file() {
        let (tmp, repo) = setup_repo("patch_apply_modify");
        let path = repo.workdir().unwrap().join("file.txt");
        fs::write(&path, "line1\nline2\n").unwrap();

        let patch = Patch::from_text(
            Some("file.txt"),
            Some("file.txt"),
            "line1\nline2\n",
            "line1\nline2 changed\nline3\n",
            3,
        );
        let result = repo.apply_patch(&patch).unwrap();

        assert!(!result.has_rejects);
        assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2 changed\nline3\n");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_apply_patch_adds_new_file() {
        let (tmp, repo) = setup_repo("patch_apply_add");
        let patch = Patch::from_text(
            None,
            Some("nested/new.txt"),
            "",
            "hello\nworld\n",
            3,
        );

        let result = repo.apply_patch(&patch).unwrap();
        let path = repo.workdir().unwrap().join("nested").join("new.txt");
        assert!(!result.has_rejects);
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nworld\n");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_apply_patch_deletes_file() {
        let (tmp, repo) = setup_repo("patch_apply_delete");
        let path = repo.workdir().unwrap().join("gone.txt");
        fs::write(&path, "goodbye\nworld\n").unwrap();

        let patch = Patch::from_text(
            Some("gone.txt"),
            None,
            "goodbye\nworld\n",
            "",
            3,
        );

        let result = repo.apply_patch(&patch).unwrap();
        assert!(!result.has_rejects);
        assert!(!path.exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_apply_patch_rejects_context_mismatch() {
        let (tmp, repo) = setup_repo("patch_apply_reject");
        let path = repo.workdir().unwrap().join("file.txt");
        fs::write(&path, "line1\nDIFFERENT\n").unwrap();

        let patch = Patch::from_text(
            Some("file.txt"),
            Some("file.txt"),
            "line1\nline2\n",
            "line1\nline2 changed\n",
            3,
        );

        let result = repo.apply_patch(&patch).unwrap();
        assert!(result.has_rejects);
        assert_eq!(result.files.len(), 1);
        assert!(!result.files[0].applied);
        assert_eq!(result.files[0].rejected_hunks[0].reason, "hunk context mismatch");
        assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nDIFFERENT\n");

        let _ = fs::remove_dir_all(&tmp);
    }
}
