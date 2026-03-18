/// MuonGit Benchmark Suite — Swift
/// Outputs JSON lines to stdout for cross-language comparison.

import Foundation
import MuonGit

/// Run a benchmark: warm up, then measure `iterations` runs.
func bench(_ name: String, iterations: Int, warmup: Int, _ body: () -> Void) {
    for _ in 0..<warmup { body() }

    var times = [Double]()
    times.reserveCapacity(iterations)
    for _ in 0..<iterations {
        let start = Date()
        body()
        times.append(Date().timeIntervalSince(start) * 1000.0)
    }

    times.sort()
    let min = times[0]
    let mean = times.reduce(0, +) / Double(times.count)
    let median: Double
    if times.count % 2 == 0 {
        median = (times[times.count / 2 - 1] + times[times.count / 2]) / 2.0
    } else {
        median = times[times.count / 2]
    }
    let opsPerSec = 1000.0 / mean

    print("""
    {"op":"\(name)","lang":"swift","iterations":\(iterations),"min_ms":\(String(format: "%.3f", min)),"mean_ms":\(String(format: "%.3f", mean)),"median_ms":\(String(format: "%.3f", median)),"ops_per_sec":\(String(format: "%.1f", opsPerSec))}
    """.trimmingCharacters(in: .whitespaces))
}

// SHA-1 10KB (comparable with Kotlin)
let data10kb = [UInt8](repeating: 0xAB, count: 10_000)
bench("sha1_10kb", iterations: 50, warmup: 5) {
    let _ = SHA1.hash(data10kb)
}

// SHA-256 10KB
bench("sha256_10kb", iterations: 50, warmup: 5) {
    let _ = SHA256Hash.hash(data10kb)
}

// OID compare 256x16K (matching libgit2 benchmark)
let oidsA = (0..<256).map { OID.hash(type: .blob, data: Array("oid_a_\($0)".utf8)) }
let oidsB = (0..<256).map { OID.hash(type: .blob, data: Array("oid_b_\($0)".utf8)) }
bench("oid_cmp_256x16k", iterations: 10, warmup: 2) {
    for _ in 0..<16384 {
        for j in 0..<256 {
            let _ = oidsA[j] == oidsB[j]
        }
    }
}

// SHA-1 1MB
let data1mb = [UInt8](repeating: 0xAB, count: 1_000_000)
bench("sha1_1mb", iterations: 20, warmup: 3) {
    let _ = SHA1.hash(data1mb)
}

// SHA-256 1MB
bench("sha256_1mb", iterations: 20, warmup: 3) {
    let _ = SHA256Hash.hash(data1mb)
}

// SHA-1 10MB
let data10mb = [UInt8](repeating: 0xCD, count: 10_000_000)
bench("sha1_10mb", iterations: 5, warmup: 1) {
    let _ = SHA1.hash(data10mb)
}

// SHA-256 10MB
bench("sha256_10mb", iterations: 5, warmup: 1) {
    let _ = SHA256Hash.hash(data10mb)
}

// OID creation 10K
bench("oid_create_10k", iterations: 10, warmup: 2) {
    for i in 0..<10_000 {
        let _ = OID.hash(type: .blob, data: Array("blob content \(i)".utf8))
    }
}

// OID creation 100K
bench("oid_create_100k", iterations: 3, warmup: 1) {
    for i in 0..<100_000 {
        let _ = OID.hash(type: .blob, data: Array("blob content \(i)".utf8))
    }
}

// Blob hashing 10K
bench("blob_hash_10k", iterations: 10, warmup: 2) {
    for i in 0..<10_000 {
        let _ = OID.hash(type: .blob, data: Array("line \(i)\nmore content here\n".utf8))
    }
}

// Tree serialize 1K entries
let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
let entries1k = (0..<1000).map {
    TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%04d.txt", $0), oid: oid)
}
bench("tree_serialize_1k", iterations: 20, warmup: 3) {
    let data = serializeTree(entries: entries1k)
    let _ = try! parseTree(oid: OID.zero, data: data)
}

// Tree serialize 10K entries
let entries10k = (0..<10_000).map {
    TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%05d.txt", $0), oid: oid)
}
bench("tree_serialize_10k", iterations: 5, warmup: 1) {
    let data = serializeTree(entries: entries10k)
    let _ = try! parseTree(oid: OID.zero, data: data)
}

// Commit serialize 10K
let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
let sig = Signature(name: "Bench Test", email: "bench@test", time: 1000000, offset: 0)
bench("commit_serialize_10k", iterations: 10, warmup: 2) {
    for i in 0..<10_000 {
        let _ = serializeCommit(treeId: treeId, parentIds: [], author: sig, committer: sig, message: "commit \(i)\n")
    }
}

// Index read/write 1K
do {
    let tmp = NSTemporaryDirectory() + "muongit_bench_index_1k"
    try? FileManager.default.removeItem(atPath: tmp)
    let repo = try Repository.create(at: tmp)
    var index = MuonGit.Index()
    for i in 0..<1000 {
        index.add(IndexEntry(mode: 0o100644, fileSize: UInt32(i), oid: oid, path: String(format: "src/file_%04d.txt", i)))
    }
    bench("index_rw_1k", iterations: 20, warmup: 3) {
        try! writeIndex(gitDir: repo.gitDir, index: index)
        let _ = try! readIndex(gitDir: repo.gitDir)
    }
    try? FileManager.default.removeItem(atPath: tmp)
}

// Index read/write 10K
do {
    let tmp = NSTemporaryDirectory() + "muongit_bench_index_10k"
    try? FileManager.default.removeItem(atPath: tmp)
    let repo = try Repository.create(at: tmp)
    var index = MuonGit.Index()
    for i in 0..<10_000 {
        index.add(IndexEntry(mode: 0o100644, fileSize: UInt32(i), oid: oid, path: String(format: "src/file_%05d.txt", i)))
    }
    bench("index_rw_10k", iterations: 5, warmup: 1) {
        try! writeIndex(gitDir: repo.gitDir, index: index)
        let _ = try! readIndex(gitDir: repo.gitDir)
    }
    try? FileManager.default.removeItem(atPath: tmp)
}

// Diff 1K trees
let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
let old1k = (0..<1000).map {
    TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%04d.txt", $0), oid: oid)
}
let new1k = (0..<1000).map { i -> TreeEntry in
    let o = i % 10 == 0 ? oid2 : oid
    return TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%04d.txt", i), oid: o)
}
bench("diff_1k", iterations: 50, warmup: 5) {
    let _ = diffTrees(oldEntries: old1k, newEntries: new1k)
}

// Diff 10K trees
let old10k = (0..<10_000).map {
    TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%05d.txt", $0), oid: oid)
}
let new10k = (0..<10_000).map { i -> TreeEntry in
    let o = i % 10 == 0 ? oid2 : oid
    return TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%05d.txt", i), oid: o)
}
bench("diff_10k", iterations: 10, warmup: 2) {
    let _ = diffTrees(oldEntries: old10k, newEntries: new10k)
}
