//! Git notes: metadata annotations on commits
//! Parity: libgit2 src/libgit2/notes.c

use std::path::Path;

use crate::commit::parse_commit;
use crate::error::MuonGitError;
use crate::odb::read_loose_object;
use crate::oid::OID;
use crate::refs::resolve_reference;
use crate::tree::parse_tree;
use crate::types::ObjectType;

/// Default notes reference.
pub const DEFAULT_NOTES_REF: &str = "refs/notes/commits";

/// A git note attached to an object.
#[derive(Debug, Clone)]
pub struct Note {
    pub note_oid: OID,
    pub annotated_oid: OID,
    pub message: String,
}

/// Read a note for a specific object.
pub fn note_read(
    git_dir: &Path,
    notes_ref: Option<&str>,
    target_oid: &OID,
) -> Result<Note, MuonGitError> {
    let r = notes_ref.unwrap_or(DEFAULT_NOTES_REF);
    let notes_commit_oid = resolve_reference(git_dir, r)?;
    let (obj_type, data) = read_loose_object(git_dir, &notes_commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject(
            "notes ref not a commit".into(),
        ));
    }
    let commit = parse_commit(notes_commit_oid, &data)?;
    let note_oid = find_note_in_tree(git_dir, &commit.tree_id, &target_oid.hex)?;

    let (blob_type, blob_data) = read_loose_object(git_dir, &note_oid)?;
    if blob_type != ObjectType::Blob {
        return Err(MuonGitError::InvalidObject("note is not a blob".into()));
    }
    let message = String::from_utf8(blob_data)
        .map_err(|_| MuonGitError::InvalidObject("note is not valid UTF-8".into()))?;

    Ok(Note {
        note_oid,
        annotated_oid: target_oid.clone(),
        message,
    })
}

/// List all notes under a notes ref.
pub fn note_list(
    git_dir: &Path,
    notes_ref: Option<&str>,
) -> Result<Vec<(OID, OID)>, MuonGitError> {
    let r = notes_ref.unwrap_or(DEFAULT_NOTES_REF);
    let notes_commit_oid = resolve_reference(git_dir, r)?;
    let (obj_type, data) = read_loose_object(git_dir, &notes_commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject(
            "notes ref not a commit".into(),
        ));
    }
    let commit = parse_commit(notes_commit_oid, &data)?;

    let mut notes = Vec::new();
    collect_notes_from_tree(git_dir, &commit.tree_id, "", &mut notes)?;
    Ok(notes)
}

fn find_note_in_tree(
    git_dir: &Path,
    tree_oid: &OID,
    target_hex: &str,
) -> Result<OID, MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, tree_oid)?;
    if obj_type != ObjectType::Tree {
        return Err(MuonGitError::InvalidObject("expected tree".into()));
    }
    let tree = parse_tree(tree_oid.clone(), &data)?;

    if target_hex.len() >= 2 {
        let prefix = &target_hex[..2];
        let rest = &target_hex[2..];
        for entry in &tree.entries {
            if entry.name == prefix && entry.mode == 0o040000 {
                return find_note_in_tree(git_dir, &entry.oid, rest);
            }
        }
    }

    for entry in &tree.entries {
        if entry.name == target_hex {
            return Ok(entry.oid.clone());
        }
    }

    Err(MuonGitError::NotFound(format!(
        "no note found for {}",
        target_hex
    )))
}

fn collect_notes_from_tree(
    git_dir: &Path,
    tree_oid: &OID,
    prefix: &str,
    notes: &mut Vec<(OID, OID)>,
) -> Result<(), MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, tree_oid)?;
    if obj_type != ObjectType::Tree {
        return Ok(());
    }
    let tree = parse_tree(tree_oid.clone(), &data)?;

    for entry in &tree.entries {
        if entry.mode == 0o040000 {
            let new_prefix = format!("{}{}", prefix, entry.name);
            collect_notes_from_tree(git_dir, &entry.oid, &new_prefix, notes)?;
        } else {
            let full_hex = format!("{}{}", prefix, entry.name);
            if full_hex.len() == 40 {
                if let Ok(annotated_oid) = OID::from_hex(&full_hex) {
                    notes.push((entry.oid.clone(), annotated_oid));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::hash_blob;
    use crate::commit::serialize_commit;
    use crate::odb::write_loose_object;
    use crate::refs::{write_reference, write_symbolic_reference};
    use crate::repository::Repository;
    use crate::tree::{serialize_tree, TreeEntry};
    use crate::types::Signature;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        fs::create_dir_all(&base).unwrap();
        let p = base.join(format!("test_notes_{}", name));
        if p.exists() {
            fs::remove_dir_all(&p).unwrap();
        }
        p
    }

    fn make_sig() -> Signature {
        Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 1700000000,
            offset: 0,
        }
    }

    /// Create a repo with a notes tree pointing target_oid -> note_message
    fn setup_notes_repo(
        name: &str,
        target_oid: &OID,
        note_message: &str,
    ) -> (PathBuf, PathBuf) {
        let tmp = test_dir(name);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let gd = repo.git_dir().to_path_buf();

        // Create blob for the note message
        let note_blob_data = note_message.as_bytes();
        let note_oid =
            write_loose_object(&gd, ObjectType::Blob, note_blob_data).unwrap();

        // Create tree with entry: target_oid.hex -> note blob
        let entry = TreeEntry {
            mode: 0o100644,
            name: target_oid.hex.clone(),
            oid: note_oid,
        };
        let tree_data = serialize_tree(&[entry]);
        let tree_oid = write_loose_object(&gd, ObjectType::Tree, &tree_data).unwrap();

        // Create notes commit
        let sig = make_sig();
        let commit_data = serialize_commit(&tree_oid, &[], &sig, &sig, "Notes added", None);
        let notes_commit =
            write_loose_object(&gd, ObjectType::Commit, &commit_data).unwrap();

        write_reference(&gd, DEFAULT_NOTES_REF, &notes_commit).unwrap();
        write_reference(&gd, "refs/heads/main", &notes_commit).unwrap();
        write_symbolic_reference(&gd, "HEAD", "refs/heads/main").unwrap();

        (tmp, gd)
    }

    #[test]
    fn test_note_read() {
        let target = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let (_, gd) = setup_notes_repo("read", &target, "This is a note");

        let note = note_read(&gd, None, &target).unwrap();
        assert_eq!(note.message, "This is a note");
        assert_eq!(note.annotated_oid, target);
    }

    #[test]
    fn test_note_read_not_found() {
        let target = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let other = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let (_, gd) = setup_notes_repo("read_nf", &target, "note");

        let result = note_read(&gd, None, &other);
        assert!(result.is_err());
    }

    #[test]
    fn test_note_list() {
        let target = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let (_, gd) = setup_notes_repo("list", &target, "note content");

        let notes = note_list(&gd, None).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].1, target);
    }

    #[test]
    fn test_note_list_no_ref() {
        let tmp = test_dir("list_no_ref");
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let gd = repo.git_dir();

        // No notes ref exists
        let result = note_list(gd, None);
        assert!(result.is_err());
    }
}
