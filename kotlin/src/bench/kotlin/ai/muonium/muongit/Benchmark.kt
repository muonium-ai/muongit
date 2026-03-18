/// MuonGit Benchmark Suite — Kotlin
/// Outputs JSON lines to stdout for cross-language comparison.

package ai.muonium.muongit

import java.io.File

/** Run a benchmark: warm up, then measure [iterations] runs. */
fun bench(name: String, iterations: Int, warmup: Int, body: () -> Unit) {
    repeat(warmup) { body() }

    val times = mutableListOf<Double>()
    repeat(iterations) {
        val start = System.nanoTime()
        body()
        times.add((System.nanoTime() - start) / 1_000_000.0)
    }

    times.sort()
    val min = times[0]
    val mean = times.sum() / times.size
    val median = if (times.size % 2 == 0) {
        (times[times.size / 2 - 1] + times[times.size / 2]) / 2.0
    } else {
        times[times.size / 2]
    }
    val opsPerSec = 1000.0 / mean

    println("""{"op":"$name","lang":"kotlin","iterations":$iterations,"min_ms":${"%.3f".format(min)},"mean_ms":${"%.3f".format(mean)},"median_ms":${"%.3f".format(median)},"ops_per_sec":${"%.1f".format(opsPerSec)}}""")
}

fun main() {
    // Note: Pure Kotlin SHA is ~300x slower than Rust/Swift release builds.
    // SHA benchmarks use 10KB data to keep total runtime under 2 minutes.

    // SHA-1 10KB
    val data10kb = ByteArray(10_000) { 0xAB.toByte() }
    bench("sha1_10kb", iterations = 5, warmup = 2) {
        SHA1.hash(data10kb)
    }

    // SHA-256 10KB
    bench("sha256_10kb", iterations = 5, warmup = 2) {
        SHA256Hash.hash(data10kb)
    }

    // OID compare 256x16K (matching libgit2 benchmark)
    val oidsA = (0 until 256).map { OID.hashObject(ObjectType.BLOB, "oid_a_$it".toByteArray()) }
    val oidsB = (0 until 256).map { OID.hashObject(ObjectType.BLOB, "oid_b_$it".toByteArray()) }
    bench("oid_cmp_256x16k", iterations = 10, warmup = 2) {
        for (i in 0 until 16384) {
            for (j in 0 until 256) {
                oidsA[j] == oidsB[j]
            }
        }
    }

    // OID creation 1K (each OID hashes ~20 bytes)
    bench("oid_create_1k", iterations = 5, warmup = 2) {
        for (i in 0 until 1_000) {
            OID.hashObject(ObjectType.BLOB, "blob content $i".toByteArray())
        }
    }

    // Blob hashing 1K
    bench("blob_hash_1k", iterations = 5, warmup = 2) {
        for (i in 0 until 1_000) {
            OID.hashObject(ObjectType.BLOB, "line $i\nmore content here\n".toByteArray())
        }
    }

    // Tree serialize 1K entries
    val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
    val entries1k = (0 until 1000).map {
        TreeEntry(mode = FileMode.BLOB, name = "file_%04d.txt".format(it), oid = oid)
    }
    bench("tree_serialize_1k", iterations = 20, warmup = 3) {
        val data = serializeTree(entries1k)
        parseTree(OID.ZERO, data)
    }

    // Tree serialize 10K entries
    val entries10k = (0 until 10_000).map {
        TreeEntry(mode = FileMode.BLOB, name = "file_%05d.txt".format(it), oid = oid)
    }
    bench("tree_serialize_10k", iterations = 5, warmup = 1) {
        val data = serializeTree(entries10k)
        parseTree(OID.ZERO, data)
    }

    // Commit serialize 10K
    val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
    val sig = Signature(name = "Bench Test", email = "bench@test", time = 1000000, offset = 0)
    bench("commit_serialize_10k", iterations = 10, warmup = 2) {
        for (i in 0 until 10_000) {
            serializeCommit(treeId = treeId, parentIds = emptyList(), author = sig, committer = sig, message = "commit $i\n")
        }
    }

    // Index read/write 1K
    val tmp1k = File(System.getProperty("java.io.tmpdir"), "muongit_bench_index_1k")
    tmp1k.deleteRecursively()
    try {
        val repo = Repository.init(tmp1k.path)
        val index = Index()
        for (i in 0 until 1000) {
            index.add(IndexEntry(mode = FileMode.BLOB, fileSize = i, oid = oid, path = "src/file_%04d.txt".format(i)))
        }
        bench("index_rw_1k", iterations = 3, warmup = 1) {
            writeIndex(repo.gitDir, index)
            readIndex(repo.gitDir)
        }
    } finally {
        tmp1k.deleteRecursively()
    }

    // Index read/write 10K (reduced — index SHA checksum is slow in pure Kotlin)
    val tmp10k = File(System.getProperty("java.io.tmpdir"), "muongit_bench_index_10k")
    tmp10k.deleteRecursively()
    try {
        val repo = Repository.init(tmp10k.path)
        val index = Index()
        for (i in 0 until 10_000) {
            index.add(IndexEntry(mode = FileMode.BLOB, fileSize = i, oid = oid, path = "src/file_%05d.txt".format(i)))
        }
        bench("index_rw_10k", iterations = 1, warmup = 0) {
            writeIndex(repo.gitDir, index)
            readIndex(repo.gitDir)
        }
    } finally {
        tmp10k.deleteRecursively()
    }

    // Diff 1K trees
    val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
    val old1k = (0 until 1000).map {
        TreeEntry(mode = FileMode.BLOB, name = "file_%04d.txt".format(it), oid = oid)
    }
    val new1k = (0 until 1000).map { i ->
        val o = if (i % 10 == 0) oid2 else oid
        TreeEntry(mode = FileMode.BLOB, name = "file_%04d.txt".format(i), oid = o)
    }
    bench("diff_1k", iterations = 50, warmup = 5) {
        diffTrees(old1k, new1k)
    }

    // Diff 10K trees
    val old10k = (0 until 10_000).map {
        TreeEntry(mode = FileMode.BLOB, name = "file_%05d.txt".format(it), oid = oid)
    }
    val new10k = (0 until 10_000).map { i ->
        val o = if (i % 10 == 0) oid2 else oid
        TreeEntry(mode = FileMode.BLOB, name = "file_%05d.txt".format(i), oid = o)
    }
    bench("diff_10k", iterations = 10, warmup = 2) {
        diffTrees(old10k, new10k)
    }
}
