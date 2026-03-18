//! MuonGit Benchmark Suite — Rust
//! Outputs JSON lines to stdout for cross-language comparison.

use muongit::*;
use muongit::sha1::SHA1;
use muongit::sha256::SHA256;
use muongit::commit::serialize_commit;
use muongit::tree::{TreeEntry, file_mode, serialize_tree, parse_tree};
use muongit::index::{Index, IndexEntry, write_index, read_index};
use muongit::diff::diff_trees;

use std::time::Instant;

/// Return a base temp directory that works both in-tree (cargo) and standalone.
fn bench_tmp_dir() -> std::path::PathBuf {
    // Prefer a sibling `tmp/` next to the running binary, fall back to `./tmp/`.
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("tmp")
}

/// Run a benchmark: warm up, then measure `iterations` runs.
/// Returns (min_ms, mean_ms, median_ms).
fn bench<F: FnMut()>(name: &str, iterations: usize, warmup: usize, mut f: F) {
    // Warm-up
    for _ in 0..warmup {
        f();
    }

    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        f();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = times[0];
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    let median = if times.len() % 2 == 0 {
        (times[times.len() / 2 - 1] + times[times.len() / 2]) / 2.0
    } else {
        times[times.len() / 2]
    };
    let ops_per_sec = 1000.0 / mean;

    println!(
        r#"{{"op":"{}","lang":"rust","iterations":{},"min_ms":{:.3},"mean_ms":{:.3},"median_ms":{:.3},"ops_per_sec":{:.1}}}"#,
        name, iterations, min, mean, median, ops_per_sec
    );
}

fn main() {
    // SHA-1 10KB (comparable with Kotlin)
    let data_10kb = vec![0xABu8; 10_000];
    bench("sha1_10kb", 50, 5, || {
        let _ = SHA1::hash(&data_10kb);
    });

    // SHA-256 10KB
    bench("sha256_10kb", 50, 5, || {
        let _ = SHA256::hash(&data_10kb);
    });

    // OID compare 256x16K (matching libgit2 benchmark)
    let oids_a: Vec<OID> = (0..256).map(|i| {
        OID::hash_object(ObjectType::Blob, format!("oid_a_{}", i).as_bytes())
    }).collect();
    let oids_b: Vec<OID> = (0..256).map(|i| {
        OID::hash_object(ObjectType::Blob, format!("oid_b_{}", i).as_bytes())
    }).collect();
    bench("oid_cmp_256x16k", 10, 2, || {
        for _ in 0..16384 {
            for j in 0..256 {
                let _ = oids_a[j] == oids_b[j];
            }
        }
    });

    // SHA-1 1MB
    let data_1mb = vec![0xABu8; 1_000_000];
    bench("sha1_1mb", 20, 3, || {
        let _ = SHA1::hash(&data_1mb);
    });

    // SHA-256 1MB
    bench("sha256_1mb", 20, 3, || {
        let _ = SHA256::hash(&data_1mb);
    });

    // SHA-1 10MB
    let data_10mb = vec![0xCDu8; 10_000_000];
    bench("sha1_10mb", 5, 1, || {
        let _ = SHA1::hash(&data_10mb);
    });

    // SHA-256 10MB
    bench("sha256_10mb", 5, 1, || {
        let _ = SHA256::hash(&data_10mb);
    });

    // OID creation 10K
    bench("oid_create_10k", 10, 2, || {
        for i in 0..10_000 {
            let data = format!("blob content {}", i);
            let _ = OID::hash_object(ObjectType::Blob, data.as_bytes());
        }
    });

    // OID creation 100K
    bench("oid_create_100k", 3, 1, || {
        for i in 0..100_000 {
            let data = format!("blob content {}", i);
            let _ = OID::hash_object(ObjectType::Blob, data.as_bytes());
        }
    });

    // Blob hashing 10K
    bench("blob_hash_10k", 10, 2, || {
        for i in 0..10_000 {
            let content = format!("line {}\nmore content here\n", i);
            let _ = OID::hash_object(ObjectType::Blob, content.as_bytes());
        }
    });

    // Tree serialize 1K entries
    let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let entries_1k: Vec<TreeEntry> = (0..1000)
        .map(|i| TreeEntry {
            mode: file_mode::BLOB,
            name: format!("file_{:04}.txt", i),
            oid: oid.clone(),
        })
        .collect();
    bench("tree_serialize_1k", 20, 3, || {
        let data = serialize_tree(&entries_1k);
        let _ = parse_tree(OID::zero(), &data).unwrap();
    });

    // Tree serialize 10K entries
    let entries_10k: Vec<TreeEntry> = (0..10_000)
        .map(|i| TreeEntry {
            mode: file_mode::BLOB,
            name: format!("file_{:05}.txt", i),
            oid: oid.clone(),
        })
        .collect();
    bench("tree_serialize_10k", 5, 1, || {
        let data = serialize_tree(&entries_10k);
        let _ = parse_tree(OID::zero(), &data).unwrap();
    });

    // Commit serialize 10K
    let tree_id = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
    let sig = Signature {
        name: "Bench Test".into(),
        email: "bench@test".into(),
        time: 1000000,
        offset: 0,
    };
    bench("commit_serialize_10k", 10, 2, || {
        for i in 0..10_000 {
            let msg = format!("commit {}\n", i);
            let _ = serialize_commit(&tree_id, &[], &sig, &sig, &msg, None);
        }
    });

    // Index read/write 1K
    let tmp = bench_tmp_dir().join("bench_index_1k");
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
    let mut index = Index::new();
    for i in 0..1000 {
        index.add(IndexEntry {
            ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
            dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
            file_size: i as u32, oid: oid.clone(), flags: 0,
            path: format!("src/file_{:04}.txt", i),
        });
    }
    bench("index_rw_1k", 20, 3, || {
        write_index(repo.git_dir(), &index).unwrap();
        let _ = read_index(repo.git_dir()).unwrap();
    });

    // Index read/write 10K
    let tmp10 = bench_tmp_dir().join("bench_index_10k");
    let _ = std::fs::remove_dir_all(&tmp10);
    let repo10 = Repository::init(tmp10.to_str().unwrap(), false).unwrap();
    let mut index10 = Index::new();
    for i in 0..10_000 {
        index10.add(IndexEntry {
            ctime_secs: 0, ctime_nanos: 0, mtime_secs: 0, mtime_nanos: 0,
            dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
            file_size: i as u32, oid: oid.clone(), flags: 0,
            path: format!("src/file_{:05}.txt", i),
        });
    }
    bench("index_rw_10k", 5, 1, || {
        write_index(repo10.git_dir(), &index10).unwrap();
        let _ = read_index(repo10.git_dir()).unwrap();
    });

    // Diff 1K trees
    let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
    let old_1k: Vec<TreeEntry> = (0..1000)
        .map(|i| TreeEntry {
            mode: file_mode::BLOB,
            name: format!("file_{:04}.txt", i),
            oid: oid.clone(),
        })
        .collect();
    let new_1k: Vec<TreeEntry> = (0..1000)
        .map(|i| {
            let o = if i % 10 == 0 { oid2.clone() } else { oid.clone() };
            TreeEntry { mode: file_mode::BLOB, name: format!("file_{:04}.txt", i), oid: o }
        })
        .collect();
    bench("diff_1k", 50, 5, || {
        let _ = diff_trees(&old_1k, &new_1k);
    });

    // Diff 10K trees
    let old_10k: Vec<TreeEntry> = (0..10_000)
        .map(|i| TreeEntry {
            mode: file_mode::BLOB,
            name: format!("file_{:05}.txt", i),
            oid: oid.clone(),
        })
        .collect();
    let new_10k: Vec<TreeEntry> = (0..10_000)
        .map(|i| {
            let o = if i % 10 == 0 { oid2.clone() } else { oid.clone() };
            TreeEntry { mode: file_mode::BLOB, name: format!("file_{:05}.txt", i), oid: o }
        })
        .collect();
    bench("diff_10k", 10, 2, || {
        let _ = diff_trees(&old_10k, &new_10k);
    });

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&tmp10);
}
