package ai.muonium.muongit

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RevisionHistoryTest {
    data class Fixture(
        val a: OID,
        val b: OID,
        val c: OID,
        val d: OID,
        val e: OID,
    )

    @Test
    fun testResolveRevisionExpressions() {
        val (repo, fixture, tmp) = setupFixture("kotlin_revision_resolve")
        try {
            assertEquals(fixture.e, resolveRevision(repo.gitDir, "HEAD"))
            assertEquals(fixture.c, resolveRevision(repo.gitDir, "mainline"))
            assertEquals(fixture.d, resolveRevision(repo.gitDir, fixture.d.hex))
            assertEquals(fixture.c, resolveRevision(repo.gitDir, "HEAD~1"))
            assertEquals(fixture.d, resolveRevision(repo.gitDir, "HEAD^2"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRevparseRanges() {
        val (repo, fixture, tmp) = setupFixture("kotlin_revision_ranges")
        try {
            val twoDot = revparse(repo.gitDir, "mainline..feature")
            assertTrue(twoDot.isRange)
            assertFalse(twoDot.usesMergeBase)
            assertEquals(fixture.c, twoDot.from)
            assertEquals(fixture.d, twoDot.to)

            val threeDot = revparse(repo.gitDir, "mainline...feature")
            assertTrue(threeDot.isRange)
            assertTrue(threeDot.usesMergeBase)
            assertEquals(fixture.c, threeDot.from)
            assertEquals(fixture.d, threeDot.to)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRevwalkDefaultOrderAndFirstParent() {
        val (repo, fixture, tmp) = setupFixture("kotlin_revwalk_default")
        try {
            val walker = Revwalk(repo.gitDir)
            walker.pushHead()
            assertEquals(listOf(fixture.e, fixture.d, fixture.c, fixture.b, fixture.a), walker.allOids())

            val firstParent = Revwalk(repo.gitDir)
            firstParent.pushHead()
            firstParent.simplifyFirstParent()
            assertEquals(listOf(fixture.e, fixture.c, fixture.b, fixture.a), firstParent.allOids())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRevwalkRangeSemantics() {
        val (repo, fixture, tmp) = setupFixture("kotlin_revwalk_ranges")
        try {
            val twoDot = Revwalk(repo.gitDir)
            twoDot.pushRange("mainline..feature")
            assertEquals(listOf(fixture.d), twoDot.allOids())

            val threeDot = Revwalk(repo.gitDir)
            threeDot.pushRange("mainline...feature")
            assertEquals(listOf(fixture.d, fixture.c), threeDot.allOids())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRevwalkTopologicalTimeOrder() {
        val (repo, fixture, tmp) = setupFixture("kotlin_revwalk_topo")
        try {
            val walker = Revwalk(repo.gitDir)
            walker.pushHead()
            walker.sorting(Revwalk.SORT_TOPOLOGICAL or Revwalk.SORT_TIME)
            assertEquals(listOf(fixture.e, fixture.d, fixture.c, fixture.b, fixture.a), walker.allOids())
        } finally {
            tmp.deleteRecursively()
        }
    }

    private fun setupFixture(name: String): Triple<Repository, Fixture, File> {
        val tmp = testDirectory(name)
        tmp.deleteRecursively()
        val repo = Repository.init(tmp.path)
        val tree = writeLooseObject(repo.gitDir, ObjectType.TREE, byteArrayOf())

        val a = makeCommit(repo.gitDir, tree, emptyList(), 1L, "A\n")
        val b = makeCommit(repo.gitDir, tree, listOf(a), 2L, "B\n")
        val c = makeCommit(repo.gitDir, tree, listOf(b), 3L, "C\n")
        val d = makeCommit(repo.gitDir, tree, listOf(b), 4L, "D\n")
        val e = makeCommit(repo.gitDir, tree, listOf(c, d), 5L, "E\n")

        writeReference(repo.gitDir, "refs/heads/main", e)
        writeReference(repo.gitDir, "refs/heads/mainline", c)
        writeReference(repo.gitDir, "refs/heads/feature", d)

        return Triple(repo, Fixture(a, b, c, d, e), tmp)
    }

    private fun makeCommit(
        gitDir: File,
        tree: OID,
        parents: List<OID>,
        time: Long,
        message: String,
    ): OID {
        val signature = Signature(name = "Muon Test", email = "test@muon.ai", time = time, offset = 0)
        val data = serializeCommit(tree, parents, signature, signature, message)
        return writeLooseObject(gitDir, ObjectType.COMMIT, data)
    }

    private fun testDirectory(name: String): File =
        File(System.getProperty("user.dir")).resolve("../tmp/$name")
}
