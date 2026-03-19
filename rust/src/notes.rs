//! Git notes — metadata annotations on commits
//! Parity: libgit2 src/libgit2/notes.c

use std::fs;
use std::path::Path;

use crate::commit::parse_commit;
use crate::error::MuonGitError;
use crate::odb::{read_loose_object, write_loose_object};
use crate::oid::OID;
use crate::refs::{read_reference, resolve_reference, write_reference};
use crate::tree::{parse_tree, serialize_tree, TreeEntry};
use crate::types::ObjectType;

/// Default notes reference
pub const DEFAULT_NOTES_REF: &str = "refs/notes/commits";

/// A git note attached to an object
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub note_oid: OID,
    pub annotated_oid: OID,
    pub message: String,
}

/// Read a note for a specific object
pub fn note_read(
    git_dir: &Path,
    notes_ref: Option<&str>,
    target_oid: &OID,
) -> Result<Note, MuonGitError> {
    let notes_ref = notes_ref.unwrap_or(DEFAULT_NOTES_REF);

    // Resolve notes ref to a commit
    let notes_commit_oid = resolve_reference(git_dir, notes_ref)?;
    let (obj_type, data) = read_loose_object(git_dir, &notes_commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject("notes ref not a commit".into()));
    }
    let commit = parse_commit(notes_commit_oid, &data)?;

    // Look up the note blob in the notes tree using fanout
    let note_oid = find_note_in_tree(git_dir, &commit.tree_id, &target_oid.hex())?;

    // Read the note blob
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

/// List all notes under a notes ref
pub fn note_list(
    git_dir: &Path,
    notes_ref: Option<&str>,
) -> Result<Vec<(OID, OID)>, MuonGitError> {
    let notes_ref = notes_ref.unwrap_or(DEFAULT_NOTES_REF);

    let notes_commit_oid = resolve_reference(git_dir, notes_ref)?;
    let (obj_type, data) = read_loose_object(git_dir, &notes_commit_oid)?;
    if obj_type != ObjectType::Commit {
        return Err(MuonGitError::InvalidObject("notes ref not a commit".into()));
    }
    let commit = parse_commit(notes_commit_oid, &data)?;

    let mut notes = Vec::new();
    collect_notes_from_tree(git_dir, &commit.tree_id, String::new(), &mut notes)?;
    Ok(notes)
}

/// Find a note blob OID in a notes tree using fanout path lookup
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

    // Try fanout: look for 2-char prefix directories
    if target_hex.len() >= 2 {
        let prefix = &target_hex[..2];
        let rest = &target_hex[2..];

        for entry in &tree.entries {
            if entry.name == prefix {
                if entry.mode == 0o040000 {
                    // Directory — recurse with remaining hex
                    return find_note_in_tree(git_dir, &entry.oid, rest);
                }
            }
            // Direct blob match (full remaining hex)
            if entry.name == target_hex {
                return Ok(entry.oid.clone());
            }
        }
    }

    // Try direct match for the full hex string
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

/// Recursively collect all (note_oid, annotated_oid) pairs from a notes tree
fn collect_notes_from_tree(
    git_dir: &Path,
    tree_oid: &OID,
    prefix: String,
    notes: &mut Vec<(OID, OID)>,
) -> Result<(), MuonGitError> {
    let (obj_type, data) = read_loose_object(git_dir, tree_oid)?;
    if obj_type != ObjectType::Tree {
        return Ok(());
    }
    let tree = parse_tree(tree_oid.clone(), &data)?;

    for entry in tree.entries {
        if entry.mode == 0o040000 {
            // Subtree — recurse with extended prefix
            let new_prefix = format!("{}{}", prefix, entry.name);
            collect_notes_from_tree(git_dir, &entry.oid, new_prefix, notes)?;
        } else {
            // Blob — this is a note. Reconstruct the annotated OID from path
            let full_hex = format!("{}{}", prefix, entry.name);
            if let Ok(annotated_oid) = OID::from_hex(&full_hex) {
                notes.push((entry.oid, annotated_oid));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::serialize_commit;
    use crate::tree::serialize_tree;
    use crate::types::Signature;

    fn test_sig() -> Signature {
        Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 1000000000,
            offset: 0,
        }
    }

    fn setup_repo(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp")
            .join(name);
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        let git_dir = base.join(".git");
        fs::create_dir_all(git_dir.join("objects")).unwrap();
        fs::create_dir_all(git_dir.join("refs/heads")).unwrap();
        fs::create_dir_all(git_dir.join("refs/notes")).unwrap();
        (base, git_dir)
    }

    #[test]
    fn test_note_read() {
        let (_base, git_dir) = setup_repo("notes_read");
        let sig = test_sig();

        // Create target commit
        let tree_oid = write_loose_object(&git_dir, ObjectType::Tree, &[]).unwrap();
        let commit_data = serialize_commit(&tree_oid, &[], &sig, &sig, "target", None);
        let target_oid = write_loose_object(&git_dir, ObjectType::Commit, &commit_data).unwrap();

        // Create note blob
        let note_msg = b"This is a note on the commit";
        let note_blob_oid = write_loose_object(&git_dir, ObjectType::Blob, note_msg).unwrap();

        // Build notes tree: use full hex as filename (no fanout for simplicity)
        let entries = vec![TreeEntry {
            mode: 0o100644,
            name: target_oid.hex(),
            oid: note_blob_oid.clone(),
        }];
        let tree_data = serialize_tree(&entries);
        let notes_tree_oid = write_loose_object(&git_dir, ObjectType::Tree, &tree_data).unwrap();

        // Create notes commit
        let notes_commit_data =
            serialize_commit(&notes_tree_oid, &[], &sig, &sig, "Notes added by 'git notes add'", None);
        let notes_commit_oid =
            write_loose_object(&git_dir, ObjectType::Commit, &notes_commit_data).unwrap();

        // Write refs/notes/commits
        write_reference(&git_dir, DEFAULT_NOTES_REF, &notes_commit_oid).unwrap();

        // Read note
        let note = note_read(&git_dir, None, &target_oid).unwrap();
        assert_eq!(note.message, "This is a note on the commit");
        assert_eq!(note.note_oid, note_blob_oid);
        assert_eq!(note.annotated_oid, target_oid);
    }

    #[test]
    fn test_note_read_with_fanout() {
        let (_base, git_dir) = setup_repo("notes_fanout");
        let sig = test_sig();

        // Create target commit
        let tree_oid = write_loose_object(&git_dir, ObjectType::Tree, &[]).unwrap();
        let commit_data = serialize_commit(&tree_oid, &[], &sig, &sig, "target", None);
        let target_oid = write_loose_object(&git_dir, ObjectType::Commit, &commit_data).unwrap();

        // Create note blob
        let note_blob_oid = write_loose_object(&git_dir, ObjectType::Blob, b"fanout note").unwrap();

        let hex = target_oid.hex();
        let prefix = &hex[..2];
        let rest = &hex[2..];

        // Build inner tree: rest of hex → blob
        let inner_entries = vec![TreeEntry {
            mode: 0o100644,
            name: rest.to_string(),
            oid: note_blob_oid.clone(),
        }];
        let inner_tree_data = serialize_tree(&inner_entries);
        let inner_tree_oid =
            write_loose_object(&git_dir, ObjectType::Tree, &inner_tree_data).unwrap();

        // Build outer tree: prefix → inner tree
        let outer_entries = vec![TreeEntry {
            mode: 0o040000,
            name: prefix.to_string(),
            oid: inner_tree_oid.clone(),
        }];
        let outer_tree_data = serialize_tree(&outer_entries);
        let notes_tree_oid =
            write_loose_object(&git_dir, ObjectType::Tree, &outer_tree_data).unwrap();

        // Create notes commit
        let notes_commit_data =
            serialize_commit(&notes_tree_oid, &[], &sig, &sig, "Notes", None);
        let notes_commit_oid =
            write_loose_object(&git_dir, ObjectType::Commit, &notes_commit_data).unwrap();
        write_reference(&git_dir, DEFAULT_NOTES_REF, &notes_commit_oid).unwrap();

        let note = note_read(&git_dir, None, &target_oid).unwrap();
        assert_eq!(note.message, "fanout note");
    }

    #[test]
    fn test_note_list() {
        let (_base, git_dir) = setup_repo("notes_list");
        let sig = test_sig();

        let tree_oid = write_loose_object(&git_dir, ObjectType::Tree, &[]).unwrap();
        let c1_data = serialize_commit(&tree_oid, &[], &sig, &sig, "c1", None);
        let c1 = write_loose_object(&git_dir, ObjectType::Commit, &c1_data).unwrap();
        let c2_data = serialize_commit(&tree_oid, &[c1.clone()], &sig, &sig, "c2", None);
        let c2 = write_loose_object(&git_dir, ObjectType::Commit, &c2_data).unwrap();

        let n1 = write_loose_object(&git_dir, ObjectType::Blob, b"note1").unwrap();
        let n2 = write_loose_object(&git_dir, ObjectType::Blob, b"note2").unwrap();

        let entries = vec![
            TreeEntry { mode: 0o100644, name: c1.hex(), oid: n1.clone() },
            TreeEntry { mode: 0o100644, name: c2.hex(), oid: n2.clone() },
        ];
        let tree_data = serialize_tree(&entries);
        let notes_tree = write_loose_object(&git_dir, ObjectType::Tree, &tree_data).unwrap();

        let notes_commit_data = serialize_commit(&notes_tree, &[], &sig, &sig, "Notes", None);
        let notes_commit = write_loose_object(&git_dir, ObjectType::Commit, &notes_commit_data).unwrap();
        write_reference(&git_dir, DEFAULT_NOTES_REF, &notes_commit).unwrap();

        let notes = note_list(&git_dir, None).unwrap();
        assert_eq!(notes.len(), 2);
    }

    #[test]
    fn test_note_not_found() {
        let (_base, git_dir) = setup_repo("notes_notfound");
        let sig = test_sig();

        let tree_oid = write_loose_object(&git_dir, ObjectType::Tree, &[]).unwrap();
        let commit_data = serialize_commit(&tree_oid, &[], &sig, &sig, "target", None);
        let target_oid = write_loose_object(&git_dir, ObjectType::Commit, &commit_data).unwrap();

        // Create empty notes tree + commit
        let empty_tree = write_loose_object(&git_dir, ObjectType::Tree, &[]).unwrap();
        let notes_commit_data = serialize_commit(&empty_tree, &[], &sig, &sig, "Notes", None);
        let notes_commit = write_loose_object(&git_dir, ObjectType::Commit, &notes_commit_data).unwrap();
        write_reference(&git_dir, DEFAULT_NOTES_REF, &notes_commit).unwrap();

        let result = note_read(&git_dir, None, &target_oid);
        assert!(result.is_err());
    }
}
