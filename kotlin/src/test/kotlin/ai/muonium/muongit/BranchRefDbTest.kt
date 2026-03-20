package ai.muonium.muongit

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class BranchRefDbTest {
    @Test
    fun testRefDbReadsLoosePackedAndSymbolicRefs() {
        val tmp = testDirectory("kotlin_refdb_reads_loose_packed_and_symbolic_refs")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val mainOid = OID("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            val packedOid = OID("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
            writeReference(repo.gitDir, "refs/heads/main", mainOid)
            File(repo.gitDir, "packed-refs").writeText("${packedOid.hex} refs/heads/release\n")

            val head = repo.refdb().read("HEAD")
            assertTrue(head.isSymbolic)
            assertEquals("refs/heads/main", head.symbolicTarget)

            val packed = repo.refdb().read("refs/heads/release")
            assertEquals(packedOid, packed.target)

            val refs = repo.refdb().list()
            assertTrue(refs.any { it.name == "refs/heads/main" })
            assertTrue(refs.any { it.name == "refs/heads/release" })
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRefDbDeleteRemovesPackedRef() {
        val tmp = testDirectory("kotlin_refdb_delete_removes_packed_ref")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val packedOid = OID("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
            File(repo.gitDir, "packed-refs").writeText("${packedOid.hex} refs/heads/release\n")

            assertTrue(repo.refdb().delete("refs/heads/release"))
            assertFailsWith<MuonGitException.NotFound> {
                repo.refdb().read("refs/heads/release")
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testBranchCreateLookupListAndUpstream() {
        val tmp = testDirectory("kotlin_branch_create_lookup_list_and_upstream")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val mainOid = OID("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            writeReference(repo.gitDir, "refs/heads/main", mainOid)

            val branch = createBranch(repo.gitDir, "feature")
            assertEquals("feature", branch.name)
            assertEquals("refs/heads/feature", branch.referenceName)
            assertEquals(mainOid, branch.target)
            assertEquals(false, branch.isHead)

            setBranchUpstream(
                repo.gitDir,
                "feature",
                BranchUpstream(remoteName = "origin", mergeRef = "refs/heads/main")
            )
            val lookedUp = lookupBranch(repo.gitDir, "feature", BranchType.LOCAL)
            assertEquals(
                BranchUpstream(remoteName = "origin", mergeRef = "refs/heads/main"),
                lookedUp.upstream
            )

            val branches = listBranches(repo.gitDir, BranchType.LOCAL)
            assertTrue(branches.any { it.name == "main" && it.isHead })
            assertTrue(branches.any { it.name == "feature" })
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testBranchDetachedHeadRenameAndDeleteEdgeCases() {
        val tmp = testDirectory("kotlin_branch_detached_head_rename_delete_edge_cases")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val detachedOid = OID("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
            File(repo.gitDir, "HEAD").writeText("${detachedOid.hex}\n")
            val detachedBranch = createBranch(repo.gitDir, "detached-copy")
            assertEquals(detachedOid, detachedBranch.target)

            val topicOid = OID("cccccccccccccccccccccccccccccccccccccccc")
            File(repo.gitDir, "packed-refs").writeText("${topicOid.hex} refs/heads/topic\n")
            writeSymbolicReference(repo.gitDir, "HEAD", "refs/heads/topic")
            setBranchUpstream(
                repo.gitDir,
                "topic",
                BranchUpstream(remoteName = "origin", mergeRef = "refs/heads/main")
            )

            val renamed = renameBranch(repo.gitDir, "topic", "renamed")
            assertEquals("renamed", renamed.name)
            assertEquals("ref: refs/heads/renamed", readReference(repo.gitDir, "HEAD"))
            assertEquals(
                BranchUpstream(remoteName = "origin", mergeRef = "refs/heads/main"),
                branchUpstream(repo.gitDir, "renamed")
            )

            assertFailsWith<MuonGitException.Conflict> {
                deleteBranch(repo.gitDir, "renamed", BranchType.LOCAL)
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    private fun testDirectory(name: String): File =
        File(System.getProperty("user.dir")).resolve("../tmp/$name")
}
