package ai.muonium.muongit

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class StagingCommitPorcelainTest {
    @Test
    fun testAddStagesModifiedAndUntrackedPathspecMatches() {
        val tmp = testDirectory("kotlin_porcelain_add_pathspec")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val index = buildIndex(repo.gitDir, listOf(
                "src/one.txt" to "base\n",
                "notes.md" to "keep\n",
            ))
            writeIndex(repo.gitDir, index)
            writeWorkdirFile(tmp, "src/one.txt", "changed\n")
            writeWorkdirFile(tmp, "src/two.txt", "new\n")
            writeWorkdirFile(tmp, "docs/readme.md", "skip\n")
            writeWorkdirFile(tmp, "notes.md", "keep\n")

            val result = repo.add(listOf("src/*.txt"))

            assertEquals(listOf("src/one.txt", "src/two.txt"), result.stagedPaths)
            assertEquals(emptyList(), result.removedPaths)

            val updated = readIndex(repo.gitDir)
            val one = updated.find("src/one.txt")!!
            val two = updated.find("src/two.txt")!!
            assertNull(updated.find("docs/readme.md"))
            assertTrue(readBlob(repo.gitDir, one.oid).data.contentEquals("changed\n".toByteArray()))
            assertTrue(readBlob(repo.gitDir, two.oid).data.contentEquals("new\n".toByteArray()))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRemoveDeletesTrackedPathsFromIndexAndWorkdir() {
        val tmp = testDirectory("kotlin_porcelain_remove")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            seedHead(repo, listOf("tracked.txt" to "tracked\n", "keep.txt" to "keep\n"), "base")

            val result = repo.remove(listOf("tracked.txt"))

            assertEquals(listOf("tracked.txt"), result.removedFromIndex)
            assertEquals(listOf("tracked.txt"), result.removedFromWorkdir)
            assertNull(readIndex(repo.gitDir).find("tracked.txt"))
            assertFalse(File(tmp, "tracked.txt").exists())
            assertTrue(File(tmp, "keep.txt").exists())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testUnstageRestoresHeadEntriesAndDropsNewPaths() {
        val tmp = testDirectory("kotlin_porcelain_unstage")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            seedHead(repo, listOf("tracked.txt" to "base\n"), "base")
            writeWorkdirFile(tmp, "tracked.txt", "staged\n")
            writeWorkdirFile(tmp, "new.txt", "new\n")
            repo.add(listOf("tracked.txt", "new.txt"))

            val result = repo.unstage(listOf("tracked.txt", "new.txt"))

            assertEquals(listOf("tracked.txt"), result.restoredPaths)
            assertEquals(listOf("new.txt"), result.removedPaths)

            val updated = readIndex(repo.gitDir)
            val tracked = updated.find("tracked.txt")!!
            assertTrue(readBlob(repo.gitDir, tracked.oid).data.contentEquals("base\n".toByteArray()))
            assertNull(updated.find("new.txt"))
            assertTrue(File(tmp, "new.txt").exists())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testUnstageOnUnbornBranchRemovesNewEntries() {
        val tmp = testDirectory("kotlin_porcelain_unstage_unborn")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            writeWorkdirFile(tmp, "new.txt", "new\n")
            repo.add(listOf("new.txt"))

            val result = repo.unstage(listOf("new.txt"))

            assertEquals(emptyList(), result.restoredPaths)
            assertEquals(listOf("new.txt"), result.removedPaths)
            assertTrue(readIndex(repo.gitDir).entries.isEmpty())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCommitUpdatesBranchAndReflogs() {
        val tmp = testDirectory("kotlin_porcelain_commit")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val base = seedHead(repo, listOf("tracked.txt" to "base\n", "remove.txt" to "remove me\n"), "base")
            writeWorkdirFile(tmp, "tracked.txt", "changed\n")
            writeWorkdirFile(tmp, "new.txt", "new\n")
            repo.add(listOf("tracked.txt", "new.txt"))
            repo.remove(listOf("remove.txt"))

            val result = repo.commit("second")

            assertEquals("refs/heads/main", result.reference)
            assertEquals(listOf(base), result.parentIds)
            assertEquals("second", result.summary)
            assertEquals(result.oid, resolveReference(repo.gitDir, "HEAD"))
            assertEquals(result.oid, resolveReference(repo.gitDir, "refs/heads/main"))

            val commit = readObject(repo.gitDir, result.oid).asCommit()
            assertEquals(listOf(base), commit.parentIds)
            val headLog = readReflog(repo.gitDir, "HEAD")
            val branchLog = readReflog(repo.gitDir, "refs/heads/main")
            assertEquals("commit: second", headLog.last().message)
            assertEquals("commit: second", branchLog.last().message)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCommitRejectsDetachedHead() {
        val tmp = testDirectory("kotlin_porcelain_commit_detached")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val base = seedHead(repo, listOf("tracked.txt" to "base\n"), "base")
            writeReference(repo.gitDir, "HEAD", base)

            val error = assertFailsWith<MuonGitException.InvalidSpec> {
                repo.commit("detached")
            }
            assertTrue(error.message!!.contains("detached HEAD"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    private fun buildIndex(gitDir: File, files: List<Pair<String, String>>): Index {
        val index = Index()
        for ((path, content) in files) {
            val oid = writeLooseObject(gitDir, ObjectType.BLOB, content.toByteArray())
            index.add(
                IndexEntry(
                    mode = FileMode.BLOB,
                    fileSize = content.toByteArray().size,
                    oid = oid,
                    flags = minOf(path.length, 0x0FFF),
                    path = path
                )
            )
        }
        return index
    }

    private fun seedHead(repo: Repository, files: List<Pair<String, String>>, message: String): OID {
        val commit = writeCommitSnapshot(repo.gitDir, files, emptyList(), message, 1)
        writeReference(repo.gitDir, "refs/heads/main", commit)
        writeIndex(repo.gitDir, buildIndex(repo.gitDir, files))
        for ((path, content) in files) {
            writeWorkdirFile(repo.workdir!!, path, content)
        }
        return commit
    }

    private fun writeCommitSnapshot(
        gitDir: File,
        files: List<Pair<String, String>>,
        parents: List<OID>,
        message: String,
        time: Long
    ): OID {
        val entries = files.map { (path, content) ->
            TreeEntry(
                mode = FileMode.BLOB,
                name = path,
                oid = writeLooseObject(gitDir, ObjectType.BLOB, content.toByteArray())
            )
        }
        val treeOid = writeLooseObject(gitDir, ObjectType.TREE, serializeTree(entries))
        val signature = Signature(name = "Muon Test", email = "test@muon.ai", time = time, offset = 0)
        val data = serializeCommit(treeOid, parents, signature, signature, "$message\n")
        return writeLooseObject(gitDir, ObjectType.COMMIT, data)
    }

    private fun writeWorkdirFile(workdir: File, path: String, content: String) {
        val file = File(workdir, path)
        file.parentFile?.mkdirs()
        file.writeText(content)
    }

    private fun testDirectory(name: String): File =
        File(System.getProperty("user.dir")).resolve("../tmp").resolve(name)
}
