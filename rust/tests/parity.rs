//! libgit2 feature parity tests
//! Derived from libgit2 v1.9.0 test suite (tests/libgit2/)
//! Verifies edge cases and boundary conditions matching libgit2 behavior.

use muongit::*;
use muongit::sha1::SHA1;
use muongit::sha256::{SHA256, HashAlgorithm};
use muongit::commit::{serialize_commit, parse_commit, format_signature};
use muongit::tree::{TreeEntry, file_mode, serialize_tree, parse_tree};
use muongit::tag::{serialize_tag, parse_tag};
use muongit::config::Config;
use muongit::index::{Index, IndexEntry, write_index, read_index};
use muongit::diff::{diff_trees, DiffStatus};
use muongit::pack::apply_delta;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ── OID parity (libgit2 tests/libgit2/core/oid.c) ──

#[test]
fn parity_oid_from_valid_hex() {
    let oid = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    assert_eq!(oid.hex(), "ae90f12eea699729ed24555e40b9fd669da12a12");
}

#[test]
fn parity_oid_from_invalid_hex() {
    // Non-hex characters
    assert!(OID::from_hex("zz90f12eea699729ed24555e40b9fd669da12a12").is_err());
    // Empty string is technically valid (0-length OID)
    let empty = OID::from_hex("");
    assert!(empty.is_ok());
    assert_eq!(empty.unwrap().raw().len(), 0);
}

#[test]
fn parity_oid_zero_is_zero() {
    let z = OID::zero();
    assert!(z.is_zero());
    assert_eq!(z.hex(), "0000000000000000000000000000000000000000");
}

#[test]
fn parity_oid_nonzero_is_not_zero() {
    let oid = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    assert!(!oid.is_zero());
}

#[test]
fn parity_oid_equality() {
    let a = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    let b = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    let c = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn parity_oid_sha256_roundtrip() {
    let hex64 = "d3e63d2f2e43d1fee23a74bf19a0ede156cba2d1bd602eba13de433cea1bb512";
    let oid = OID::from_hex(hex64).unwrap();
    assert_eq!(oid.hex(), hex64);
    assert_eq!(oid.raw().len(), 32);
}

#[test]
fn parity_oid_sha1_vs_sha256_different() {
    let data = b"test content\n";
    let sha1_oid = OID::hash_object(ObjectType::Blob, data);
    let sha256_oid = OID::hash_object_sha256(ObjectType::Blob, data);
    assert_ne!(sha1_oid.hex(), sha256_oid.hex());
    assert_eq!(sha1_oid.hex().len(), 40);
    assert_eq!(sha256_oid.hex().len(), 64);
}

#[test]
fn parity_hash_algorithm_properties() {
    assert_eq!(HashAlgorithm::SHA1.digest_length(), 20);
    assert_eq!(HashAlgorithm::SHA256.digest_length(), 32);
    assert_eq!(HashAlgorithm::SHA1.hex_length(), 40);
    assert_eq!(HashAlgorithm::SHA256.hex_length(), 64);
    assert_ne!(HashAlgorithm::SHA1, HashAlgorithm::SHA256);
}

// ── Signature parity (libgit2 tests/libgit2/commit/signature.c) ──

#[test]
fn parity_signature_positive_offset() {
    let sig = Signature { name: "Test User".into(), email: "test@test.tt".into(), time: 1461698487, offset: 120 };
    assert_eq!(format_signature(&sig), "Test User <test@test.tt> 1461698487 +0200");
}

#[test]
fn parity_signature_negative_offset() {
    let sig = Signature { name: "Test".into(), email: "test@test.com".into(), time: 1000, offset: -300 };
    assert_eq!(format_signature(&sig), "Test <test@test.com> 1000 -0500");
}

#[test]
fn parity_signature_zero_offset() {
    let sig = Signature { name: "A".into(), email: "a@b.c".into(), time: 0, offset: 0 };
    assert_eq!(format_signature(&sig), "A <a@b.c> 0 +0000");
}

#[test]
fn parity_signature_large_offset() {
    // +1234 = 12 hours 34 minutes = 754 minutes
    let sig = Signature { name: "A".into(), email: "a@example.com".into(), time: 1461698487, offset: 754 };
    assert_eq!(format_signature(&sig), "A <a@example.com> 1461698487 +1234");
}

#[test]
fn parity_signature_single_char_name() {
    let sig = Signature { name: "x".into(), email: "x@y.z".into(), time: 100, offset: 0 };
    assert_eq!(format_signature(&sig), "x <x@y.z> 100 +0000");
}

// ── Commit parity (libgit2 tests/libgit2/object/validate.c) ──

#[test]
fn parity_commit_no_parents() {
    let tree_id = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
    let author = Signature { name: "Author".into(), email: "a@a.com".into(), time: 1638286404, offset: -300 };
    let committer = Signature { name: "Committer".into(), email: "c@c.com".into(), time: 1638324642, offset: -300 };

    let data = serialize_commit(&tree_id, &[], &author, &committer, "initial commit\n", None);
    let oid = OID::hash_object(ObjectType::Commit, &data);
    let parsed = parse_commit(oid, &data).unwrap();

    assert_eq!(parsed.tree_id, tree_id);
    assert!(parsed.parent_ids.is_empty());
    assert_eq!(parsed.author.name, "Author");
    assert_eq!(parsed.committer.email, "c@c.com");
    assert_eq!(parsed.message, "initial commit\n");
}

#[test]
fn parity_commit_multiple_parents() {
    let tree_id = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
    let parent1 = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    let parent2 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let sig = Signature { name: "M".into(), email: "m@m.com".into(), time: 1000, offset: 0 };

    let data = serialize_commit(&tree_id, &[parent1.clone(), parent2.clone()], &sig, &sig, "merge\n", None);
    let oid = OID::hash_object(ObjectType::Commit, &data);
    let parsed = parse_commit(oid, &data).unwrap();

    assert_eq!(parsed.parent_ids.len(), 2);
    assert_eq!(parsed.parent_ids[0], parent1);
    assert_eq!(parsed.parent_ids[1], parent2);
}

#[test]
fn parity_commit_with_encoding() {
    let tree_id = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
    let sig = Signature { name: "UTF8".into(), email: "u@u.com".into(), time: 0, offset: 0 };

    let data = serialize_commit(&tree_id, &[], &sig, &sig, "msg\n", Some("ISO-8859-1"));
    let oid = OID::hash_object(ObjectType::Commit, &data);
    let parsed = parse_commit(oid, &data).unwrap();

    assert_eq!(parsed.message_encoding.as_deref(), Some("ISO-8859-1"));
}

#[test]
fn parity_commit_roundtrip_preserves_oid() {
    let tree_id = OID::from_hex("bdd24e358576f1baa275df98cdcaf3ac9a3f4233").unwrap();
    let parent_id = OID::from_hex("d6d956f1d66210bfcd0484166befab33b5987a39").unwrap();
    let author = Signature { name: "Edward Thomson".into(), email: "ethomson@edwardthomson.com".into(), time: 1638286404, offset: -300 };
    let committer = Signature { name: "Edward Thomson".into(), email: "ethomson@edwardthomson.com".into(), time: 1638324642, offset: -300 };

    let data = serialize_commit(&tree_id, &[parent_id], &author, &committer, "commit go here.\n", None);
    let oid1 = OID::hash_object(ObjectType::Commit, &data);
    let parsed = parse_commit(oid1.clone(), &data).unwrap();

    // Re-serialize and hash again
    let data2 = serialize_commit(&parsed.tree_id, &parsed.parent_ids, &parsed.author, &parsed.committer, &parsed.message, parsed.message_encoding.as_deref());
    let oid2 = OID::hash_object(ObjectType::Commit, &data2);
    assert_eq!(oid1, oid2);
}

// ── Tree parity (libgit2 tests/libgit2/object/tree/parse.c) ──

#[test]
fn parity_tree_empty() {
    let data = serialize_tree(&[]);
    assert!(data.is_empty()); // Empty tree is zero bytes
    let oid = OID::hash_object(ObjectType::Tree, &data);
    let parsed = parse_tree(oid.clone(), &data).unwrap();
    assert!(parsed.entries.is_empty());
    // Verify empty tree produces consistent OID
    let oid2 = OID::hash_object(ObjectType::Tree, &[]);
    assert_eq!(oid, oid2);
}

#[test]
fn parity_tree_single_blob() {
    let oid = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    let entries = vec![TreeEntry { mode: file_mode::BLOB, name: "foo".into(), oid: oid.clone() }];
    let data = serialize_tree(&entries);
    let parsed = parse_tree(OID::zero(), &data).unwrap();
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].name, "foo");
    assert_eq!(parsed.entries[0].mode, file_mode::BLOB);
    assert_eq!(parsed.entries[0].oid, oid);
}

#[test]
fn parity_tree_single_subtree() {
    let oid = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    let entries = vec![TreeEntry { mode: file_mode::TREE, name: "subdir".into(), oid: oid.clone() }];
    let data = serialize_tree(&entries);
    let parsed = parse_tree(OID::zero(), &data).unwrap();
    assert_eq!(parsed.entries.len(), 1);
    assert!(parsed.entries[0].is_tree());
    assert!(!parsed.entries[0].is_blob());
}

#[test]
fn parity_tree_multiple_modes() {
    let oid1 = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    let oid2 = OID::from_hex("e8bfe5af39579a7e4898bb23f3a76a72c368cee6").unwrap();
    let entries = vec![
        TreeEntry { mode: file_mode::BLOB, name: "file.txt".into(), oid: oid1.clone() },
        TreeEntry { mode: file_mode::BLOB_EXE, name: "run.sh".into(), oid: oid2.clone() },
        TreeEntry { mode: file_mode::LINK, name: "sym".into(), oid: oid1.clone() },
        TreeEntry { mode: file_mode::TREE, name: "dir".into(), oid: oid2.clone() },
    ];
    let data = serialize_tree(&entries);
    let parsed = parse_tree(OID::zero(), &data).unwrap();
    // Entries should be sorted by name
    assert_eq!(parsed.entries.len(), 4);
    assert_eq!(parsed.entries[0].name, "dir");
    assert_eq!(parsed.entries[0].mode, file_mode::TREE);
    assert_eq!(parsed.entries[1].name, "file.txt");
    assert_eq!(parsed.entries[1].mode, file_mode::BLOB);
    assert_eq!(parsed.entries[2].name, "run.sh");
    assert_eq!(parsed.entries[2].mode, file_mode::BLOB_EXE);
    assert_eq!(parsed.entries[3].name, "sym");
    assert_eq!(parsed.entries[3].mode, file_mode::LINK);
}

#[test]
fn parity_tree_roundtrip_preserves_oid() {
    let oid = OID::from_hex("ce013625030ba8dba906f756967f9e9ca394464a").unwrap();
    let entries = vec![
        TreeEntry { mode: file_mode::BLOB, name: "hello.txt".into(), oid: oid.clone() },
        TreeEntry { mode: file_mode::BLOB_EXE, name: "script.sh".into(), oid: oid.clone() },
    ];
    let data1 = serialize_tree(&entries);
    let tree_oid1 = OID::hash_object(ObjectType::Tree, &data1);

    let parsed = parse_tree(tree_oid1.clone(), &data1).unwrap();
    let data2 = serialize_tree(&parsed.entries);
    let tree_oid2 = OID::hash_object(ObjectType::Tree, &data2);
    assert_eq!(tree_oid1, tree_oid2);
}

// ── Tag parity ──

#[test]
fn parity_tag_targeting_different_types() {
    let target = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    let tagger = Signature { name: "T".into(), email: "t@t".into(), time: 0, offset: 0 };

    for obj_type in &[ObjectType::Commit, ObjectType::Tree, ObjectType::Blob] {
        let data = serialize_tag(&target, *obj_type, "v1.0", Some(&tagger), "tag msg\n");
        let oid = OID::hash_object(ObjectType::Tag, &data);
        let parsed = parse_tag(oid, &data).unwrap();
        assert_eq!(parsed.target_type, *obj_type);
        assert_eq!(parsed.tag_name, "v1.0");
    }
}

#[test]
fn parity_tag_without_tagger() {
    let target = OID::from_hex("ae90f12eea699729ed24555e40b9fd669da12a12").unwrap();
    let data = serialize_tag(&target, ObjectType::Commit, "lightweight", None, "no tagger\n");
    let oid = OID::hash_object(ObjectType::Tag, &data);
    let parsed = parse_tag(oid, &data).unwrap();
    assert!(parsed.tagger.is_none());
    assert_eq!(parsed.tag_name, "lightweight");
}

// ── Config parity (libgit2 tests/libgit2/config/read.c) ──

#[test]
fn parity_config_boolean_values() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_parity_config_bool");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let config_path = tmp.join("config");
    std::fs::write(&config_path, "[core]\n\tfilemode = true\n\tbare = false\n\tyes = yes\n\tno = no\n\ton = on\n\toff = off\n\tone = 1\n\tzero = 0\n").unwrap();

    let cfg = Config::load(&config_path).unwrap();
    assert_eq!(cfg.get_bool("core", "filemode"), Some(true));
    assert_eq!(cfg.get_bool("core", "bare"), Some(false));
    assert_eq!(cfg.get_bool("core", "yes"), Some(true));
    assert_eq!(cfg.get_bool("core", "no"), Some(false));
    assert_eq!(cfg.get_bool("core", "on"), Some(true));
    assert_eq!(cfg.get_bool("core", "off"), Some(false));
    assert_eq!(cfg.get_bool("core", "one"), Some(true));
    assert_eq!(cfg.get_bool("core", "zero"), Some(false));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn parity_config_int_suffixes() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_parity_config_int");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let config_path = tmp.join("config");
    std::fs::write(&config_path, "[core]\n\tplain = 42\n\tkilo = 1k\n\tmega = 1m\n\tgiga = 1g\n").unwrap();

    let cfg = Config::load(&config_path).unwrap();
    assert_eq!(cfg.get_int("core", "plain"), Some(42));
    assert_eq!(cfg.get_int("core", "kilo"), Some(1024));
    assert_eq!(cfg.get_int("core", "mega"), Some(1048576));
    assert_eq!(cfg.get_int("core", "giga"), Some(1073741824));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn parity_config_case_insensitive_keys() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_parity_config_case");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let config_path = tmp.join("config");
    std::fs::write(&config_path, "[Core]\n\tFileMode = true\n").unwrap();

    let cfg = Config::load(&config_path).unwrap();
    // Section and key should be case-insensitive
    assert_eq!(cfg.get_bool("core", "filemode"), Some(true));
    assert_eq!(cfg.get_bool("CORE", "FILEMODE"), Some(true));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn parity_config_comments_ignored() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_parity_config_comments");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let config_path = tmp.join("config");
    std::fs::write(&config_path, "# This is a comment\n; So is this\n[core]\n\t# comment in section\n\tbare = false\n").unwrap();

    let cfg = Config::load(&config_path).unwrap();
    assert_eq!(cfg.get_bool("core", "bare"), Some(false));

    let _ = std::fs::remove_dir_all(&tmp);
}

// ── Blob parity ──

#[test]
fn parity_blob_empty_oid() {
    // Well-known: empty blob OID
    let oid = OID::hash_object(ObjectType::Blob, b"");
    assert_eq!(oid.hex(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
}

#[test]
fn parity_blob_known_content() {
    // Well-known: "hello\n" blob OID
    let oid = OID::hash_object(ObjectType::Blob, b"hello\n");
    assert_eq!(oid.hex(), "ce013625030ba8dba906f756967f9e9ca394464a");
}

#[test]
fn parity_blob_newline_only() {
    // libgit2 loose_data.h: single newline byte
    let oid = OID::hash_object(ObjectType::Blob, b"\n");
    assert_eq!(oid.hex(), "8b137891791fe96927ad78e64b0aad7bded08bdc");
}

// ── Index parity (libgit2 tests/libgit2/index/) ──

#[test]
fn parity_index_sorted_by_path() {
    let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let mut index = Index::new();
    // Add in reverse order
    index.add(IndexEntry {
        ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
        dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
        file_size: 0, oid: oid.clone(), flags: 0, path: "z.txt".into(),
    });
    index.add(IndexEntry {
        ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
        dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
        file_size: 0, oid: oid.clone(), flags: 0, path: "a.txt".into(),
    });
    index.add(IndexEntry {
        ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
        dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
        file_size: 0, oid: oid.clone(), flags: 0, path: "m/file.c".into(),
    });

    assert_eq!(index.entries[0].path, "a.txt");
    assert_eq!(index.entries[1].path, "m/file.c");
    assert_eq!(index.entries[2].path, "z.txt");
}

#[test]
fn parity_index_duplicate_path_replaces() {
    let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let oid2 = OID::from_hex("ce013625030ba8dba906f756967f9e9ca394464a").unwrap();
    let mut index = Index::new();

    index.add(IndexEntry {
        ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
        dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
        file_size: 10, oid: oid1.clone(), flags: 0, path: "file.txt".into(),
    });
    index.add(IndexEntry {
        ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
        dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
        file_size: 20, oid: oid2.clone(), flags: 0, path: "file.txt".into(),
    });

    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries[0].oid, oid2);
    assert_eq!(index.entries[0].file_size, 20);
}

#[test]
fn parity_index_many_entries_roundtrip() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_parity_index_many");
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

    let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let mut index = Index::new();
    for i in 0..100 {
        index.add(IndexEntry {
            ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
            dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
            file_size: i as u32, oid: oid.clone(), flags: 0, path: format!("file_{:04}.txt", i),
        });
    }
    write_index(repo.git_dir(), &index).unwrap();

    let loaded = read_index(repo.git_dir()).unwrap();
    assert_eq!(loaded.entries.len(), 100);
    // Verify sorted
    for i in 1..loaded.entries.len() {
        assert!(loaded.entries[i - 1].path < loaded.entries[i].path);
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

// ── Diff parity (libgit2 tests/libgit2/diff/tree.c) ──

#[test]
fn parity_diff_sorted_output() {
    let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

    let old = vec![
        TreeEntry { mode: file_mode::BLOB, name: "a.txt".into(), oid: oid1.clone() },
        TreeEntry { mode: file_mode::BLOB, name: "c.txt".into(), oid: oid1.clone() },
        TreeEntry { mode: file_mode::BLOB, name: "e.txt".into(), oid: oid1.clone() },
    ];
    let new = vec![
        TreeEntry { mode: file_mode::BLOB, name: "b.txt".into(), oid: oid2.clone() },
        TreeEntry { mode: file_mode::BLOB, name: "c.txt".into(), oid: oid2.clone() },
        TreeEntry { mode: file_mode::BLOB, name: "d.txt".into(), oid: oid2.clone() },
    ];

    let deltas = diff_trees(&old, &new);
    // Should be sorted by path
    let paths: Vec<&str> = deltas.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, vec!["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"]);

    assert_eq!(deltas[0].status, DiffStatus::Deleted);  // a.txt removed
    assert_eq!(deltas[1].status, DiffStatus::Added);    // b.txt added
    assert_eq!(deltas[2].status, DiffStatus::Modified);  // c.txt changed
    assert_eq!(deltas[3].status, DiffStatus::Added);    // d.txt added
    assert_eq!(deltas[4].status, DiffStatus::Deleted);  // e.txt removed
}

#[test]
fn parity_diff_mode_change_is_modified() {
    let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let old = vec![TreeEntry { mode: file_mode::BLOB, name: "f".into(), oid: oid.clone() }];
    let new = vec![TreeEntry { mode: file_mode::BLOB_EXE, name: "f".into(), oid: oid.clone() }];
    let deltas = diff_trees(&old, &new);
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].status, DiffStatus::Modified);
}

// ── Delta parity (libgit2 pack) ──

#[test]
fn parity_delta_empty_insert() {
    // Delta that inserts 3 bytes from scratch
    let base = b"base";
    let mut delta: Vec<u8> = vec![4, 3, 3]; // base_size=4, target_size=3, insert 3 bytes
    delta.extend_from_slice(b"new");
    let result = apply_delta(base, &delta).unwrap();
    assert_eq!(&result, b"new");
}

#[test]
fn parity_delta_invalid_opcode_zero() {
    let base = b"base";
    let delta: Vec<u8> = vec![4, 1, 0]; // opcode 0 is invalid
    assert!(apply_delta(base, &delta).is_err());
}

// ── SHA hash vectors (NIST) ──

#[test]
fn parity_sha1_nist_vectors() {
    // abc
    assert_eq!(hex(&SHA1::hash(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
    // empty
    assert_eq!(hex(&SHA1::hash(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
}

#[test]
fn parity_sha256_nist_vectors() {
    // abc
    assert_eq!(hex(&SHA256::hash(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    // empty
    assert_eq!(hex(&SHA256::hash(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

// ── Repository parity (libgit2 tests/libgit2/repo/) ──

#[test]
fn parity_repo_init_creates_structure() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_parity_repo_init");
    let _ = std::fs::remove_dir_all(&tmp);

    let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
    assert!(!repo.is_bare());
    assert!(std::path::Path::new(repo.git_dir()).join("HEAD").exists());
    assert!(std::path::Path::new(repo.git_dir()).join("objects").exists());
    assert!(std::path::Path::new(repo.git_dir()).join("refs").exists());

    // HEAD should point to refs/heads/main
    let head = std::fs::read_to_string(std::path::Path::new(repo.git_dir()).join("HEAD")).unwrap();
    assert!(head.contains("ref: refs/heads/main"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn parity_repo_init_bare() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_parity_repo_bare");
    let _ = std::fs::remove_dir_all(&tmp);

    let repo = Repository::init(tmp.to_str().unwrap(), true).unwrap();
    assert!(repo.is_bare());
    assert!(std::path::Path::new(repo.git_dir()).join("HEAD").exists());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn parity_repo_reinit_preserves() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_parity_repo_reinit");
    let _ = std::fs::remove_dir_all(&tmp);

    let _repo1 = Repository::init(tmp.to_str().unwrap(), false).unwrap();
    // Write a custom ref
    let git_dir = tmp.join(".git");
    std::fs::write(git_dir.join("refs/heads/main"), "ae90f12eea699729ed24555e40b9fd669da12a12\n").unwrap();

    // Re-init should not destroy existing refs
    let _repo2 = Repository::init(tmp.to_str().unwrap(), false).unwrap();
    let ref_content = std::fs::read_to_string(git_dir.join("refs/heads/main")).unwrap();
    assert!(ref_content.contains("ae90f12eea699729ed24555e40b9fd669da12a12"));

    let _ = std::fs::remove_dir_all(&tmp);
}
