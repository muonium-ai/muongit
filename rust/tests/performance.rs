//! Performance tests for muongit
//! These tests measure throughput of key operations and assert minimum performance.

use muongit::*;
use muongit::sha1::SHA1;
use muongit::sha256::SHA256;
use muongit::commit::serialize_commit;
use muongit::tree::{TreeEntry, file_mode, serialize_tree, parse_tree};
use muongit::index::{Index, IndexEntry, write_index, read_index};
use muongit::diff::diff_trees;

use std::time::Instant;

/// Helper: measure execution time in milliseconds
fn measure_ms<F: FnOnce()>(f: F) -> f64 {
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1000.0
}

#[test]
fn perf_sha1_throughput_1mb() {
    let data = vec![0xABu8; 1_000_000]; // 1 MB
    let ms = measure_ms(|| {
        let _ = SHA1::hash(&data);
    });
    eprintln!("[perf] SHA-1 1MB: {:.2}ms", ms);
    // Should complete well under 1 second
    assert!(ms < 1000.0, "SHA-1 1MB took {}ms, expected < 1000ms", ms);
}

#[test]
fn perf_sha256_throughput_1mb() {
    let data = vec![0xABu8; 1_000_000]; // 1 MB
    let ms = measure_ms(|| {
        let _ = SHA256::hash(&data);
    });
    eprintln!("[perf] SHA-256 1MB: {:.2}ms", ms);
    assert!(ms < 1000.0, "SHA-256 1MB took {}ms, expected < 1000ms", ms);
}

#[test]
fn perf_sha1_throughput_10mb() {
    let data = vec![0xCDu8; 10_000_000]; // 10 MB
    let ms = measure_ms(|| {
        let _ = SHA1::hash(&data);
    });
    eprintln!("[perf] SHA-1 10MB: {:.2}ms", ms);
    assert!(ms < 30000.0, "SHA-1 10MB took {}ms, expected < 30000ms", ms);
}

#[test]
fn perf_oid_creation_10k() {
    let ms = measure_ms(|| {
        for i in 0..10_000 {
            let data = format!("blob content {}", i);
            let _ = OID::hash_object(ObjectType::Blob, data.as_bytes());
        }
    });
    eprintln!("[perf] OID creation 10K: {:.2}ms", ms);
    assert!(ms < 2000.0, "OID creation 10K took {}ms, expected < 2000ms", ms);
}

#[test]
fn perf_tree_serialize_1k_entries() {
    let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let entries: Vec<TreeEntry> = (0..1000).map(|i| {
        TreeEntry { mode: file_mode::BLOB, name: format!("file_{:04}.txt", i), oid: oid.clone() }
    }).collect();

    let ms = measure_ms(|| {
        let data = serialize_tree(&entries);
        let _ = parse_tree(OID::zero(), &data).unwrap();
    });
    eprintln!("[perf] Tree serialize+parse 1K entries: {:.2}ms", ms);
    assert!(ms < 1000.0, "Tree 1K took {}ms, expected < 1000ms", ms);
}

#[test]
fn perf_commit_serialize_10k() {
    let tree_id = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
    let sig = Signature { name: "Perf Test".into(), email: "perf@test".into(), time: 1000000, offset: 0 };

    let ms = measure_ms(|| {
        for i in 0..10_000 {
            let msg = format!("commit {}\n", i);
            let _ = serialize_commit(&tree_id, &[], &sig, &sig, &msg, None);
        }
    });
    eprintln!("[perf] Commit serialize 10K: {:.2}ms", ms);
    assert!(ms < 2000.0, "Commit serialize 10K took {}ms, expected < 2000ms", ms);
}

#[test]
fn perf_index_readwrite_1k() {
    let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_perf_index");
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

    let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let mut index = Index::new();
    for i in 0..1000 {
        index.add(IndexEntry {
            ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
            dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
            file_size: i as u32, oid: oid.clone(), flags: 0, path: format!("src/file_{:04}.txt", i),
        });
    }

    let ms = measure_ms(|| {
        write_index(repo.git_dir(), &index).unwrap();
        let _ = read_index(repo.git_dir()).unwrap();
    });
    eprintln!("[perf] Index write+read 1K entries: {:.2}ms", ms);
    assert!(ms < 2000.0, "Index 1K took {}ms, expected < 2000ms", ms);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn perf_diff_large_trees() {
    let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

    let old: Vec<TreeEntry> = (0..1000).map(|i| {
        TreeEntry { mode: file_mode::BLOB, name: format!("file_{:04}.txt", i), oid: oid1.clone() }
    }).collect();
    let new: Vec<TreeEntry> = (0..1000).map(|i| {
        let oid = if i % 10 == 0 { oid2.clone() } else { oid1.clone() };
        TreeEntry { mode: file_mode::BLOB, name: format!("file_{:04}.txt", i), oid }
    }).collect();

    let ms = measure_ms(|| {
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 100); // 10% modified
    });
    eprintln!("[perf] Diff 1K-entry trees: {:.2}ms", ms);
    assert!(ms < 1000.0, "Diff 1K took {}ms, expected < 1000ms", ms);
}

#[test]
fn perf_blob_hashing_10k() {
    let ms = measure_ms(|| {
        for i in 0..10_000 {
            let content = format!("line {}\nmore content here\n", i);
            let _ = OID::hash_object(ObjectType::Blob, content.as_bytes());
        }
    });
    eprintln!("[perf] Blob hashing 10K: {:.2}ms", ms);
    assert!(ms < 2000.0, "Blob hashing 10K took {}ms, expected < 2000ms", ms);
}

#[test]
fn perf_sha1_vs_sha256_comparison() {
    let data = vec![0xABu8; 1_000_000];

    let ms_sha1 = measure_ms(|| { let _ = SHA1::hash(&data); });
    let ms_sha256 = measure_ms(|| { let _ = SHA256::hash(&data); });

    eprintln!("[perf] SHA-1 1MB: {:.2}ms, SHA-256 1MB: {:.2}ms, ratio: {:.2}x",
        ms_sha1, ms_sha256, ms_sha256 / ms_sha1.max(0.001));
    // Both should complete reasonably
    assert!(ms_sha1 < 1000.0);
    assert!(ms_sha256 < 1000.0);
}
