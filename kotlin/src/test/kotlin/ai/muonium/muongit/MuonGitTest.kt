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
            val content = "hello, muongit!\n".toByteArray()
            val oid = writeLooseObject(repo.gitDir, ObjectType.BLOB, content)
            assertTrue(!oid.isZero)

            val expectedOid = OID.hashObject(ObjectType.BLOB, content)
            assertEquals(expectedOid, oid)

            val (readType, readContent) = readLooseObject(repo.gitDir, oid)
            assertEquals(ObjectType.BLOB, readType)
            assertTrue(content.contentEquals(readContent))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testWriteAndReadCommitObject() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_odb_commit")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val commitData = "tree 0000000000000000000000000000000000000000\nauthor Test <test@test.com> 0 +0000\ncommitter Test <test@test.com> 0 +0000\n\ntest commit\n".toByteArray()
            val oid = writeLooseObject(repo.gitDir, ObjectType.COMMIT, commitData)
            val (readType, readContent) = readLooseObject(repo.gitDir, oid)
            assertEquals(ObjectType.COMMIT, readType)
            assertTrue(commitData.contentEquals(readContent))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testWriteIdempotent() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_odb_idempotent")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val data = "idempotent test\n".toByteArray()
            val oid1 = writeLooseObject(repo.gitDir, ObjectType.BLOB, data)
            val oid2 = writeLooseObject(repo.gitDir, ObjectType.BLOB, data)
            assertEquals(oid1, oid2)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testReadNonexistentObject() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_odb_missing")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val fakeOid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            try {
                readLooseObject(repo.gitDir, fakeOid)
                assertTrue(false, "should have thrown")
            } catch (_: MuonGitException.NotFound) {
                // expected
            }
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
            val headValue = readReference(repo.gitDir, "HEAD")
            assertEquals("ref: refs/heads/main", headValue)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testResolveReferenceUnbornThrows() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_refs_unborn")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            try {
                resolveReference(repo.gitDir, "HEAD")
                assertTrue(false, "should have thrown")
            } catch (_: MuonGitException.NotFound) {
                // expected
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testResolveHeadWithCommit() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_refs_resolve")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val fakeOid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            java.io.File(repo.gitDir, "refs/heads/main").writeText(fakeOid)
            val resolved = resolveReference(repo.gitDir, "HEAD")
            assertEquals(fakeOid, resolved.hex)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testPackedRefs() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_refs_packed")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val packedOid = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            java.io.File(repo.gitDir, "packed-refs").writeText(
                "# pack-refs with: peeled fully-peeled sorted\n$packedOid refs/tags/v1.0\n"
            )
            val tagValue = readReference(repo.gitDir, "refs/tags/v1.0")
            assertEquals(packedOid, tagValue)
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
            val oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            val oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            java.io.File(repo.gitDir, "refs/heads/main").writeText(oid1)
            java.io.File(repo.gitDir, "refs/heads/feature").writeText(oid2)

            val refs = listReferences(repo.gitDir)
            val refMap = refs.toMap()
            assertEquals(oid1, refMap["refs/heads/main"])
            assertEquals(oid2, refMap["refs/heads/feature"])
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testLooseOverridesPacked() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_refs_override")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val packedOid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
            val looseOid = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"

            java.io.File(repo.gitDir, "packed-refs").writeText(
                "# pack-refs\n$packedOid refs/tags/v1.0\n"
            )
            java.io.File(repo.gitDir, "refs/tags").mkdirs()
            java.io.File(repo.gitDir, "refs/tags/v1.0").writeText(looseOid)

            val refs = listReferences(repo.gitDir)
            val refMap = refs.toMap()
            assertEquals(looseOid, refMap["refs/tags/v1.0"])
        } finally {
            tmp.deleteRecursively()
        }
    }
}
