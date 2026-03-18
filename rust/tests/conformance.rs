//! Conformance test suite
//! These tests use identical inputs and expected outputs across all three ports
//! (Rust, Swift, Kotlin) to verify cross-language consistency.

use muongit::*;
use muongit::sha1::SHA1;
use muongit::sha256::{SHA256, HashAlgorithm};
use muongit::commit::{serialize_commit, parse_commit};
use muongit::tree::{TreeEntry, file_mode, serialize_tree, parse_tree};
use muongit::tag::{serialize_tag, parse_tag};
use muongit::pack::apply_delta;
use muongit::index::{Index, IndexEntry, write_index, read_index};

#[test]
fn conformance_sha1_vectors() {
    // Vector 1: empty string
    let d1 = SHA1::hash(b"");
    assert_eq!(hex(&d1), "da39a3ee5e6b4b0d3255bfef95601890afd80709");

    // Vector 2: "hello"
    let d2 = SHA1::hash(b"hello");
    assert_eq!(hex(&d2), "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");

    // Vector 3: longer string
    let d3 = SHA1::hash(b"The quick brown fox jumps over the lazy dog");
    assert_eq!(hex(&d3), "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12");

    // Vector 4: with newline
    let d4 = SHA1::hash(b"hello world\n");
    assert_eq!(hex(&d4), "22596363b3de40b06f981fb85d82312e8c0ed511");
}

#[test]
fn conformance_blob_oid() {
    let oid1 = OID::hash_object(ObjectType::Blob, b"hello\n");
    assert_eq!(oid1.hex(), "ce013625030ba8dba906f756967f9e9ca394464a");

    let oid2 = OID::hash_object(ObjectType::Blob, b"");
    assert_eq!(oid2.hex(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");

    let oid3 = OID::hash_object(ObjectType::Blob, b"test content\n");
    assert_eq!(oid3.hex(), "d670460b4b4aece5915caf5c68d12f560a9fe3e4");
}

#[test]
fn conformance_commit_oid() {
    let tree_id = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
    let author = Signature { name: "Conf Author".into(), email: "author@conf.test".into(), time: 1700000000, offset: 0 };
    let committer = Signature { name: "Conf Committer".into(), email: "committer@conf.test".into(), time: 1700000000, offset: 0 };

    let data = serialize_commit(&tree_id, &[], &author, &committer, "conformance test commit\n", None);
    let oid = OID::hash_object(ObjectType::Commit, &data);

    let parsed = parse_commit(oid.clone(), &data).unwrap();
    assert_eq!(parsed.tree_id, tree_id);
    assert_eq!(parsed.author.name, "Conf Author");
    assert_eq!(parsed.committer.email, "committer@conf.test");
    assert_eq!(parsed.message, "conformance test commit\n");

    assert!(!oid.is_zero());
    assert_eq!(oid.hex().len(), 40);
}

#[test]
fn conformance_tree_oid() {
    let blob_oid = OID::from_hex("ce013625030ba8dba906f756967f9e9ca394464a").unwrap();
    let entries = vec![
        TreeEntry { mode: file_mode::BLOB, name: "hello.txt".into(), oid: blob_oid.clone() },
    ];
    let data = serialize_tree(&entries);
    let oid = OID::hash_object(ObjectType::Tree, &data);

    let parsed = parse_tree(oid.clone(), &data).unwrap();
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].name, "hello.txt");
    assert_eq!(parsed.entries[0].oid, blob_oid);

    assert!(!oid.is_zero());
    assert_eq!(oid.hex().len(), 40);
}

#[test]
fn conformance_tag_oid() {
    let target_id = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let tagger = Signature { name: "Conf Tagger".into(), email: "tagger@conf.test".into(), time: 1700000000, offset: 0 };

    let data = serialize_tag(&target_id, ObjectType::Commit, "v1.0-conf", Some(&tagger), "conformance tag\n");
    let oid = OID::hash_object(ObjectType::Tag, &data);

    let parsed = parse_tag(oid.clone(), &data).unwrap();
    assert_eq!(parsed.target_id, target_id);
    assert_eq!(parsed.tag_name, "v1.0-conf");
    assert_eq!(parsed.tagger.as_ref().unwrap().name, "Conf Tagger");

    assert!(!oid.is_zero());
}

#[test]
fn conformance_signature_format() {
    use muongit::commit::format_signature;

    // Positive offset
    let sig1 = Signature { name: "Test User".into(), email: "test@example.com".into(), time: 1234567890, offset: 330 };
    assert_eq!(format_signature(&sig1), "Test User <test@example.com> 1234567890 +0530");

    // Negative offset
    let sig2 = Signature { name: "Test".into(), email: "test@test.com".into(), time: 1000, offset: -480 };
    assert_eq!(format_signature(&sig2), "Test <test@test.com> 1000 -0800");

    // Zero offset
    let sig3 = Signature { name: "Zero".into(), email: "zero@test.com".into(), time: 0, offset: 0 };
    assert_eq!(format_signature(&sig3), "Zero <zero@test.com> 0 +0000");
}

#[test]
fn conformance_delta_apply() {
    // Copy entire base
    let base1 = b"hello world";
    let delta1: Vec<u8> = vec![11, 11, 0x80 | 0x01 | 0x10, 0, 11];
    let result1 = apply_delta(base1, &delta1).unwrap();
    assert_eq!(std::str::from_utf8(&result1).unwrap(), "hello world");

    // Insert only
    let base2 = b"hello";
    let mut delta2: Vec<u8> = vec![5, 6, 6];
    delta2.extend_from_slice(b"world!");
    let result2 = apply_delta(base2, &delta2).unwrap();
    assert_eq!(std::str::from_utf8(&result2).unwrap(), "world!");

    // Copy + insert
    let base3 = b"hello cruel";
    let mut delta3: Vec<u8> = vec![11, 11, 0x80 | 0x01 | 0x10, 0, 5, 6];
    delta3.extend_from_slice(b" world");
    let result3 = apply_delta(base3, &delta3).unwrap();
    assert_eq!(std::str::from_utf8(&result3).unwrap(), "hello world");
}

#[test]
fn conformance_index_round_trip() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_conf_index");
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

    let oid = OID::from_hex("ce013625030ba8dba906f756967f9e9ca394464a").unwrap();

    let mut index = Index::new();
    index.add(IndexEntry {
        ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
        dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
        file_size: 6, oid: oid.clone(), flags: 0, path: "hello.txt".into(),
    });
    index.add(IndexEntry {
        ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
        dev: 0, ino: 0, mode: 0o100755, uid: 0, gid: 0,
        file_size: 100, oid: OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap(), flags: 0, path: "script.sh".into(),
    });
    write_index(repo.git_dir(), &index).unwrap();

    let loaded = read_index(repo.git_dir()).unwrap();
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.entries[0].path, "hello.txt");
    assert_eq!(loaded.entries[1].path, "script.sh");
    assert_eq!(loaded.entries[0].mode, 0o100644);
    assert_eq!(loaded.entries[1].mode, 0o100755);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn conformance_sha256_vectors() {
    // Vector 1: empty string
    let d1 = SHA256::hash(b"");
    assert_eq!(hex(&d1), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");

    // Vector 2: "hello"
    let d2 = SHA256::hash(b"hello");
    assert_eq!(hex(&d2), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");

    // Vector 3: longer string
    let d3 = SHA256::hash(b"The quick brown fox jumps over the lazy dog");
    assert_eq!(hex(&d3), "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592");
}

#[test]
fn conformance_sha256_blob_oid() {
    let oid1 = OID::hash_object_sha256(ObjectType::Blob, b"hello\n");
    assert_eq!(oid1.hex().len(), 64);
    assert!(!oid1.is_zero());

    let oid2 = OID::hash_object_sha256(ObjectType::Blob, b"");
    assert_eq!(oid2.hex().len(), 64);

    // SHA-256 and SHA-1 should produce different OIDs
    let oid_sha1 = OID::hash_object(ObjectType::Blob, b"hello\n");
    assert_ne!(oid1.hex(), oid_sha1.hex());
}

#[test]
fn conformance_hash_algorithm() {
    assert_eq!(HashAlgorithm::SHA1.digest_length(), 20);
    assert_eq!(HashAlgorithm::SHA256.digest_length(), 32);
    assert_eq!(HashAlgorithm::SHA1.hex_length(), 40);
    assert_eq!(HashAlgorithm::SHA256.hex_length(), 64);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
