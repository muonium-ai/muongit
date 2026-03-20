package ai.muonium.muongit

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SwitchResetRestoreTest {
    @Test
    fun testSwitchBranchUpdatesHeadAndWorktree() {
        val tmp = testDirectory("kotlin_switch_branch_updates_head_and_worktree")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val mainTree = writeTree(repo.gitDir, listOf("shared.txt" to "main\n", "only-main.txt" to "remove me\n"))
            val mainCommit = writeCommit(repo.gitDir, mainTree, emptyList(), "main", 1)
            val featureTree = writeTree(repo.gitDir, listOf("shared.txt" to "feature\n", "only-feature.txt" to "add me\n"))
            val featureCommit = writeCommit(repo.gitDir, featureTree, listOf(mainCommit), "feature", 2)

            writeReference(repo.gitDir, "refs/heads/main", mainCommit)
            writeReference(repo.gitDir, "refs/heads/feature", featureCommit)
            writeSymbolicReference(repo.gitDir, "HEAD", "refs/heads/main")
            seedWorkdir(mainCommit, repo)

            val result = repo.switchBranch("feature")

            assertEquals(mainCommit, result.previousHead)
            assertEquals(featureCommit, result.headOid)
            assertEquals("refs/heads/feature", result.headRef)
            assertEquals("ref: refs/heads/feature", readReference(repo.gitDir, "HEAD"))
            assertEquals("feature\n", File(tmp, "shared.txt").readText())
            assertFalse(File(tmp, "only-main.txt").exists())
            assertEquals("add me\n", File(tmp, "only-feature.txt").readText())
            assertTrue(result.updatedPaths.contains("shared.txt"))
            assertTrue(result.removedPaths.contains("only-main.txt"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCheckoutRevisionDetachesHead() {
        val tmp = testDirectory("kotlin_checkout_revision_detaches_head")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val mainTree = writeTree(repo.gitDir, listOf("shared.txt" to "main\n"))
            val mainCommit = writeCommit(repo.gitDir, mainTree, emptyList(), "main", 1)
            val featureTree = writeTree(repo.gitDir, listOf("shared.txt" to "detached\n"))
            val featureCommit = writeCommit(repo.gitDir, featureTree, listOf(mainCommit), "feature", 2)

            writeReference(repo.gitDir, "refs/heads/main", mainCommit)
            writeReference(repo.gitDir, "refs/heads/feature", featureCommit)
            writeSymbolicReference(repo.gitDir, "HEAD", "refs/heads/main")
            seedWorkdir(mainCommit, repo)

            val result = repo.checkoutRevision(featureCommit.hex)

            assertEquals(mainCommit, result.previousHead)
            assertEquals(featureCommit, result.headOid)
            assertNull(result.headRef)
            assertEquals(featureCommit.hex, readReference(repo.gitDir, "HEAD"))
            assertEquals("detached\n", File(tmp, "shared.txt").readText())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testSwitchBranchRejectsLocalChanges() {
        val tmp = testDirectory("kotlin_switch_branch_rejects_local_changes")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val mainTree = writeTree(repo.gitDir, listOf("shared.txt" to "main\n"))
            val mainCommit = writeCommit(repo.gitDir, mainTree, emptyList(), "main", 1)
            val featureTree = writeTree(repo.gitDir, listOf("shared.txt" to "feature\n"))
            val featureCommit = writeCommit(repo.gitDir, featureTree, listOf(mainCommit), "feature", 2)

            writeReference(repo.gitDir, "refs/heads/main", mainCommit)
            writeReference(repo.gitDir, "refs/heads/feature", featureCommit)
            writeSymbolicReference(repo.gitDir, "HEAD", "refs/heads/main")
            seedWorkdir(mainCommit, repo)
            File(tmp, "shared.txt").writeText("dirty\n")

            val error = assertFailsWith<MuonGitException.Conflict> {
                repo.switchBranch("feature")
            }

            assertTrue(error.message!!.contains("shared.txt"))
            assertEquals("ref: refs/heads/main", readReference(repo.gitDir, "HEAD"))
            assertEquals("dirty\n", File(tmp, "shared.txt").readText())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testResetModesUpdateRefsIndexAndWorktree() {
        val tmp = testDirectory("kotlin_reset_modes_update_refs_index_and_worktree")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val baseTree = writeTree(repo.gitDir, listOf("file.txt" to "base\n"))
            val baseCommit = writeCommit(repo.gitDir, baseTree, emptyList(), "base", 1)
            val changedTree = writeTree(repo.gitDir, listOf("file.txt" to "changed\n", "new.txt" to "new\n"))
            val changedCommit = writeCommit(repo.gitDir, changedTree, listOf(baseCommit), "changed", 2)

            writeReference(repo.gitDir, "refs/heads/main", changedCommit)
            writeSymbolicReference(repo.gitDir, "HEAD", "refs/heads/main")
            seedWorkdir(changedCommit, repo)

            val baseEntries = materializeEntries(repo.gitDir, baseCommit)
            val changedEntries = materializeEntries(repo.gitDir, changedCommit)

            File(tmp, "file.txt").writeText("dirty soft\n")
            repo.reset(baseCommit.hex, ResetMode.SOFT)
            assertEquals(baseCommit, resolveReference(repo.gitDir, "HEAD"))
            assertEquals(changedEntries.getValue("file.txt").oid, readIndex(repo.gitDir).find("file.txt")?.oid)
            assertEquals("dirty soft\n", File(tmp, "file.txt").readText())

            writeReference(repo.gitDir, "refs/heads/main", changedCommit)
            seedWorkdir(changedCommit, repo)
            File(tmp, "file.txt").writeText("dirty mixed\n")
            repo.reset(baseCommit.hex, ResetMode.MIXED)
            assertEquals(baseCommit, resolveReference(repo.gitDir, "HEAD"))
            assertEquals(baseEntries.getValue("file.txt").oid, readIndex(repo.gitDir).find("file.txt")?.oid)
            assertEquals("dirty mixed\n", File(tmp, "file.txt").readText())
            assertTrue(File(tmp, "new.txt").exists())

            writeReference(repo.gitDir, "refs/heads/main", changedCommit)
            seedWorkdir(changedCommit, repo)
            File(tmp, "file.txt").writeText("dirty hard\n")
            val hard = repo.reset(baseCommit.hex, ResetMode.HARD)
            assertEquals(baseCommit, resolveReference(repo.gitDir, "HEAD"))
            assertEquals("base\n", File(tmp, "file.txt").readText())
            assertFalse(File(tmp, "new.txt").exists())
            assertTrue(hard.removedPaths.contains("new.txt"))
            assertEquals(baseEntries.getValue("file.txt").oid, readIndex(repo.gitDir).find("file.txt")?.oid)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRestoreStagedAndWorktreePaths() {
        val tmp = testDirectory("kotlin_restore_staged_and_worktree_paths")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val commitTree = writeTree(repo.gitDir, listOf("file.txt" to "committed\n"))
            val commit = writeCommit(repo.gitDir, commitTree, emptyList(), "commit", 1)

            writeReference(repo.gitDir, "refs/heads/main", commit)
            writeSymbolicReference(repo.gitDir, "HEAD", "refs/heads/main")
            seedWorkdir(commit, repo)

            val headEntry = materializeEntries(repo.gitDir, commit).getValue("file.txt")

            File(tmp, "file.txt").writeText("worktree\n")
            val stagedOid = writeLooseObject(repo.gitDir, ObjectType.BLOB, "staged\n".toByteArray())
            val index = readIndex(repo.gitDir)
            index.add(
                IndexEntry(
                    mode = headEntry.mode,
                    fileSize = "staged\n".toByteArray().size,
                    oid = stagedOid,
                    flags = "file.txt".length,
                    path = "file.txt",
                )
            )
            writeIndex(repo.gitDir, index)

            val result = repo.restore(listOf("file.txt"), RestoreOptions(staged = true, worktree = true))

            assertEquals(headEntry.oid, readIndex(repo.gitDir).find("file.txt")?.oid)
            assertEquals("committed\n", File(tmp, "file.txt").readText())
            assertEquals(listOf("file.txt"), result.stagedPaths)
            assertEquals(listOf("file.txt"), result.restoredPaths)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRestoreFromSourceUpdatesWorktreeOnly() {
        val tmp = testDirectory("kotlin_restore_from_source_updates_worktree_only")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val baseTree = writeTree(repo.gitDir, listOf("file.txt" to "base\n"))
            val baseCommit = writeCommit(repo.gitDir, baseTree, emptyList(), "base", 1)
            val changedTree = writeTree(repo.gitDir, listOf("file.txt" to "changed\n"))
            val changedCommit = writeCommit(repo.gitDir, changedTree, listOf(baseCommit), "changed", 2)

            writeReference(repo.gitDir, "refs/heads/main", changedCommit)
            writeSymbolicReference(repo.gitDir, "HEAD", "refs/heads/main")
            seedWorkdir(changedCommit, repo)
            File(tmp, "file.txt").writeText("dirty\n")

            repo.restore(listOf("file.txt"), RestoreOptions(source = baseCommit.hex, staged = false, worktree = true))

            assertEquals("base\n", File(tmp, "file.txt").readText())
            assertEquals(
                materializeEntries(repo.gitDir, changedCommit).getValue("file.txt").oid,
                readIndex(repo.gitDir).find("file.txt")?.oid
            )
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRestoreMissingPathFails() {
        val tmp = testDirectory("kotlin_restore_missing_path_fails")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val commitTree = writeTree(repo.gitDir, listOf("file.txt" to "committed\n"))
            val commit = writeCommit(repo.gitDir, commitTree, emptyList(), "commit", 1)

            writeReference(repo.gitDir, "refs/heads/main", commit)
            writeSymbolicReference(repo.gitDir, "HEAD", "refs/heads/main")
            seedWorkdir(commit, repo)

            val error = assertFailsWith<MuonGitException.NotFound> {
                repo.restore(listOf("missing.txt"))
            }
            assertTrue(error.message!!.contains("missing.txt"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    private data class MaterializedEntry(
        val path: String,
        val oid: OID,
        val mode: Int,
        val data: ByteArray,
    )

    private fun writeTree(gitDir: File, files: List<Pair<String, String>>): OID {
        val entries = files.map { (name, content) ->
            TreeEntry(
                mode = FileMode.BLOB,
                name = name,
                oid = writeLooseObject(gitDir, ObjectType.BLOB, content.toByteArray()),
            )
        }
        return writeLooseObject(gitDir, ObjectType.TREE, serializeTree(entries))
    }

    private fun writeCommit(gitDir: File, treeOid: OID, parents: List<OID>, message: String, time: Long): OID {
        val signature = Signature(name = "Muon Test", email = "test@muon.ai", time = time, offset = 0)
        return writeLooseObject(
            gitDir,
            ObjectType.COMMIT,
            serializeCommit(treeOid, parents, signature, signature, "$message\n")
        )
    }

    private fun materializeEntries(gitDir: File, commitOid: OID): Map<String, MaterializedEntry> {
        val commit = readObject(gitDir, commitOid).asCommit()
        val result = sortedMapOf<String, MaterializedEntry>()
        collectEntries(gitDir, commit.treeId, "", result)
        return result
    }

    private fun collectEntries(
        gitDir: File,
        treeOid: OID,
        prefix: String,
        result: MutableMap<String, MaterializedEntry>,
    ) {
        val tree = readObject(gitDir, treeOid).asTree()
        for (entry in tree.entries) {
            val path = if (prefix.isEmpty()) entry.name else "$prefix/${entry.name}"
            if (entry.mode == FileMode.TREE) {
                collectEntries(gitDir, entry.oid, path, result)
            } else {
                val blob = readBlob(gitDir, entry.oid)
                result[path] = MaterializedEntry(path, entry.oid, entry.mode, blob.data)
            }
        }
    }

    private fun seedWorkdir(commitOid: OID, repo: Repository) {
        val entries = materializeEntries(repo.gitDir, commitOid)
        clearWorkdir(repo.workdir!!)
        val index = Index()
        for (path in entries.keys.sorted()) {
            val entry = entries.getValue(path)
            index.add(
                IndexEntry(
                    mode = entry.mode,
                    fileSize = entry.data.size,
                    oid = entry.oid,
                    flags = minOf(path.length, 0x0FFF),
                    path = path,
                )
            )
        }
        writeIndex(repo.gitDir, index)
        checkoutIndex(repo.gitDir, repo.workdir!!, CheckoutOptions(force = true))
    }

    private fun clearWorkdir(workdir: File) {
        val entries = workdir.listFiles() ?: return
        for (entry in entries) {
            if (entry.name == ".git") {
                continue
            }
            entry.deleteRecursively()
        }
    }

    private fun testDirectory(name: String): File =
        File(System.getProperty("user.dir")).resolve("../tmp/$name")
}
