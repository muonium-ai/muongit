package ai.muonium.muongit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class MuonGitTest {
    @Test
    fun testOIDFromHex() {
        val hex = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val oid = OID(hex)
        assertEquals(hex, oid.hex)
    }

    @Test
    fun testOIDEquality() {
        val a = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val b = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        assertEquals(a, b)
    }

    @Test
    fun testSignature() {
        val sig = Signature(name = "Test User", email = "test@example.com")
        assertEquals("Test User", sig.name)
        assertEquals("test@example.com", sig.email)
    }

    @Test
    fun testVersion() {
        assertEquals("0.1.0", MuonGitVersion.STRING)
        assertEquals("1.9.0", MuonGitVersion.LIBGIT2_PARITY)
    }

    @Test
    fun testObjectType() {
        assertEquals(1, ObjectType.COMMIT.value)
        assertEquals(2, ObjectType.TREE.value)
        assertEquals(3, ObjectType.BLOB.value)
        assertEquals(4, ObjectType.TAG.value)
    }

    // SHA-1 Tests

    @Test
    fun testSHA1Empty() {
        val digest = SHA1.hash(byteArrayOf())
        val hex = digest.joinToString("") { "%02x".format(it) }
        assertEquals("da39a3ee5e6b4b0d3255bfef95601890afd80709", hex)
    }

    @Test
    fun testSHA1Hello() {
        val digest = SHA1.hash("hello")
        val hex = digest.joinToString("") { "%02x".format(it) }
        assertEquals("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d", hex)
    }

    @Test
    fun testSHA1GitBlob() {
        val data = "hello\n".encodeToByteArray()
        val oid = OID.hashObject(ObjectType.BLOB, data)
        assertEquals("ce013625030ba8dba906f756967f9e9ca394464a", oid.hex)
    }

    @Test
    fun testOIDZero() {
        val z = OID.ZERO
        assertTrue(z.isZero)
        assertEquals("0000000000000000000000000000000000000000", z.hex)
    }

    // Repository Tests

    @Test
    fun testInitAndOpen() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_init")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            assertEquals(false, repo.isBare)
            assertTrue(repo.workdir != null)
            assertTrue(repo.isHeadUnborn)

            val repo2 = Repository.open(tmp.path)
            assertEquals(false, repo2.isBare)
            assertEquals("ref: refs/heads/main", repo2.head())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testInitBare() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_bare")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path, bare = true)
            assertTrue(repo.isBare)
            assertTrue(repo.workdir == null)

            val repo2 = Repository.open(tmp.path)
            assertTrue(repo2.isBare)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testOpenNonexistent() {
        try {
            Repository.open("/tmp/muongit_does_not_exist_12345")
            assertTrue(false, "should have thrown")
        } catch (_: MuonGitException.NotFound) {
            // expected
        }
    }

    @Test
    fun testDiscover() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_discover")
        tmp.deleteRecursively()
        try {
            Repository.init(tmp.path)
            val subdir = java.io.File(tmp, "a/b/c")
            subdir.mkdirs()

            val found = Repository.discover(subdir.path)
            assertEquals(false, found.isBare)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // ODB Tests

    @Test
    fun testWriteAndReadLooseObject() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_odb")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val gitDir = repo.gitDir

            val content = "hello world\n".toByteArray()
            val oid = writeLooseObject(gitDir, ObjectType.BLOB, content)

            // Verify OID matches what hashObject would produce
            val expectedOid = OID.hashObject(ObjectType.BLOB, content)
            assertEquals(expectedOid, oid)

            // Verify the object file exists on disk
            val hex = oid.hex
            val objectFile = java.io.File(gitDir, "objects/${hex.substring(0, 2)}/${hex.substring(2)}")
            assertTrue(objectFile.exists(), "loose object file should exist")

            // Read it back
            val (readType, readContent) = readLooseObject(gitDir, oid)
            assertEquals(ObjectType.BLOB, readType)
            assertTrue(content.contentEquals(readContent), "content should round-trip")

            // Write again should be idempotent (no error)
            val oid2 = writeLooseObject(gitDir, ObjectType.BLOB, content)
            assertEquals(oid, oid2)

            // Test with a different object type
            val commitData = "tree 0000000000000000000000000000000000000000\nauthor Test <test@test.com> 0 +0000\ncommitter Test <test@test.com> 0 +0000\n\ntest commit\n".toByteArray()
            val commitOid = writeLooseObject(gitDir, ObjectType.COMMIT, commitData)
            val (commitType, commitContent) = readLooseObject(gitDir, commitOid)
            assertEquals(ObjectType.COMMIT, commitType)
            assertTrue(commitData.contentEquals(commitContent))
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Refs Tests

    @Test
    fun testReadReference() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_refs")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val gitDir = repo.gitDir

            // HEAD should be a symbolic ref
            val headValue = readReference(gitDir, "HEAD")
            assertEquals("ref: refs/heads/main", headValue)

            // Write a loose ref
            val refsHeadsMain = java.io.File(gitDir, "refs/heads/main")
            val fakeOid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            refsHeadsMain.writeText("$fakeOid\n")

            val mainValue = readReference(gitDir, "refs/heads/main")
            assertEquals(fakeOid, mainValue)

            // Test resolveReference follows symbolic ref HEAD -> refs/heads/main -> OID
            val resolved = resolveReference(gitDir, "HEAD")
            assertEquals(fakeOid, resolved.hex)

            // Test packed-refs fallback
            val packedOid = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            java.io.File(gitDir, "packed-refs").writeText(
                "# pack-refs with: peeled fully-peeled sorted\n$packedOid refs/tags/v1.0\n"
            )
            val tagValue = readReference(gitDir, "refs/tags/v1.0")
            assertEquals(packedOid, tagValue)

            // Test not found
            try {
                readReference(gitDir, "refs/heads/nonexistent")
                assertTrue(false, "should have thrown")
            } catch (_: MuonGitException.NotFound) {
                // expected
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testListReferences() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_listrefs")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val gitDir = repo.gitDir

            // Create some loose refs
            val oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            val oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            java.io.File(gitDir, "refs/heads/main").writeText("$oid1\n")
            java.io.File(gitDir, "refs/heads/feature").writeText("$oid2\n")

            // Create a packed ref
            val oid3 = "ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            java.io.File(gitDir, "packed-refs").writeText(
                "# pack-refs with: peeled fully-peeled sorted\n$oid3 refs/tags/v1.0\n"
            )

            val refs = listReferences(gitDir)
            assertNotNull(refs)
            assertTrue(refs.size >= 3, "should have at least 3 refs, got ${refs.size}")

            val refMap = refs.toMap()
            assertEquals(oid2, refMap["refs/heads/feature"])
            assertEquals(oid1, refMap["refs/heads/main"])
            assertEquals(oid3, refMap["refs/tags/v1.0"])

            // Verify loose overrides packed
            val oid4 = "ddf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            java.io.File(gitDir, "refs/tags").mkdirs()
            java.io.File(gitDir, "refs/tags/v1.0").writeText("$oid4\n")

            val refs2 = listReferences(gitDir)
            val refMap2 = refs2.toMap()
            assertEquals(oid4, refMap2["refs/tags/v1.0"], "loose ref should override packed ref")
        } finally {
            tmp.deleteRecursively()
        }
    }
}
