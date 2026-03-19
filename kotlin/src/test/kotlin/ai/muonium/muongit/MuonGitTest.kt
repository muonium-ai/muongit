package ai.muonium.muongit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.test.assertFailsWith
import kotlin.test.assertNotEquals
import kotlin.test.assertFalse

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
        assertEquals("0.9.0", MuonGitVersion.STRING)
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

    // Commit Tests

    @Test
    fun testParseAndSerializeCommit() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val author = Signature(name = "Author", email = "author@example.com", time = 1234567890L, offset = 0)
        val committer = Signature(name = "Committer", email = "committer@example.com", time = 1234567890L, offset = 0)

        val data = serializeCommit(treeId, emptyList(), author, committer, "Initial commit\n")
        val oid = OID.hashObject(ObjectType.COMMIT, data)
        val commit = parseCommit(oid, data)

        assertEquals(treeId, commit.treeId)
        assertTrue(commit.parentIds.isEmpty())
        assertEquals("Author", commit.author.name)
        assertEquals("committer@example.com", commit.committer.email)
        assertEquals("Initial commit\n", commit.message)
        assertNull(commit.messageEncoding)
    }

    @Test
    fun testCommitWithParents() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val parent1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val parent2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val sig = Signature(name = "Test", email = "test@test.com")

        val data = serializeCommit(treeId, listOf(parent1, parent2), sig, sig, "merge\n")
        val oid = OID.hashObject(ObjectType.COMMIT, data)
        val commit = parseCommit(oid, data)

        assertEquals(2, commit.parentIds.size)
        assertEquals(parent1, commit.parentIds[0])
        assertEquals(parent2, commit.parentIds[1])
    }

    @Test
    fun testCommitMissingTreeThrows() {
        val raw = "author Test <t@t.com> 0 +0000\ncommitter Test <t@t.com> 0 +0000\n\nmsg\n".toByteArray()
        assertFailsWith<MuonGitException.InvalidObject> {
            parseCommit(OID.ZERO, raw)
        }
    }

    @Test
    fun testCommitWithEncoding() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val sig = Signature(name = "Test", email = "test@test.com", time = 100L)

        val data = serializeCommit(treeId, emptyList(), sig, sig, "msg\n", "UTF-8")
        val oid = OID.hashObject(ObjectType.COMMIT, data)
        val commit = parseCommit(oid, data)

        assertEquals("UTF-8", commit.messageEncoding)
    }

    @Test
    fun testSignatureParsing() {
        val sig = parseSignatureLine("Test User <test@example.com> 1234567890 +0530")
        assertEquals("Test User", sig.name)
        assertEquals("test@example.com", sig.email)
        assertEquals(1234567890L, sig.time)
        assertEquals(330, sig.offset) // 5*60+30
    }

    @Test
    fun testSignatureFormatNegativeOffset() {
        val sig = Signature(name = "Test", email = "test@test.com", time = 1000L, offset = -480)
        val formatted = formatSignatureLine(sig)
        assertEquals("Test <test@test.com> 1000 -0800", formatted)
    }

    @Test
    fun testCommitODBRoundTrip() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_commit_odb")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
            val sig = Signature(name = "Test", email = "test@test.com", time = 1234567890L, offset = 0)

            val commitData = serializeCommit(treeId, emptyList(), sig, sig, "test\n")
            val oid = writeLooseObject(repo.gitDir, ObjectType.COMMIT, commitData)

            val (readType, readData) = readLooseObject(repo.gitDir, oid)
            assertEquals(ObjectType.COMMIT, readType)

            val commit = parseCommit(oid, readData)
            assertEquals(treeId, commit.treeId)
            assertEquals("Test", commit.author.name)
            assertEquals("test\n", commit.message)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Tree Tests

    @Test
    fun testSerializeAndParseTree() {
        val blobOid = OID("ce013625030ba8dba906f756967f9e9ca394464a")
        val entries = listOf(TreeEntry(mode = FileMode.BLOB, name = "hello.txt", oid = blobOid))

        val data = serializeTree(entries)
        val treeOid = OID.hashObject(ObjectType.TREE, data)
        val tree = parseTree(treeOid, data)

        assertEquals(1, tree.entries.size)
        assertEquals("hello.txt", tree.entries[0].name)
        assertEquals(FileMode.BLOB, tree.entries[0].mode)
        assertEquals(blobOid, tree.entries[0].oid)
    }

    @Test
    fun testTreeMultipleEntriesSorted() {
        val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val oid3 = OID("ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        val entries = listOf(
            TreeEntry(mode = FileMode.BLOB, name = "z.txt", oid = oid1),
            TreeEntry(mode = FileMode.BLOB, name = "a.txt", oid = oid2),
            TreeEntry(mode = FileMode.TREE, name = "lib", oid = oid3),
        )

        val data = serializeTree(entries)
        val treeOid = OID.hashObject(ObjectType.TREE, data)
        val tree = parseTree(treeOid, data)

        assertEquals(3, tree.entries.size)
        assertEquals("a.txt", tree.entries[0].name)
        assertEquals("lib", tree.entries[1].name)
        assertTrue(tree.entries[1].isTree)
        assertEquals("z.txt", tree.entries[2].name)
    }

    @Test
    fun testTreeEntryTypes() {
        val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        val blob = TreeEntry(mode = FileMode.BLOB, name = "f", oid = oid)
        assertTrue(blob.isBlob)
        assertTrue(!blob.isTree)

        val exe = TreeEntry(mode = FileMode.BLOB_EXE, name = "f", oid = oid)
        assertTrue(exe.isBlob)

        val tree = TreeEntry(mode = FileMode.TREE, name = "d", oid = oid)
        assertTrue(tree.isTree)
        assertTrue(!tree.isBlob)
    }

    @Test
    fun testParseEmptyTree() {
        val oid = OID.hashObject(ObjectType.TREE, byteArrayOf())
        val tree = parseTree(oid, byteArrayOf())
        assertTrue(tree.entries.isEmpty())
    }

    @Test
    fun testTreeODBRoundTrip() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_tree_odb")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val blobOid = OID("ce013625030ba8dba906f756967f9e9ca394464a")
            val entries = listOf(TreeEntry(mode = FileMode.BLOB, name = "file.txt", oid = blobOid))

            val treeData = serializeTree(entries)
            val oid = writeLooseObject(repo.gitDir, ObjectType.TREE, treeData)

            val (readType, readData) = readLooseObject(repo.gitDir, oid)
            assertEquals(ObjectType.TREE, readType)

            val tree = parseTree(oid, readData)
            assertEquals(1, tree.entries.size)
            assertEquals("file.txt", tree.entries[0].name)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Blob Tests

    @Test
    fun testHashBlob() {
        val oid = hashBlob("hello\n".toByteArray())
        assertEquals("ce013625030ba8dba906f756967f9e9ca394464a", oid.hex)
    }

    @Test
    fun testHashBlobEmpty() {
        val oid = hashBlob(byteArrayOf())
        assertEquals("e69de29bb2d1d6434b8b29ae775ad8c2e48c5391", oid.hex)
    }

    @Test
    fun testWriteAndReadBlob() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_blob_rw")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val content = "blob content\n".toByteArray()
            val oid = writeBlob(repo.gitDir, content)
            val blob = readBlob(repo.gitDir, oid)

            assertTrue(blob.data.contentEquals(content))
            assertEquals(content.size, blob.size)
            assertEquals(oid, blob.oid)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testWriteBlobFromFile() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_blob_file")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val filePath = java.io.File(tmp, "test.txt")
            filePath.writeText("file content\n")

            val oid = writeBlobFromFile(repo.gitDir, filePath.path)
            val expected = hashBlob("file content\n".toByteArray())
            assertEquals(expected, oid)

            val blob = readBlob(repo.gitDir, oid)
            assertEquals("file content\n", String(blob.data))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testReadNonBlobTypeErrors() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_blob_type_err")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val commitData = "tree 0000000000000000000000000000000000000000\nauthor T <t@t> 0 +0000\ncommitter T <t@t> 0 +0000\n\nm\n".toByteArray()
            val oid = writeLooseObject(repo.gitDir, ObjectType.COMMIT, commitData)

            assertFailsWith<MuonGitException.InvalidObject> {
                readBlob(repo.gitDir, oid)
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Tag Tests

    @Test
    fun testParseAndSerializeTag() {
        val targetId = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val tagger = Signature(name = "Tagger", email = "tagger@example.com", time = 1234567890L, offset = 0)

        val data = serializeTag(targetId, ObjectType.COMMIT, "v1.0", tagger, "Release v1.0\n")
        val oid = OID.hashObject(ObjectType.TAG, data)
        val tag = parseTag(oid, data)

        assertEquals(targetId, tag.targetId)
        assertEquals(ObjectType.COMMIT, tag.targetType)
        assertEquals("v1.0", tag.tagName)
        assertEquals("Tagger", tag.tagger?.name)
        assertEquals("Release v1.0\n", tag.message)
    }

    @Test
    fun testTagWithoutTagger() {
        val targetId = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val data = serializeTag(targetId, ObjectType.COMMIT, "v0.1", null, "lightweight\n")
        val oid = OID.hashObject(ObjectType.TAG, data)
        val tag = parseTag(oid, data)

        assertNull(tag.tagger)
        assertEquals("v0.1", tag.tagName)
    }

    @Test
    fun testTagMissingObjectThrows() {
        val raw = "type commit\ntag v1\n\nmsg\n".toByteArray()
        assertFailsWith<MuonGitException.InvalidObject> {
            parseTag(OID.ZERO, raw)
        }
    }

    @Test
    fun testTagTargetingTree() {
        val targetId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val data = serializeTag(targetId, ObjectType.TREE, "tree-tag", null, "tag a tree\n")
        val oid = OID.hashObject(ObjectType.TAG, data)
        val tag = parseTag(oid, data)

        assertEquals(ObjectType.TREE, tag.targetType)
    }

    @Test
    fun testTagODBRoundTrip() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_tag_odb")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val targetId = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val tagger = Signature(name = "T", email = "t@t.com", time = 100L, offset = 0)
            val tagData = serializeTag(targetId, ObjectType.COMMIT, "v1.0", tagger, "msg\n")
            val oid = writeLooseObject(repo.gitDir, ObjectType.TAG, tagData)

            val (readType, readData) = readLooseObject(repo.gitDir, oid)
            assertEquals(ObjectType.TAG, readType)

            val tag = parseTag(oid, readData)
            assertEquals("v1.0", tag.tagName)
            assertEquals(targetId, tag.targetId)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Ref Write/Update/Delete Tests

    @Test
    fun testWriteAndReadReference() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_ref_write")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            writeReference(repo.gitDir, "refs/heads/feature", oid)

            val value = readReference(repo.gitDir, "refs/heads/feature")
            assertEquals(oid.hex, value)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testWriteSymbolicReference() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_ref_sym")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            writeSymbolicReference(repo.gitDir, "refs/heads/alias", "refs/heads/main")

            val value = readReference(repo.gitDir, "refs/heads/alias")
            assertEquals("ref: refs/heads/main", value)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testDeleteReference() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_ref_delete")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            writeReference(repo.gitDir, "refs/heads/feature", oid)

            assertTrue(deleteReference(repo.gitDir, "refs/heads/feature"))
            assertFailsWith<MuonGitException.NotFound> {
                readReference(repo.gitDir, "refs/heads/feature")
            }
            assertTrue(!deleteReference(repo.gitDir, "refs/heads/nonexistent"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testUpdateReferenceSuccess() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_ref_update")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

            updateReference(repo.gitDir, "refs/heads/feature", oid1, OID.ZERO)
            assertEquals(oid1.hex, readReference(repo.gitDir, "refs/heads/feature"))

            updateReference(repo.gitDir, "refs/heads/feature", oid2, oid1)
            assertEquals(oid2.hex, readReference(repo.gitDir, "refs/heads/feature"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testUpdateReferenceConflict() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_ref_cas")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val oidWrong = OID("ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

            writeReference(repo.gitDir, "refs/heads/feature", oid1)

            assertFailsWith<MuonGitException.Conflict> {
                updateReference(repo.gitDir, "refs/heads/feature", oid2, oidWrong)
            }
            assertFailsWith<MuonGitException.Conflict> {
                updateReference(repo.gitDir, "refs/heads/feature", oid2, OID.ZERO)
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Config Tests

    @Test
    fun testParseSimpleConfig() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_config_parse")
        tmp.deleteRecursively()
        tmp.mkdirs()
        try {
            val configFile = java.io.File(tmp, "config")
            configFile.writeText("[core]\n\tbare = false\n\trepositoryformatversion = 0\n")

            val config = Config.load(configFile.path)
            assertEquals("false", config.get("core", "bare"))
            assertEquals(false, config.getBool("core", "bare"))
            assertEquals(0, config.getInt("core", "repositoryformatversion"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testConfigSubsection() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_config_sub")
        tmp.deleteRecursively()
        tmp.mkdirs()
        try {
            val configFile = java.io.File(tmp, "config")
            configFile.writeText("[remote \"origin\"]\n\turl = https://example.com/repo.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n")

            val config = Config.load(configFile.path)
            assertEquals("https://example.com/repo.git", config.get("remote.origin", "url"))
            assertEquals("+refs/heads/*:refs/remotes/origin/*", config.get("remote.origin", "fetch"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testConfigSetAndUnset() {
        val config = Config()
        config.set("core", "bare", "true")
        assertEquals("true", config.get("core", "bare"))

        config.set("core", "bare", "false")
        assertEquals("false", config.get("core", "bare"))

        config.unset("core", "bare")
        assertNull(config.get("core", "bare"))
    }

    @Test
    fun testConfigCaseInsensitive() {
        val config = Config()
        config.set("Core", "Bare", "true")
        assertEquals("true", config.get("core", "bare"))
        assertEquals("true", config.get("CORE", "BARE"))
    }

    @Test
    fun testConfigIntSuffixes() {
        assertEquals(42, parseConfigInt("42"))
        assertEquals(1024, parseConfigInt("1k"))
        assertEquals(2 * 1024 * 1024, parseConfigInt("2m"))
        assertEquals(1024 * 1024 * 1024, parseConfigInt("1g"))
    }

    @Test
    fun testConfigRoundTrip() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_config_rt")
        tmp.deleteRecursively()
        tmp.mkdirs()
        try {
            val configFile = java.io.File(tmp, "config")
            val config = Config(configFile.path)
            config.set("core", "bare", "false")
            config.set("core", "repositoryformatversion", "0")
            config.set("remote.origin", "url", "https://example.com/repo.git")
            config.save()

            val loaded = Config.load(configFile.path)
            assertEquals("false", loaded.get("core", "bare"))
            assertEquals("https://example.com/repo.git", loaded.get("remote.origin", "url"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRepoConfig() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_config_repo")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val config = Config.load(java.io.File(repo.gitDir, "config").path)
            assertEquals(false, config.getBool("core", "bare"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Reflog Tests

    @Test
    fun testParseReflogEntry() {
        val content = "0000000000000000000000000000000000000000 aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d Test <test@test.com> 1234567890 +0000\tcommit (initial): first commit\n"
        val entries = parseReflog(content)
        assertEquals(1, entries.size)
        assertTrue(entries[0].oldOid.isZero)
        assertEquals("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d", entries[0].newOid.hex)
        assertEquals("Test", entries[0].committer.name)
        assertEquals("commit (initial): first commit", entries[0].message)
    }

    @Test
    fun testAppendAndReadReflog() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_reflog_rw")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val zero = OID.ZERO
            val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val sig = Signature(name = "Test", email = "t@t.com", time = 100L, offset = 0)

            appendReflog(repo.gitDir, "HEAD", zero, oid1, sig, "commit (initial): first")
            appendReflog(repo.gitDir, "HEAD", oid1, oid2, sig, "commit: second")

            val entries = readReflog(repo.gitDir, "HEAD")
            assertEquals(2, entries.size)
            assertTrue(entries[0].oldOid.isZero)
            assertEquals(oid1, entries[0].newOid)
            assertEquals("commit (initial): first", entries[0].message)
            assertEquals(oid1, entries[1].oldOid)
            assertEquals(oid2, entries[1].newOid)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testReadNonexistentReflog() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_reflog_empty")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val entries = readReflog(repo.gitDir, "HEAD")
            assertTrue(entries.isEmpty())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testReflogForBranch() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_reflog_branch")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val sig = Signature(name = "T", email = "t@t", time = 0L, offset = 0)

            appendReflog(repo.gitDir, "refs/heads/main", OID.ZERO, oid, sig, "branch: Created")

            val entries = readReflog(repo.gitDir, "refs/heads/main")
            assertEquals(1, entries.size)
            assertEquals("branch: Created", entries[0].message)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Index Tests

    @Test
    fun testReadWriteEmptyIndex() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_index_empty")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val index = Index()
            writeIndex(repo.gitDir, index)

            val loaded = readIndex(repo.gitDir)
            assertEquals(2, loaded.version)
            assertTrue(loaded.entries.isEmpty())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testReadWriteSingleEntry() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_index_single")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("ce013625030ba8dba906f756967f9e9ca394464a")
            val entry = IndexEntry(mode = 33188, fileSize = 6, oid = oid, path = "hello.txt")

            val index = Index()
            index.add(entry)
            writeIndex(repo.gitDir, index)

            val loaded = readIndex(repo.gitDir)
            assertEquals(1, loaded.entries.size)
            assertEquals("hello.txt", loaded.entries[0].path)
            assertEquals(33188, loaded.entries[0].mode) // 0o100644
            assertEquals(oid, loaded.entries[0].oid)
            assertEquals(6, loaded.entries[0].fileSize)
            assertEquals(9, loaded.entries[0].flags and 0xFFF) // "hello.txt".length
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testReadWriteMultipleEntriesSorted() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_index_multi")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

            val index = Index()
            index.add(IndexEntry(mode = 33188, oid = oid, path = "z.txt"))
            index.add(IndexEntry(mode = 33188, oid = oid, path = "a.txt"))
            index.add(IndexEntry(mode = 33188, oid = oid, path = "lib/main.c"))
            writeIndex(repo.gitDir, index)

            val loaded = readIndex(repo.gitDir)
            assertEquals(3, loaded.entries.size)
            assertEquals("a.txt", loaded.entries[0].path)
            assertEquals("lib/main.c", loaded.entries[1].path)
            assertEquals("z.txt", loaded.entries[2].path)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testIndexAddRemoveFind() {
        val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val index = Index()
        index.add(IndexEntry(mode = 33188, oid = oid, path = "foo.txt"))
        index.add(IndexEntry(mode = 33188, oid = oid, path = "bar.txt"))

        assertNotNull(index.find("foo.txt"))
        assertNull(index.find("nonexistent"))

        assertTrue(index.remove("foo.txt"))
        assertTrue(!index.remove("foo.txt"))
        assertNull(index.find("foo.txt"))
        assertEquals(1, index.entries.size)
    }

    @Test
    fun testIndexChecksumValidation() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_index_checksum")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val index = Index()
            index.add(IndexEntry(mode = 33188, fileSize = 10, oid = oid, path = "test.txt"))
            writeIndex(repo.gitDir, index)

            // Corrupt the data
            val indexFile = java.io.File(repo.gitDir, "index")
            val data = indexFile.readBytes()
            data[20] = (data[20].toInt() xor 0xFF).toByte()
            indexFile.writeBytes(data)

            assertFailsWith<MuonGitException.InvalidObject> {
                readIndex(repo.gitDir)
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Diff Tests

    private fun treeEntry(name: String, hex: String, mode: Int) =
        TreeEntry(mode = mode, name = name, oid = OID(hex))

    @Test
    fun testDiffIdenticalTrees() {
        val oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val entries = listOf(treeEntry("a.txt", oid, FileMode.BLOB), treeEntry("b.txt", oid, FileMode.BLOB))
        val deltas = diffTrees(entries, entries)
        assertTrue(deltas.isEmpty())
    }

    @Test
    fun testDiffAddedFile() {
        val oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val old = listOf(treeEntry("a.txt", oid, FileMode.BLOB))
        val new = listOf(treeEntry("a.txt", oid, FileMode.BLOB), treeEntry("b.txt", oid, FileMode.BLOB))
        val deltas = diffTrees(old, new)
        assertEquals(1, deltas.size)
        assertEquals(DiffStatus.ADDED, deltas[0].status)
        assertEquals("b.txt", deltas[0].path)
        assertNull(deltas[0].oldEntry)
        assertNotNull(deltas[0].newEntry)
    }

    @Test
    fun testDiffDeletedFile() {
        val oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val old = listOf(treeEntry("a.txt", oid, FileMode.BLOB), treeEntry("b.txt", oid, FileMode.BLOB))
        val new = listOf(treeEntry("a.txt", oid, FileMode.BLOB))
        val deltas = diffTrees(old, new)
        assertEquals(1, deltas.size)
        assertEquals(DiffStatus.DELETED, deltas[0].status)
        assertEquals("b.txt", deltas[0].path)
        assertNotNull(deltas[0].oldEntry)
        assertNull(deltas[0].newEntry)
    }

    @Test
    fun testDiffModifiedFile() {
        val oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val old = listOf(treeEntry("a.txt", oid1, FileMode.BLOB))
        val new = listOf(treeEntry("a.txt", oid2, FileMode.BLOB))
        val deltas = diffTrees(old, new)
        assertEquals(1, deltas.size)
        assertEquals(DiffStatus.MODIFIED, deltas[0].status)
        assertEquals("a.txt", deltas[0].path)
        assertNotNull(deltas[0].oldEntry)
        assertNotNull(deltas[0].newEntry)
    }

    @Test
    fun testDiffModeChange() {
        val oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val old = listOf(treeEntry("script.sh", oid, FileMode.BLOB))
        val new = listOf(treeEntry("script.sh", oid, FileMode.BLOB_EXE))
        val deltas = diffTrees(old, new)
        assertEquals(1, deltas.size)
        assertEquals(DiffStatus.MODIFIED, deltas[0].status)
    }

    @Test
    fun testDiffEmptyToFull() {
        val oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val new = listOf(treeEntry("a.txt", oid, FileMode.BLOB), treeEntry("b.txt", oid, FileMode.BLOB))
        val deltas = diffTrees(emptyList(), new)
        assertEquals(2, deltas.size)
        assertTrue(deltas.all { it.status == DiffStatus.ADDED })
    }

    @Test
    fun testDiffFullToEmpty() {
        val oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val old = listOf(treeEntry("a.txt", oid, FileMode.BLOB), treeEntry("b.txt", oid, FileMode.BLOB))
        val deltas = diffTrees(old, emptyList())
        assertEquals(2, deltas.size)
        assertTrue(deltas.all { it.status == DiffStatus.DELETED })
    }

    @Test
    fun testDiffMixedChanges() {
        val oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val old = listOf(
            treeEntry("a.txt", oid1, FileMode.BLOB),
            treeEntry("b.txt", oid1, FileMode.BLOB),
            treeEntry("c.txt", oid1, FileMode.BLOB),
        )
        val new = listOf(
            treeEntry("a.txt", oid1, FileMode.BLOB), // unchanged
            treeEntry("b.txt", oid2, FileMode.BLOB), // modified
            treeEntry("d.txt", oid1, FileMode.BLOB), // added
        )
        val deltas = diffTrees(old, new)
        assertEquals(3, deltas.size)
        assertEquals(DiffStatus.MODIFIED, deltas[0].status)
        assertEquals("b.txt", deltas[0].path)
        assertEquals(DiffStatus.DELETED, deltas[1].status)
        assertEquals("c.txt", deltas[1].path)
        assertEquals(DiffStatus.ADDED, deltas[2].status)
        assertEquals("d.txt", deltas[2].path)
    }

    // Status Tests

    private fun makeStatusIndexEntry(path: String, oid: OID, fileSize: Int) =
        IndexEntry(mode = 33188, fileSize = fileSize, oid = oid, path = path)

    @Test
    fun testStatusCleanWorkdir() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_status_clean")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val content = "hello\n".toByteArray()
            java.io.File(tmp, "hello.txt").writeBytes(content)

            val oid = OID.hashObject(ObjectType.BLOB, content)
            val index = Index()
            index.add(makeStatusIndexEntry("hello.txt", oid, content.size))
            writeIndex(repo.gitDir, index)

            val status = workdirStatus(repo.gitDir, tmp)
            assertTrue(status.isEmpty())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testStatusModifiedFile() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_status_modified")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val content = "hello\n".toByteArray()
            java.io.File(tmp, "hello.txt").writeBytes(content)

            val oid = OID.hashObject(ObjectType.BLOB, content)
            val index = Index()
            index.add(makeStatusIndexEntry("hello.txt", oid, content.size))
            writeIndex(repo.gitDir, index)

            // Modify the file
            java.io.File(tmp, "hello.txt").writeText("changed\n")

            val status = workdirStatus(repo.gitDir, tmp)
            assertEquals(1, status.size)
            assertEquals("hello.txt", status[0].path)
            assertEquals(FileStatus.MODIFIED, status[0].status)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testStatusDeletedFile() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_status_deleted")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val content = "hello\n".toByteArray()
            val oid = OID.hashObject(ObjectType.BLOB, content)
            val index = Index()
            index.add(makeStatusIndexEntry("hello.txt", oid, content.size))
            writeIndex(repo.gitDir, index)

            // Don't create the file
            val status = workdirStatus(repo.gitDir, tmp)
            assertEquals(1, status.size)
            assertEquals("hello.txt", status[0].path)
            assertEquals(FileStatus.DELETED, status[0].status)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testStatusNewFile() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_status_new")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val index = Index()
            writeIndex(repo.gitDir, index)

            java.io.File(tmp, "new.txt").writeText("new\n")

            val status = workdirStatus(repo.gitDir, tmp)
            assertEquals(1, status.size)
            assertEquals("new.txt", status[0].path)
            assertEquals(FileStatus.NEW, status[0].status)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testStatusMixed() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_status_mixed")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val contentA = "aaa\n".toByteArray()
            val contentB = "bbb\n".toByteArray()
            val oidA = OID.hashObject(ObjectType.BLOB, contentA)
            val oidB = OID.hashObject(ObjectType.BLOB, contentB)

            val index = Index()
            index.add(makeStatusIndexEntry("a.txt", oidA, contentA.size))
            index.add(makeStatusIndexEntry("b.txt", oidB, contentB.size))
            index.add(makeStatusIndexEntry("c.txt", oidA, contentA.size))
            writeIndex(repo.gitDir, index)

            // a.txt: unchanged
            java.io.File(tmp, "a.txt").writeBytes(contentA)
            // b.txt: modified
            java.io.File(tmp, "b.txt").writeText("modified\n")
            // c.txt: deleted (not created)
            // d.txt: new
            java.io.File(tmp, "d.txt").writeText("new\n")

            val status = workdirStatus(repo.gitDir, tmp)

            val modified = status.filter { it.status == FileStatus.MODIFIED }
            val deleted = status.filter { it.status == FileStatus.DELETED }
            val new = status.filter { it.status == FileStatus.NEW }

            assertEquals(1, modified.size)
            assertEquals("b.txt", modified[0].path)
            assertEquals(1, deleted.size)
            assertEquals("c.txt", deleted[0].path)
            assertEquals(1, new.size)
            assertEquals("d.txt", new[0].path)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Diff Formatting Tests

    @Test
    fun testDiffLinesIdentical() {
        val edits = diffLines("a\nb\nc\n", "a\nb\nc\n")
        assertTrue(edits.all { it.kind == EditKind.EQUAL })
    }

    @Test
    fun testDiffLinesInsert() {
        val edits = diffLines("a\nc\n", "a\nb\nc\n")
        val inserts = edits.filter { it.kind == EditKind.INSERT }
        assertEquals(1, inserts.size)
        assertEquals("b", inserts[0].text)
    }

    @Test
    fun testDiffLinesDelete() {
        val edits = diffLines("a\nb\nc\n", "a\nc\n")
        val deletes = edits.filter { it.kind == EditKind.DELETE }
        assertEquals(1, deletes.size)
        assertEquals("b", deletes[0].text)
    }

    @Test
    fun testDiffLinesModify() {
        val edits = diffLines("a\nb\nc\n", "a\nB\nc\n")
        val deletes = edits.filter { it.kind == EditKind.DELETE }
        val inserts = edits.filter { it.kind == EditKind.INSERT }
        assertEquals(1, deletes.size)
        assertEquals("b", deletes[0].text)
        assertEquals(1, inserts.size)
        assertEquals("B", inserts[0].text)
    }

    @Test
    fun testFormatPatchBasic() {
        val old = "line1\nline2\nline3\n"
        val new = "line1\nmodified\nline3\n"
        val patch = formatPatch("file.txt", "file.txt", old, new)
        assertTrue(patch.contains("--- a/file.txt"))
        assertTrue(patch.contains("+++ b/file.txt"))
        assertTrue(patch.contains("@@"))
        assertTrue(patch.contains("-line2"))
        assertTrue(patch.contains("+modified"))
    }

    @Test
    fun testFormatPatchNoChanges() {
        val text = "same\n"
        val patch = formatPatch("f.txt", "f.txt", text, text)
        assertTrue(patch.isEmpty())
    }

    @Test
    fun testFormatPatchAddedFile() {
        val patch = formatPatch("new.txt", "new.txt", "", "hello\nworld\n")
        assertTrue(patch.contains("+hello"))
        assertTrue(patch.contains("+world"))
    }

    @Test
    fun testFormatPatchDeletedFile() {
        val patch = formatPatch("old.txt", "old.txt", "goodbye\nworld\n", "")
        assertTrue(patch.contains("-goodbye"))
        assertTrue(patch.contains("-world"))
    }

    @Test
    fun testDiffStatBasic() {
        val stat = diffStat("file.txt", "a\nb\nc\n", "a\nB\nc\nd\n")
        assertEquals("file.txt", stat.path)
        assertEquals(1, stat.deletions)
        assertEquals(2, stat.insertions)
    }

    @Test
    fun testFormatStatOutput() {
        val stats = listOf(
            DiffStatEntry("file.txt", 3, 1),
            DiffStatEntry("other.rs", 0, 5),
        )
        val output = formatStat(stats)
        assertTrue(output.contains("file.txt"))
        assertTrue(output.contains("other.rs"))
        assertTrue(output.contains("2 files changed"))
        assertTrue(output.contains("3 insertions(+)"))
        assertTrue(output.contains("6 deletions(-)"))
    }

    @Test
    fun testFormatStatEmpty() {
        val output = formatStat(emptyList())
        assertTrue(output.isEmpty())
    }

    // Index-to-Workdir Diff Tests

    @Test
    fun testDiffWorkdirClean() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_diff_workdir_clean")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val content = "hello\n".toByteArray()
            java.io.File(tmp, "hello.txt").writeBytes(content)

            val oid = OID.hashObject(ObjectType.BLOB, content)
            val index = Index()
            index.add(makeStatusIndexEntry("hello.txt", oid, content.size))
            writeIndex(repo.gitDir, index)

            val deltas = diffIndexToWorkdir(repo.gitDir, tmp)
            assertTrue(deltas.isEmpty())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testDiffWorkdirModified() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_diff_workdir_mod")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val content = "hello\n".toByteArray()
            java.io.File(tmp, "hello.txt").writeBytes(content)

            val oid = OID.hashObject(ObjectType.BLOB, content)
            val index = Index()
            index.add(makeStatusIndexEntry("hello.txt", oid, content.size))
            writeIndex(repo.gitDir, index)

            // Modify the file
            java.io.File(tmp, "hello.txt").writeText("changed\n")

            val deltas = diffIndexToWorkdir(repo.gitDir, tmp)
            assertEquals(1, deltas.size)
            assertEquals(DiffStatus.MODIFIED, deltas[0].status)
            assertEquals("hello.txt", deltas[0].path)
            assertNotNull(deltas[0].oldEntry)
            assertNotNull(deltas[0].newEntry)
            assertNotEquals(deltas[0].oldEntry!!.oid, deltas[0].newEntry!!.oid)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testDiffWorkdirDeleted() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_diff_workdir_del")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val content = "hello\n".toByteArray()
            val oid = OID.hashObject(ObjectType.BLOB, content)
            val index = Index()
            index.add(makeStatusIndexEntry("hello.txt", oid, content.size))
            writeIndex(repo.gitDir, index)

            // Don't create the file — it's deleted
            val deltas = diffIndexToWorkdir(repo.gitDir, tmp)
            assertEquals(1, deltas.size)
            assertEquals(DiffStatus.DELETED, deltas[0].status)
            assertEquals("hello.txt", deltas[0].path)
            assertNotNull(deltas[0].oldEntry)
            assertNull(deltas[0].newEntry)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testDiffWorkdirNewFile() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_diff_workdir_new")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val index = Index()
            writeIndex(repo.gitDir, index)

            // Create a file not in the index
            java.io.File(tmp, "new.txt").writeText("new\n")

            val deltas = diffIndexToWorkdir(repo.gitDir, tmp)
            assertEquals(1, deltas.size)
            assertEquals(DiffStatus.ADDED, deltas[0].status)
            assertEquals("new.txt", deltas[0].path)
            assertNull(deltas[0].oldEntry)
            assertNotNull(deltas[0].newEntry)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testDiffWorkdirMixed() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_diff_workdir_mixed")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val contentA = "aaa\n".toByteArray()
            val contentB = "bbb\n".toByteArray()
            val oidA = OID.hashObject(ObjectType.BLOB, contentA)
            val oidB = OID.hashObject(ObjectType.BLOB, contentB)

            val index = Index()
            index.add(makeStatusIndexEntry("a.txt", oidA, contentA.size))
            index.add(makeStatusIndexEntry("b.txt", oidB, contentB.size))
            index.add(makeStatusIndexEntry("c.txt", oidA, contentA.size))
            writeIndex(repo.gitDir, index)

            // a.txt: unchanged
            java.io.File(tmp, "a.txt").writeBytes(contentA)
            // b.txt: modified
            java.io.File(tmp, "b.txt").writeText("modified\n")
            // c.txt: deleted (not created)
            // d.txt: new
            java.io.File(tmp, "d.txt").writeText("new\n")

            val deltas = diffIndexToWorkdir(repo.gitDir, tmp)

            val modified = deltas.filter { it.status == DiffStatus.MODIFIED }
            val deleted = deltas.filter { it.status == DiffStatus.DELETED }
            val added = deltas.filter { it.status == DiffStatus.ADDED }

            assertEquals(1, modified.size)
            assertEquals("b.txt", modified[0].path)
            assertEquals(1, deleted.size)
            assertEquals("c.txt", deleted[0].path)
            assertEquals(1, added.size)
            assertEquals("d.txt", added[0].path)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Ignore / Glob Tests

    @Test
    fun testGlobMatchBasic() {
        assertTrue(globMatch("*.txt", "hello.txt"))
        assertFalse(globMatch("*.txt", "hello.rs"))
        assertTrue(globMatch("hello.*", "hello.txt"))
        assertTrue(globMatch("?ello.txt", "hello.txt"))
        assertFalse(globMatch("?ello.txt", "hhello.txt"))
    }

    @Test
    fun testGlobMatchStarNoSlash() {
        assertFalse(globMatch("*.txt", "dir/hello.txt"))
        assertTrue(globMatch("*.txt", "hello.txt"))
    }

    @Test
    fun testGlobMatchDoubleStar() {
        assertTrue(globMatch("**/*.txt", "hello.txt"))
        assertTrue(globMatch("**/*.txt", "dir/hello.txt"))
        assertTrue(globMatch("**/*.txt", "a/b/c/hello.txt"))
        assertTrue(globMatch("**/build", "build"))
        assertTrue(globMatch("**/build", "src/build"))
    }

    @Test
    fun testGlobMatchCharClass() {
        assertTrue(globMatch("[abc].txt", "a.txt"))
        assertTrue(globMatch("[abc].txt", "b.txt"))
        assertFalse(globMatch("[abc].txt", "d.txt"))
        assertTrue(globMatch("[a-z].txt", "m.txt"))
        assertFalse(globMatch("[a-z].txt", "M.txt"))
        assertTrue(globMatch("[!abc].txt", "d.txt"))
        assertFalse(globMatch("[!abc].txt", "a.txt"))
    }

    @Test
    fun testIgnoreBasic() {
        val ignore = Ignore()
        ignore.addPatterns("*.o\n*.log\nbuild/\n", "")

        assertTrue(ignore.isIgnored("main.o", false))
        assertTrue(ignore.isIgnored("debug.log", false))
        assertTrue(ignore.isIgnored("src/test.o", false))
        assertFalse(ignore.isIgnored("main.c", false))
        assertTrue(ignore.isIgnored("build", true))
        assertFalse(ignore.isIgnored("build", false))
    }

    @Test
    fun testIgnoreNegation() {
        val ignore = Ignore()
        ignore.addPatterns("*.log\n!important.log\n", "")

        assertTrue(ignore.isIgnored("debug.log", false))
        assertFalse(ignore.isIgnored("important.log", false))
    }

    @Test
    fun testIgnoreDoubleStar() {
        val ignore = Ignore()
        ignore.addPatterns("**/build\nlogs/**/*.log\n", "")

        assertTrue(ignore.isIgnored("build", false))
        assertTrue(ignore.isIgnored("src/build", false))
        assertTrue(ignore.isIgnored("logs/2024/error.log", false))
    }

    @Test
    fun testIgnoreWithPath() {
        val ignore = Ignore()
        ignore.addPatterns("doc/*.html\n", "")

        assertTrue(ignore.isIgnored("doc/index.html", false))
        assertFalse(ignore.isIgnored("src/index.html", false))
    }

    @Test
    fun testIgnoreLoadFromRepo() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_ignore_load_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val workdir = repo.workdir!!

            java.io.File(workdir, ".gitignore").writeText("*.o\nbuild/\n")

            val ignore = Ignore.load(repo.gitDir, workdir)
            assertTrue(ignore.isIgnored("main.o", false))
            assertTrue(ignore.isIgnored("build", true))
            assertFalse(ignore.isIgnored("main.c", false))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testIgnoreSubdir() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_ignore_subdir_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val workdir = repo.workdir!!

            java.io.File(workdir, ".gitignore").writeText("*.o\n")
            val vendorDir = java.io.File(workdir, "vendor")
            vendorDir.mkdirs()
            java.io.File(vendorDir, ".gitignore").writeText("*.tmp\n")

            val ignore = Ignore.load(repo.gitDir, workdir)
            ignore.loadForPath(workdir, "vendor")

            assertTrue(ignore.isIgnored("main.o", false))
            assertTrue(ignore.isIgnored("vendor/cache.tmp", false))
            assertFalse(ignore.isIgnored("src/cache.tmp", false))
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Merge Base Tests

    private fun makeCommit(gitDir: java.io.File, treeOid: OID, parents: List<OID>, msg: String): OID {
        val sb = StringBuilder()
        sb.append("tree ${treeOid.hex}\n")
        for (p in parents) sb.append("parent ${p.hex}\n")
        sb.append("author Test <test@test.com> 1000000000 +0000\n")
        sb.append("committer Test <test@test.com> 1000000000 +0000\n")
        sb.append("\n$msg")
        return writeLooseObject(gitDir, ObjectType.COMMIT, sb.toString().toByteArray())
    }

    private fun makeEmptyTree(gitDir: java.io.File): OID {
        return writeLooseObject(gitDir, ObjectType.TREE, ByteArray(0))
    }

    @Test
    fun testMergeBaseSameCommit() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_mb_same_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val tree = makeEmptyTree(repo.gitDir)
            val c1 = makeCommit(repo.gitDir, tree, emptyList(), "initial")

            val result = mergeBase(repo.gitDir, c1, c1)
            assertEquals(c1, result)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testMergeBaseLinearHistory() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_mb_linear_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val tree = makeEmptyTree(repo.gitDir)
            val a = makeCommit(repo.gitDir, tree, emptyList(), "A")
            val b = makeCommit(repo.gitDir, tree, listOf(a), "B")
            val c = makeCommit(repo.gitDir, tree, listOf(b), "C")

            assertEquals(b, mergeBase(repo.gitDir, b, c))
            assertEquals(a, mergeBase(repo.gitDir, a, c))
            assertEquals(a, mergeBase(repo.gitDir, a, b))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testMergeBaseForkAndMerge() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_mb_fork_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val tree = makeEmptyTree(repo.gitDir)
            val a = makeCommit(repo.gitDir, tree, emptyList(), "A")
            val b = makeCommit(repo.gitDir, tree, listOf(a), "B")
            val c = makeCommit(repo.gitDir, tree, listOf(a), "C")
            val d = makeCommit(repo.gitDir, tree, listOf(b, c), "D")

            assertEquals(a, mergeBase(repo.gitDir, b, c))
            assertEquals(b, mergeBase(repo.gitDir, b, d))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testMergeBaseNoCommonAncestor() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_mb_disjoint_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val tree = makeEmptyTree(repo.gitDir)
            val a = makeCommit(repo.gitDir, tree, emptyList(), "A")
            val b = makeCommit(repo.gitDir, tree, listOf(a), "B")
            val c = makeCommit(repo.gitDir, tree, emptyList(), "C")
            val d = makeCommit(repo.gitDir, tree, listOf(c), "D")

            assertNull(mergeBase(repo.gitDir, b, d))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testMergeBasesMultiple() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_mb_multi_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val tree = makeEmptyTree(repo.gitDir)
            val a = makeCommit(repo.gitDir, tree, emptyList(), "A")
            val b = makeCommit(repo.gitDir, tree, listOf(a), "B")
            val c = makeCommit(repo.gitDir, tree, listOf(a), "C")

            val bases = mergeBases(repo.gitDir, b, c)
            assertEquals(1, bases.size)
            assertEquals(a, bases[0])
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Checkout Tests

    private fun addBlobToIndex(gitDir: java.io.File, index: Index, path: String, content: ByteArray, executable: Boolean) {
        val oid = writeLooseObject(gitDir, ObjectType.BLOB, content)
        val mode = if (executable) 0b001000000_111_101_101 else 0b001000000_110_100_100 // 0o100755 / 0o100644
        index.entries.add(IndexEntry(
            mode = mode, oid = oid, fileSize = content.size,
            flags = minOf(path.length, 0xFFF), path = path
        ))
        index.entries.sortBy { it.path }
    }

    @Test
    fun testCheckoutBasic() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_checkout_basic_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val workdir = repo.workdir!!
            val index = Index()
            addBlobToIndex(repo.gitDir, index, "hello.txt", "Hello, world!\n".toByteArray(), false)
            addBlobToIndex(repo.gitDir, index, "src/main.rs", "fn main() {}\n".toByteArray(), false)
            writeIndex(repo.gitDir, index)

            val result = checkoutIndex(repo.gitDir, workdir, CheckoutOptions(force = true))
            assertEquals(2, result.updated.size)
            assertTrue(result.conflicts.isEmpty())
            assertEquals("Hello, world!\n", java.io.File(workdir, "hello.txt").readText())
            assertEquals("fn main() {}\n", java.io.File(workdir, "src/main.rs").readText())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCheckoutCreatesDirectories() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_checkout_dirs_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val workdir = repo.workdir!!
            val index = Index()
            addBlobToIndex(repo.gitDir, index, "a/b/c/deep.txt", "deep content".toByteArray(), false)
            writeIndex(repo.gitDir, index)

            val result = checkoutIndex(repo.gitDir, workdir, CheckoutOptions(force = true))
            assertEquals(1, result.updated.size)
            assertTrue(java.io.File(workdir, "a/b/c/deep.txt").exists())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCheckoutConflictDetection() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_checkout_conflict_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val workdir = repo.workdir!!

            java.io.File(workdir, "existing.txt").writeText("local changes")

            val index = Index()
            addBlobToIndex(repo.gitDir, index, "existing.txt", "index content".toByteArray(), false)
            writeIndex(repo.gitDir, index)

            val r1 = checkoutIndex(repo.gitDir, workdir, CheckoutOptions(force = false))
            assertTrue(r1.updated.isEmpty())
            assertEquals(1, r1.conflicts.size)
            assertEquals("local changes", java.io.File(workdir, "existing.txt").readText())

            val r2 = checkoutIndex(repo.gitDir, workdir, CheckoutOptions(force = true))
            assertEquals(1, r2.updated.size)
            assertEquals("index content", java.io.File(workdir, "existing.txt").readText())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCheckoutExecutableMode() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_checkout_exec_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val workdir = repo.workdir!!
            val index = Index()
            addBlobToIndex(repo.gitDir, index, "script.sh", "#!/bin/sh\necho hi\n".toByteArray(), true)
            writeIndex(repo.gitDir, index)

            checkoutIndex(repo.gitDir, workdir, CheckoutOptions(force = true))
            assertTrue(java.io.File(workdir, "script.sh").canExecute(), "file should be executable")
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCheckoutPaths() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_checkout_paths_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val workdir = repo.workdir!!
            val index = Index()
            addBlobToIndex(repo.gitDir, index, "a.txt", "aaa".toByteArray(), false)
            addBlobToIndex(repo.gitDir, index, "b.txt", "bbb".toByteArray(), false)
            addBlobToIndex(repo.gitDir, index, "c.txt", "ccc".toByteArray(), false)
            writeIndex(repo.gitDir, index)

            val result = checkoutPaths(repo.gitDir, workdir, listOf("a.txt", "c.txt"), CheckoutOptions(force = true))
            assertEquals(2, result.updated.size)
            assertTrue(java.io.File(workdir, "a.txt").exists())
            assertFalse(java.io.File(workdir, "b.txt").exists())
            assertTrue(java.io.File(workdir, "c.txt").exists())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCheckoutPathNotInIndex() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_checkout_notfound_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val workdir = repo.workdir!!
            val index = Index()
            writeIndex(repo.gitDir, index)

            assertFailsWith<MuonGitException.NotFound> {
                checkoutPaths(repo.gitDir, workdir, listOf("nonexistent.txt"), CheckoutOptions(force = true))
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Remote Tests

    @Test
    fun testAddAndGetRemote() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_remote_add_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            val remote = addRemote(repo.gitDir, "origin", "https://example.com/repo.git")

            assertEquals("origin", remote.name)
            assertEquals("https://example.com/repo.git", remote.url)
            assertEquals(1, remote.fetchRefspecs.size)
            assertEquals("+refs/heads/*:refs/remotes/origin/*", remote.fetchRefspecs[0])

            val loaded = getRemote(repo.gitDir, "origin")
            assertEquals("https://example.com/repo.git", loaded.url)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testListRemotes() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_remote_list_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            addRemote(repo.gitDir, "origin", "https://example.com/repo.git")
            addRemote(repo.gitDir, "upstream", "https://example.com/upstream.git")

            val names = listRemotes(repo.gitDir)
            assertTrue(names.contains("origin"))
            assertTrue(names.contains("upstream"))
            assertEquals(2, names.size)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRemoveRemote() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_remote_rm_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            addRemote(repo.gitDir, "origin", "https://example.com/repo.git")
            removeRemote(repo.gitDir, "origin")

            assertFailsWith<MuonGitException.NotFound> { getRemote(repo.gitDir, "origin") }
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testRenameRemote() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_remote_rename_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            addRemote(repo.gitDir, "origin", "https://example.com/repo.git")
            renameRemote(repo.gitDir, "origin", "upstream")

            assertFailsWith<MuonGitException.NotFound> { getRemote(repo.gitDir, "origin") }
            val remote = getRemote(repo.gitDir, "upstream")
            assertEquals("https://example.com/repo.git", remote.url)
            assertEquals("+refs/heads/*:refs/remotes/upstream/*", remote.fetchRefspecs[0])
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testAddDuplicateRemote() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_remote_dup_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            addRemote(repo.gitDir, "origin", "https://example.com/repo.git")
            assertFailsWith<MuonGitException.InvalidSpec> {
                addRemote(repo.gitDir, "origin", "https://other.com/repo.git")
            }
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testParseRefspec() {
        val r1 = parseRefspec("+refs/heads/*:refs/remotes/origin/*")!!
        assertTrue(r1.first)
        assertEquals("refs/heads/*", r1.second)
        assertEquals("refs/remotes/origin/*", r1.third)

        val r2 = parseRefspec("refs/heads/main:refs/heads/main")!!
        assertFalse(r2.first)
        assertEquals("refs/heads/main", r2.second)
        assertEquals("refs/heads/main", r2.third)

        assertNull(parseRefspec("no-colon"))
    }

    @Test
    fun testGetNonexistentRemote() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "test_remote_noexist_${System.nanoTime()}")
        try {
            val repo = Repository.init(tmp.absolutePath, bare = false)
            assertFailsWith<MuonGitException.NotFound> { getRemote(repo.gitDir, "nope") }
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Three-Way Merge Tests

    @Test
    fun testMerge3NoChanges() {
        val base = "line1\nline2\nline3"
        val result = merge3(base, base, base)
        assertFalse(result.hasConflicts)
        assertEquals("line1\nline2\nline3\n", result.toCleanString())
    }

    @Test
    fun testMerge3OursOnly() {
        val result = merge3("line1\nline2\nline3", "line1\nmodified\nline3", "line1\nline2\nline3")
        assertFalse(result.hasConflicts)
        assertEquals("line1\nmodified\nline3\n", result.toCleanString())
    }

    @Test
    fun testMerge3TheirsOnly() {
        val result = merge3("line1\nline2\nline3", "line1\nline2\nline3", "line1\nline2\nchanged")
        assertFalse(result.hasConflicts)
        assertEquals("line1\nline2\nchanged\n", result.toCleanString())
    }

    @Test
    fun testMerge3BothDifferentRegions() {
        val result = merge3("line1\nline2\nline3", "changed1\nline2\nline3", "line1\nline2\nchanged3")
        assertFalse(result.hasConflicts)
        assertEquals("changed1\nline2\nchanged3\n", result.toCleanString())
    }

    @Test
    fun testMerge3SameChangeBothSides() {
        val both = "line1\nSAME\nline3"
        val result = merge3("line1\nline2\nline3", both, both)
        assertFalse(result.hasConflicts)
        assertEquals("line1\nSAME\nline3\n", result.toCleanString())
    }

    @Test
    fun testMerge3Conflict() {
        val result = merge3("line1\nline2\nline3", "line1\nours\nline3", "line1\ntheirs\nline3")
        assertTrue(result.hasConflicts)
        assertNull(result.toCleanString())
        val text = result.toStringWithMarkers()
        assertTrue(text.contains("<<<<<<< ours"))
        assertTrue(text.contains("======="))
        assertTrue(text.contains(">>>>>>> theirs"))
    }

    @Test
    fun testMerge3OursAddsLines() {
        val result = merge3("line1\nline3", "line1\nline2\nline3", "line1\nline3")
        assertFalse(result.hasConflicts)
        assertEquals("line1\nline2\nline3\n", result.toCleanString())
    }

    @Test
    fun testMerge3TheirsDeletesLines() {
        val result = merge3("line1\nline2\nline3", "line1\nline2\nline3", "line1\nline3")
        assertFalse(result.hasConflicts)
        assertEquals("line1\nline3\n", result.toCleanString())
    }

    @Test
    fun testMerge3EmptyBase() {
        val result = merge3("", "added", "")
        assertFalse(result.hasConflicts)
        assertEquals("added\n", result.toCleanString())
    }

    // Transport Tests

    @Test
    fun testPktLineEncode() {
        val encoded = pktLineEncode("hello\n".toByteArray(Charsets.US_ASCII))
        assertTrue("000ahello\n".toByteArray(Charsets.US_ASCII).contentEquals(encoded))
    }

    @Test
    fun testPktLineFlushValue() {
        assertTrue("0000".toByteArray(Charsets.US_ASCII).contentEquals(pktLineFlush()))
    }

    @Test
    fun testPktLineDecode() {
        val input = "000ahello\n0000".toByteArray(Charsets.US_ASCII)
        val (lines, consumed) = pktLineDecode(input)
        assertEquals(14, consumed)
        assertEquals(2, lines.size)
        assertEquals(PktLine.Data("hello\n".toByteArray(Charsets.UTF_8)), lines[0])
        assertEquals(PktLine.Flush, lines[1])
    }

    @Test
    fun testPktLineRoundtrip() {
        val data = "test data here".toByteArray(Charsets.UTF_8)
        val encoded = pktLineEncode(data)
        val (lines, _) = pktLineDecode(encoded)
        assertEquals(1, lines.size)
        assertEquals(PktLine.Data(data), lines[0])
    }

    @Test
    fun testParseRefAdvertisement() {
        val oidHex = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val line1 = "$oidHex HEAD\u0000multi_ack thin-pack side-band\n"
        val line2 = "$oidHex refs/heads/main\n"

        var input = pktLineEncode(line1.toByteArray(Charsets.UTF_8))
        input += pktLineEncode(line2.toByteArray(Charsets.UTF_8))
        input += pktLineFlush()

        val (decoded, _) = pktLineDecode(input)
        val (refs, caps) = parseRefAdvertisement(decoded)

        assertEquals(2, refs.size)
        assertEquals("HEAD", refs[0].name)
        assertEquals("refs/heads/main", refs[1].name)
        assertTrue(caps.has("multi_ack"))
        assertTrue(caps.has("thin-pack"))
        assertTrue(caps.has("side-band"))
        assertFalse(caps.has("ofs-delta"))
    }

    @Test
    fun testBuildWantHave() {
        val want = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val have = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        val data = buildWantHave(listOf(want), listOf(have), listOf("multi_ack", "thin-pack"))
        val text = String(data, Charsets.UTF_8)

        assertTrue(text.contains("want aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d multi_ack thin-pack"))
        assertTrue(text.contains("have bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"))
        assertTrue(text.contains("done"))
    }

    @Test
    fun testParseGitUrlHttps() {
        val result = parseGitUrl("https://github.com/user/repo.git")!!
        assertEquals("https", result.first)
        assertEquals("github.com", result.second)
        assertEquals("/user/repo.git", result.third)
    }

    @Test
    fun testParseGitUrlSsh() {
        val result = parseGitUrl("git@github.com:user/repo.git")!!
        assertEquals("ssh", result.first)
        assertEquals("git@github.com", result.second)
        assertEquals("user/repo.git", result.third)
    }

    @Test
    fun testParseGitUrlSshProtocol() {
        val result = parseGitUrl("ssh://git@github.com/user/repo.git")!!
        assertEquals("ssh", result.first)
        assertEquals("git@github.com", result.second)
        assertEquals("/user/repo.git", result.third)
    }

    @Test
    fun testServerCapabilitiesGet() {
        val caps = ServerCapabilities(listOf(
            "multi_ack",
            "agent=git/2.30.0",
            "symref=HEAD:refs/heads/main",
        ))
        assertTrue(caps.has("multi_ack"))
        assertTrue(caps.has("agent"))
        assertEquals("git/2.30.0", caps.get("agent"))
        assertEquals("HEAD:refs/heads/main", caps.get("symref"))
        assertNull(caps.get("multi_ack"))
    }

    // Fetch/Push/Clone Tests

    @Test
    fun testRefspecMatchGlob() {
        assertEquals("main", refspecMatch("refs/heads/main", "refs/heads/*"))
        assertEquals("feature/x", refspecMatch("refs/heads/feature/x", "refs/heads/*"))
        assertNull(refspecMatch("refs/tags/v1", "refs/heads/*"))
    }

    @Test
    fun testRefspecMatchExact() {
        assertEquals("", refspecMatch("refs/heads/main", "refs/heads/main"))
        assertNull(refspecMatch("refs/heads/dev", "refs/heads/main"))
    }

    @Test
    fun testApplyRefspecGlob() {
        assertEquals("refs/remotes/origin/main", applyRefspec("refs/heads/main", "+refs/heads/*:refs/remotes/origin/*"))
        assertEquals("refs/remotes/origin/feature/x", applyRefspec("refs/heads/feature/x", "+refs/heads/*:refs/remotes/origin/*"))
    }

    @Test
    fun testApplyRefspecNoMatch() {
        assertNull(applyRefspec("refs/tags/v1", "+refs/heads/*:refs/remotes/origin/*"))
    }

    @Test
    fun testComputeFetchWants() {
        val tmp = createTempDir("test_fetch_wants")
        try {
            val repo = Repository.init(tmp.absolutePath)
            val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

            val remoteRefs = listOf(
                RemoteRef(oid1, "refs/heads/main"),
                RemoteRef(oid2, "refs/heads/dev"),
                RemoteRef(oid1, "refs/tags/v1"),
            )
            val refspecs = listOf("+refs/heads/*:refs/remotes/origin/*")

            val neg = computeFetchWants(remoteRefs, refspecs, repo.gitDir)
            assertEquals(2, neg.wants.size)
            assertEquals(2, neg.matchedRefs.size)
            assertEquals("refs/remotes/origin/main", neg.matchedRefs[0].localName)
            assertEquals("refs/remotes/origin/dev", neg.matchedRefs[1].localName)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testComputeFetchWantsSkipsExisting() {
        val tmp = createTempDir("test_fetch_skip")
        try {
            val repo = Repository.init(tmp.absolutePath)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            writeReference(repo.gitDir, "refs/remotes/origin/main", oid)

            val remoteRefs = listOf(RemoteRef(oid, "refs/heads/main"))
            val refspecs = listOf("+refs/heads/*:refs/remotes/origin/*")

            val neg = computeFetchWants(remoteRefs, refspecs, repo.gitDir)
            assertEquals(0, neg.wants.size)
            assertEquals(1, neg.matchedRefs.size)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testUpdateRefsFromFetch() {
        val tmp = createTempDir("test_fetch_update")
        try {
            val repo = Repository.init(tmp.absolutePath)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val matched = listOf(MatchedRef("refs/heads/main", "refs/remotes/origin/main", oid))

            val count = updateRefsFromFetch(repo.gitDir, matched)
            assertEquals(1, count)

            val resolved = resolveReference(repo.gitDir, "refs/remotes/origin/main")
            assertEquals(oid, resolved)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testComputePushUpdates() {
        val tmp = createTempDir("test_push_updates")
        try {
            val repo = Repository.init(tmp.absolutePath)
            val localOid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val remoteOid = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

            writeReference(repo.gitDir, "refs/heads/main", localOid)

            val remoteRefs = listOf(RemoteRef(remoteOid, "refs/heads/main"))
            val updates = computePushUpdates(
                listOf("refs/heads/main:refs/heads/main"),
                repo.gitDir,
                remoteRefs
            )

            assertEquals(1, updates.size)
            assertEquals(localOid, updates[0].srcOid)
            assertEquals(remoteOid, updates[0].dstOid)
            assertFalse(updates[0].force)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testBuildPushReportOutput() {
        val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val updates = listOf(PushUpdate("refs/heads/main", "refs/heads/main", oid1, oid2, false))
        val report = buildPushReport(updates)
        assertTrue(report.contains(oid1.hex))
        assertTrue(report.contains(oid2.hex))
        assertTrue(report.contains("refs/heads/main"))
    }

    @Test
    fun testCloneSetupCreatesRepo() {
        val tmp = createTempDir("test_clone_setup")
        try {
            val repo = cloneSetup(tmp.absolutePath, "https://example.com/repo.git")
            val remote = getRemote(repo.gitDir, "origin")
            assertEquals("https://example.com/repo.git", remote.url)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testCloneFinishSetsUpRefs() {
        val tmp = createTempDir("test_clone_finish")
        try {
            val repo = Repository.init(tmp.absolutePath)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            cloneFinish(repo.gitDir, "origin", "main", oid)

            val resolved = resolveReference(repo.gitDir, "refs/heads/main")
            assertEquals(oid, resolved)

            val head = java.io.File(repo.gitDir, "HEAD").readText()
            assertTrue(head.contains("refs/heads/main"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testDefaultBranchFromCapsValue() {
        val caps = ServerCapabilities(listOf("multi_ack", "symref=HEAD:refs/heads/main"))
        assertEquals("main", defaultBranchFromCaps(caps))

        val caps2 = ServerCapabilities(listOf("multi_ack"))
        assertNull(defaultBranchFromCaps(caps2))
    }

    @Test
    fun testCloneSetupWithBranch() {
        val tmp = createTempDir("test_clone_branch")
        try {
            val opts = CloneOptions(branch = "develop")
            val repo = cloneSetup(tmp.absolutePath, "https://example.com/repo.git", opts)
            val head = java.io.File(repo.gitDir, "HEAD").readText()
            assertTrue(head.contains("refs/heads/develop"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Attributes Tests

    @Test
    fun testParseSimpleAttrs() {
        val attrs = Attributes()
        attrs.parse("*.txt text\n*.bin binary\n")

        assertEquals(AttrValue.Set, attrs.get("hello.txt", "text"))
        assertTrue(attrs.isBinary("image.bin"))
        assertFalse(attrs.isBinary("hello.txt"))
    }

    @Test
    fun testParseUnsetAndValue() {
        val attrs = Attributes()
        attrs.parse("*.md text eol=lf\n*.png -text -diff\n")

        assertEquals(AttrValue.Set, attrs.get("README.md", "text"))
        assertEquals(AttrValue.Value("lf"), attrs.get("README.md", "eol"))
        assertEquals("lf", attrs.eol("README.md"))
        assertEquals(AttrValue.Unset, attrs.get("image.png", "text"))
        assertTrue(attrs.isBinary("image.png"))
    }

    @Test
    fun testBinaryMacro() {
        val attrs = Attributes()
        attrs.parse("*.jpg binary\n")

        assertTrue(attrs.isBinary("photo.jpg"))
        assertEquals(AttrValue.Unset, attrs.get("photo.jpg", "diff"))
        assertEquals(AttrValue.Unset, attrs.get("photo.jpg", "merge"))
        assertEquals(AttrValue.Unset, attrs.get("photo.jpg", "text"))
    }

    @Test
    fun testLastMatchWins() {
        val attrs = Attributes()
        attrs.parse("* text\n*.bin -text\n")

        assertEquals(AttrValue.Set, attrs.get("file.txt", "text"))
        assertEquals(AttrValue.Unset, attrs.get("file.bin", "text"))
    }

    @Test
    fun testPathWithDirectory() {
        val attrs = Attributes()
        attrs.parse("src/*.rs text eol=lf\n")

        assertEquals(AttrValue.Set, attrs.get("src/main.rs", "text"))
        assertNull(attrs.get("main.rs", "text"))
    }

    @Test
    fun testGetAllAttrs() {
        val attrs = Attributes()
        attrs.parse("*.rs text eol=lf diff\n")

        val all = attrs.getAll("main.rs")
        assertEquals(3, all.size)
        assertTrue(all.any { it.first == "text" && it.second == AttrValue.Set })
        assertTrue(all.any { it.first == "eol" && it.second == AttrValue.Value("lf") })
        assertTrue(all.any { it.first == "diff" && it.second == AttrValue.Set })
    }

    @Test
    fun testCommentAndEmptyLines() {
        val attrs = Attributes()
        attrs.parse("# comment\n\n*.txt text\n  # another comment\n")

        assertEquals(AttrValue.Set, attrs.get("file.txt", "text"))
        assertEquals(1, attrs.rules.size)
    }

    @Test
    fun testGlobPatterns() {
        val attrs = Attributes()
        attrs.parse("*.txt text\n*.[ch] diff\nMakefile export-ignore\n")

        assertEquals(AttrValue.Set, attrs.get("file.txt", "text"))
        assertEquals(AttrValue.Set, attrs.get("main.c", "diff"))
        assertEquals(AttrValue.Set, attrs.get("util.h", "diff"))
        assertNull(attrs.get("main.rs", "diff"))
        assertEquals(AttrValue.Set, attrs.get("Makefile", "export-ignore"))
    }

    @Test
    fun testLoadAttrsFile() {
        val tmp = createTempDir("test_attrs_load")
        try {
            val path = java.io.File(tmp, ".gitattributes")
            path.writeText("*.txt text\n*.bin binary\n")

            val attrs = Attributes.load(path)
            assertEquals(AttrValue.Set, attrs.get("file.txt", "text"))
            assertTrue(attrs.isBinary("data.bin"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testLoadForRepo() {
        val tmp = createTempDir("test_attrs_repo")
        try {
            val repo = Repository.init(tmp.absolutePath)
            val workdir = repo.workdir!!

            java.io.File(workdir, ".gitattributes").writeText("*.txt text\n")

            val infoDir = java.io.File(repo.gitDir, "info")
            infoDir.mkdirs()
            java.io.File(infoDir, "attributes").writeText("*.bin binary\n")

            val attrs = Attributes.loadForRepo(repo.gitDir, workdir)
            assertEquals(AttrValue.Set, attrs.get("file.txt", "text"))
            assertTrue(attrs.isBinary("data.bin"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Submodule Tests

    @Test
    fun testParseGitmodules() {
        val content = "[submodule \"lib/foo\"]\n\tpath = lib/foo\n\turl = https://github.com/example/foo.git\n[submodule \"lib/bar\"]\n\tpath = lib/bar\n\turl = https://github.com/example/bar.git\n\tbranch = develop\n"
        val subs = parseGitmodules(content)
        assertEquals(2, subs.size)
        assertEquals("lib/foo", subs[0].name)
        assertEquals("lib/foo", subs[0].path)
        assertEquals("https://github.com/example/foo.git", subs[0].url)
        assertNull(subs[0].branch)
        assertEquals("lib/bar", subs[1].name)
        assertEquals("develop", subs[1].branch)
    }

    @Test
    fun testParseGitmodulesWithOptions() {
        val content = "[submodule \"vendor/lib\"]\n\tpath = vendor/lib\n\turl = git@github.com:example/lib.git\n\tshallow = true\n\tupdate = rebase\n\tfetchRecurseSubmodules = false\n"
        val subs = parseGitmodules(content)
        assertEquals(1, subs.size)
        assertTrue(subs[0].shallow)
        assertEquals("rebase", subs[0].update)
        assertEquals(false, subs[0].fetchRecurse)
    }

    @Test
    fun testParseEmptyGitmodules() {
        val subs = parseGitmodules("")
        assertTrue(subs.isEmpty())
    }

    @Test
    fun testLoadSubmodulesNoFile() {
        val tmp = createTempDir("test_submod_nofile")
        try {
            val repo = Repository.init(tmp.absolutePath, false)
            val subs = loadSubmodules(repo.workdir!!)
            assertTrue(subs.isEmpty())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testWriteAndLoadGitmodules() {
        val tmp = createTempDir("test_submod_write")
        try {
            val repo = Repository.init(tmp.absolutePath, false)
            val subs = listOf(
                Submodule("libs/core", "libs/core", "https://example.com/core.git", branch = "main"),
                Submodule("vendor/ext", "vendor/ext", "https://example.com/ext.git", shallow = true, update = "merge", fetchRecurse = true)
            )
            writeGitmodules(repo.workdir!!, subs)
            val loaded = loadSubmodules(repo.workdir!!)

            assertEquals(2, loaded.size)
            assertEquals("libs/core", loaded[0].name)
            assertEquals("https://example.com/core.git", loaded[0].url)
            assertEquals("main", loaded[0].branch)
            assertEquals("vendor/ext", loaded[1].name)
            assertTrue(loaded[1].shallow)
            assertEquals("merge", loaded[1].update)
            assertEquals(true, loaded[1].fetchRecurse)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testGetSubmodule() {
        val tmp = createTempDir("test_submod_get")
        try {
            val repo = Repository.init(tmp.absolutePath, false)
            val subs = listOf(Submodule("mylib", "lib/mylib", "https://example.com/mylib.git"))
            writeGitmodules(repo.workdir!!, subs)

            val sub = getSubmodule(repo.workdir!!, "mylib")
            assertEquals("lib/mylib", sub.path)
            assertEquals("https://example.com/mylib.git", sub.url)

            assertFailsWith<MuonGitException.NotFound> { getSubmodule(repo.workdir!!, "nonexistent") }
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testSubmoduleInit() {
        val tmp = createTempDir("test_submod_init")
        try {
            val repo = Repository.init(tmp.absolutePath, false)
            val subs = listOf(
                Submodule("foo", "foo", "https://example.com/foo.git"),
                Submodule("bar", "bar", "https://example.com/bar.git")
            )
            writeGitmodules(repo.workdir!!, subs)

            val count = submoduleInit(repo.gitDir, repo.workdir!!)
            assertEquals(2, count)

            val config = Config.load(java.io.File(repo.gitDir, "config").absolutePath)
            assertEquals("https://example.com/foo.git", config.get("submodule.foo", "url"))
            assertEquals("https://example.com/bar.git", config.get("submodule.bar", "url"))

            // Re-init should not re-add
            val count2 = submoduleInit(repo.gitDir, repo.workdir!!)
            assertEquals(0, count2)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testSubmoduleInitSelective() {
        val tmp = createTempDir("test_submod_initsel")
        try {
            val repo = Repository.init(tmp.absolutePath, false)
            val subs = listOf(
                Submodule("a", "a", "https://example.com/a.git"),
                Submodule("b", "b", "https://example.com/b.git")
            )
            writeGitmodules(repo.workdir!!, subs)

            val count = submoduleInit(repo.gitDir, repo.workdir!!, listOf("a"))
            assertEquals(1, count)

            val config = Config.load(java.io.File(repo.gitDir, "config").absolutePath)
            assertEquals("https://example.com/a.git", config.get("submodule.a", "url"))
            assertNull(config.get("submodule.b", "url"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Pack Index Tests

    private fun sortedTestOids(): Triple<List<OID>, IntArray, LongArray> {
        val oids = listOf(
            OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"),
            OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"),
            OID("ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"),
        ).sortedBy { it.hex }
        val crcs = intArrayOf(0x12345678, 0x23456789, 0x3456789A.toInt())
        val offsets = longArrayOf(12L, 256L, 1024L)
        return Triple(oids, crcs, offsets)
    }

    @Test
    fun testParsePackIndex() {
        val (oids, crcs, offsets) = sortedTestOids()
        val data = buildPackIndex(oids, crcs, offsets)
        val idx = parsePackIndex(data)

        assertEquals(3, idx.count)
        assertEquals(3, idx.oids.size)
        assertEquals(3, idx.crcs.size)
        assertEquals(3, idx.offsets.size)
    }

    @Test
    fun testPackIndexFind() {
        val (oids, crcs, offsets) = sortedTestOids()
        val data = buildPackIndex(oids, crcs, offsets)
        val idx = parsePackIndex(data)

        assertEquals(offsets[0], idx.find(oids[0]))
        assertEquals(offsets[1], idx.find(oids[1]))
        assertEquals(offsets[2], idx.find(oids[2]))

        val missing = OID("ddf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        assertNull(idx.find(missing))
    }

    @Test
    fun testPackIndexContains() {
        val (oids, crcs, offsets) = sortedTestOids()
        val data = buildPackIndex(oids, crcs, offsets)
        val idx = parsePackIndex(data)

        assertTrue(idx.contains(oids[0]))
        assertTrue(idx.contains(oids[1]))

        val missing = OID("0000000000000000000000000000000000000001")
        assertTrue(!idx.contains(missing))
    }

    @Test
    fun testPackIndexFanout() {
        val (oids, crcs, offsets) = sortedTestOids()
        val data = buildPackIndex(oids, crcs, offsets)
        val idx = parsePackIndex(data)

        assertEquals(0, idx.fanout[0xa9])
        assertEquals(1, idx.fanout[0xaa])
        assertEquals(2, idx.fanout[0xbb])
        assertEquals(3, idx.fanout[0xcc])
        assertEquals(3, idx.fanout[255])
    }

    @Test
    fun testPackIndexEmpty() {
        val data = buildPackIndex(emptyList(), intArrayOf(), longArrayOf())
        val idx = parsePackIndex(data)
        assertEquals(0, idx.count)
        assertTrue(idx.oids.isEmpty())
    }

    @Test
    fun testPackIndexBadMagic() {
        val data = buildPackIndex(emptyList(), intArrayOf(), longArrayOf())
        data[0] = 0x00
        assertFailsWith<MuonGitException.InvalidObject> {
            parsePackIndex(data)
        }
    }

    // Pack Object Tests

    @Test
    fun testApplyDeltaCopy() {
        val base = "hello world".toByteArray()
        val delta = byteArrayOf(11, 11, (0x80 or 0x01 or 0x10).toByte(), 0, 11)
        val result = applyDelta(base, delta)
        assertTrue(result.contentEquals(base))
    }

    @Test
    fun testApplyDeltaInsert() {
        val base = "hello".toByteArray()
        val delta = byteArrayOf(5, 6, 6) + "world!".toByteArray()
        val result = applyDelta(base, delta)
        assertTrue(result.contentEquals("world!".toByteArray()))
    }

    @Test
    fun testApplyDeltaMixed() {
        val base = "hello cruel".toByteArray()
        val delta = byteArrayOf(11, 11, (0x80 or 0x01 or 0x10).toByte(), 0, 5, 6) + " world".toByteArray()
        val result = applyDelta(base, delta)
        assertTrue(result.contentEquals("hello world".toByteArray()))
    }

    @Test
    fun testBuildAndReadPack() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_pack_read")
        tmp.deleteRecursively()
        tmp.mkdirs()
        try {
            val blobData = "hello pack\n".toByteArray()
            val packData = buildTestPack(listOf(Pair(ObjectType.BLOB, blobData)))
            val packFile = java.io.File(tmp, "test.pack")
            packFile.writeBytes(packData)

            val oid = OID.hashObject(ObjectType.BLOB, blobData)
            val idxData = buildPackIndex(listOf(oid), intArrayOf(0), longArrayOf(12))
            val idx = parsePackIndex(idxData)

            val obj = readPackObject(packFile.path, 12, idx)
            assertEquals(ObjectType.BLOB, obj.objType)
            assertTrue(obj.data.contentEquals(blobData))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testBuildAndReadMultipleObjects() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_pack_multi")
        tmp.deleteRecursively()
        tmp.mkdirs()
        try {
            val blob1 = "first blob\n".toByteArray()
            val blob2 = "second blob\n".toByteArray()
            val packData = buildTestPack(listOf(Pair(ObjectType.BLOB, blob1), Pair(ObjectType.BLOB, blob2)))
            val packFile = java.io.File(tmp, "test.pack")
            packFile.writeBytes(packData)

            val oid1 = OID.hashObject(ObjectType.BLOB, blob1)
            val idxData = buildPackIndex(listOf(oid1), intArrayOf(0), longArrayOf(12))
            val idx = parsePackIndex(idxData)

            val obj1 = readPackObject(packFile.path, 12, idx)
            assertEquals(ObjectType.BLOB, obj1.objType)
            assertTrue(obj1.data.contentEquals(blob1))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testReadPackCommit() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_pack_commit")
        tmp.deleteRecursively()
        tmp.mkdirs()
        try {
            val commitData = "tree 0000000000000000000000000000000000000000\nauthor Test <t@t> 0 +0000\ncommitter Test <t@t> 0 +0000\n\ntest\n".toByteArray()
            val packData = buildTestPack(listOf(Pair(ObjectType.COMMIT, commitData)))
            val packFile = java.io.File(tmp, "test.pack")
            packFile.writeBytes(packData)

            val oid = OID.hashObject(ObjectType.COMMIT, commitData)
            val idxData = buildPackIndex(listOf(oid), intArrayOf(0), longArrayOf(12))
            val idx = parsePackIndex(idxData)

            val obj = readPackObject(packFile.path, 12, idx)
            assertEquals(ObjectType.COMMIT, obj.objType)
            assertTrue(obj.data.contentEquals(commitData))
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Conformance Tests

    @Test
    fun testConformanceSHA1Vectors() {
        // Vector 1: empty string
        val d1 = SHA1.hash(byteArrayOf())
        assertEquals("da39a3ee5e6b4b0d3255bfef95601890afd80709", d1.joinToString("") { "%02x".format(it) })

        // Vector 2: "hello"
        val d2 = SHA1.hash("hello")
        assertEquals("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d", d2.joinToString("") { "%02x".format(it) })

        // Vector 3: longer string
        val d3 = SHA1.hash("The quick brown fox jumps over the lazy dog")
        assertEquals("2fd4e1c67a2d28fced849ee1bb76e7391b93eb12", d3.joinToString("") { "%02x".format(it) })

        // Vector 4: with newline
        val d4 = SHA1.hash("hello world\n")
        assertEquals("22596363b3de40b06f981fb85d82312e8c0ed511", d4.joinToString("") { "%02x".format(it) })
    }

    @Test
    fun testConformanceBlobOID() {
        val oid1 = OID.hashObject(ObjectType.BLOB, "hello\n".toByteArray())
        assertEquals("ce013625030ba8dba906f756967f9e9ca394464a", oid1.hex)

        val oid2 = OID.hashObject(ObjectType.BLOB, byteArrayOf())
        assertEquals("e69de29bb2d1d6434b8b29ae775ad8c2e48c5391", oid2.hex)

        val oid3 = OID.hashObject(ObjectType.BLOB, "test content\n".toByteArray())
        assertEquals("d670460b4b4aece5915caf5c68d12f560a9fe3e4", oid3.hex)
    }

    @Test
    fun testConformanceCommitOID() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val author = Signature(name = "Conf Author", email = "author@conf.test", time = 1700000000L, offset = 0)
        val committer = Signature(name = "Conf Committer", email = "committer@conf.test", time = 1700000000L, offset = 0)

        val data = serializeCommit(treeId, emptyList(), author, committer, "conformance test commit\n")
        val oid = OID.hashObject(ObjectType.COMMIT, data)

        val parsed = parseCommit(oid, data)
        assertEquals(treeId, parsed.treeId)
        assertEquals("Conf Author", parsed.author.name)
        assertEquals("committer@conf.test", parsed.committer.email)
        assertEquals("conformance test commit\n", parsed.message)

        assertTrue(!oid.isZero)
        assertEquals(40, oid.hex.length)
    }

    @Test
    fun testConformanceTreeOID() {
        val blobOid = OID("ce013625030ba8dba906f756967f9e9ca394464a")
        val entries = listOf(TreeEntry(mode = FileMode.BLOB, name = "hello.txt", oid = blobOid))
        val data = serializeTree(entries)
        val oid = OID.hashObject(ObjectType.TREE, data)

        val parsed = parseTree(oid, data)
        assertEquals(1, parsed.entries.size)
        assertEquals("hello.txt", parsed.entries[0].name)
        assertEquals(blobOid, parsed.entries[0].oid)

        assertTrue(!oid.isZero)
        assertEquals(40, oid.hex.length)
    }

    @Test
    fun testConformanceTagOID() {
        val targetId = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val tagger = Signature(name = "Conf Tagger", email = "tagger@conf.test", time = 1700000000L, offset = 0)

        val data = serializeTag(targetId, ObjectType.COMMIT, "v1.0-conf", tagger, "conformance tag\n")
        val oid = OID.hashObject(ObjectType.TAG, data)

        val parsed = parseTag(oid, data)
        assertEquals(targetId, parsed.targetId)
        assertEquals("v1.0-conf", parsed.tagName)
        assertEquals("Conf Tagger", parsed.tagger?.name)

        assertTrue(!oid.isZero)
    }

    @Test
    fun testConformanceSHA256Vectors() {
        // Vector 1: empty string
        val d1 = SHA256Hash.hash(byteArrayOf())
        assertEquals("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", d1.joinToString("") { "%02x".format(it) })

        // Vector 2: "hello"
        val d2 = SHA256Hash.hash("hello")
        assertEquals("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824", d2.joinToString("") { "%02x".format(it) })

        // Vector 3: longer string
        val d3 = SHA256Hash.hash("The quick brown fox jumps over the lazy dog")
        assertEquals("d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592", d3.joinToString("") { "%02x".format(it) })
    }

    @Test
    fun testConformanceSHA256BlobOID() {
        val oid1 = OID.hashObjectSHA256(ObjectType.BLOB, "hello\n".toByteArray())
        assertEquals(64, oid1.hex.length)
        assertTrue(!oid1.isZero)

        val oid2 = OID.hashObjectSHA256(ObjectType.BLOB, byteArrayOf())
        assertEquals(64, oid2.hex.length)

        // SHA-256 and SHA-1 should produce different OIDs
        val oidSha1 = OID.hashObject(ObjectType.BLOB, "hello\n".toByteArray())
        assertTrue(oid1.hex != oidSha1.hex)
    }

    @Test
    fun testConformanceHashAlgorithm() {
        assertEquals(20, HashAlgorithm.SHA1.digestLength)
        assertEquals(32, HashAlgorithm.SHA256.digestLength)
        assertEquals(40, HashAlgorithm.SHA1.hexLength)
        assertEquals(64, HashAlgorithm.SHA256.hexLength)
    }

    @Test
    fun testConformanceSignatureFormat() {
        // Positive offset
        val sig1 = Signature(name = "Test User", email = "test@example.com", time = 1234567890L, offset = 330)
        assertEquals("Test User <test@example.com> 1234567890 +0530", formatSignatureLine(sig1))

        // Negative offset
        val sig2 = Signature(name = "Test", email = "test@test.com", time = 1000L, offset = -480)
        assertEquals("Test <test@test.com> 1000 -0800", formatSignatureLine(sig2))

        // Zero offset
        val sig3 = Signature(name = "Zero", email = "zero@test.com", time = 0L, offset = 0)
        assertEquals("Zero <zero@test.com> 0 +0000", formatSignatureLine(sig3))
    }

    @Test
    fun testConformanceDeltaApply() {
        // Copy entire base
        val base1 = "hello world".toByteArray()
        val delta1 = byteArrayOf(11, 11, (0x80 or 0x01 or 0x10).toByte(), 0, 11)
        val result1 = applyDelta(base1, delta1)
        assertEquals("hello world", String(result1))

        // Insert only
        val base2 = "hello".toByteArray()
        val delta2 = byteArrayOf(5, 6, 6) + "world!".toByteArray()
        val result2 = applyDelta(base2, delta2)
        assertEquals("world!", String(result2))

        // Copy + insert
        val base3 = "hello cruel".toByteArray()
        val delta3 = byteArrayOf(11, 11, (0x80 or 0x01 or 0x10).toByte(), 0, 5, 6) + " world".toByteArray()
        val result3 = applyDelta(base3, delta3)
        assertEquals("hello world", String(result3))
    }

    // SHA-256 Tests

    @Test
    fun testSHA256Empty() {
        val digest = SHA256Hash.hash(byteArrayOf())
        assertEquals("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", digest.joinToString("") { "%02x".format(it) })
    }

    @Test
    fun testSHA256Hello() {
        val digest = SHA256Hash.hash("hello")
        assertEquals("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824", digest.joinToString("") { "%02x".format(it) })
    }

    @Test
    fun testSHA256GitBlob() {
        val data = "hello\n".toByteArray()
        val oid = OID.hashObjectSHA256(ObjectType.BLOB, data)
        assertEquals(64, oid.hex.length)
        assertTrue(!oid.isZero)
    }

    @Test
    fun testSHA256Longer() {
        val digest = SHA256Hash.hash("The quick brown fox jumps over the lazy dog")
        assertEquals("d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592", digest.joinToString("") { "%02x".format(it) })
    }

    @Test
    fun testHashAlgorithm() {
        assertEquals(20, HashAlgorithm.SHA1.digestLength)
        assertEquals(32, HashAlgorithm.SHA256.digestLength)
        assertEquals(40, HashAlgorithm.SHA1.hexLength)
        assertEquals(64, HashAlgorithm.SHA256.hexLength)
    }

    @Test
    fun testZeroSHA256() {
        val z = OID.ZERO_SHA256
        assertTrue(z.isZero)
        assertEquals(64, z.hex.length)
    }

    @Test
    fun testConformanceIndexRoundTrip() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_test_conf_index")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("ce013625030ba8dba906f756967f9e9ca394464a")

            val index = Index()
            index.add(IndexEntry(mode = 33188, fileSize = 6, oid = oid, path = "hello.txt"))
            index.add(IndexEntry(mode = 33261, fileSize = 100, oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"), path = "script.sh"))
            writeIndex(repo.gitDir, index)

            val loaded = readIndex(repo.gitDir)
            assertEquals(2, loaded.entries.size)
            assertEquals("hello.txt", loaded.entries[0].path)
            assertEquals("script.sh", loaded.entries[1].path)
            assertEquals(33188, loaded.entries[0].mode) // 0o100644
            assertEquals(33261, loaded.entries[1].mode) // 0o100755
        } finally {
            tmp.deleteRecursively()
        }
    }

    // ============================================================
    // Parity tests (libgit2 test suite)
    // ============================================================

    // OID parity (libgit2 core/oid.c)

    @Test
    fun testParityOIDFromValidHex() {
        val hex = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        val oid = OID(hex)
        assertEquals(hex, oid.hex)
    }

    @Test
    fun testParityOIDZeroIsZero() {
        val oid = OID.ZERO
        assertTrue(oid.isZero)
    }

    @Test
    fun testParityOIDNonzeroIsNotZero() {
        val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        assertTrue(!oid.isZero)
    }

    @Test
    fun testParityOIDEquality() {
        val a = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val b = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val c = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        assertEquals(a, b)
        assertTrue(a != c)
    }

    @Test
    fun testParityOIDSHA256Roundtrip() {
        val data = "hello sha256".toByteArray()
        val oid = OID.hashObjectSHA256(ObjectType.BLOB, data)
        assertEquals(64, oid.hex.length)
        assertEquals(32, oid.raw.size)
    }

    @Test
    fun testParityOIDSHA1vsSHA256Different() {
        val data = "test data".toByteArray()
        val sha1 = OID.hashObject(ObjectType.BLOB, data)
        val sha256 = OID.hashObjectSHA256(ObjectType.BLOB, data)
        assertTrue(sha1.hex != sha256.hex)
        assertEquals(40, sha1.hex.length)
        assertEquals(64, sha256.hex.length)
    }

    @Test
    fun testParityHashAlgorithmProperties() {
        assertEquals(20, HashAlgorithm.SHA1.digestLength)
        assertEquals(40, HashAlgorithm.SHA1.hexLength)
        assertEquals(32, HashAlgorithm.SHA256.digestLength)
        assertEquals(64, HashAlgorithm.SHA256.hexLength)
    }

    // Signature parity (libgit2 commit/signature.c)

    @Test
    fun testParitySignaturePositiveOffset() {
        val sig = Signature(name = "Test", email = "t@t", time = 1000000, offset = 330)
        val line = formatSignatureLine(sig)
        assertTrue(line.contains("+0530"))
    }

    @Test
    fun testParitySignatureNegativeOffset() {
        val sig = Signature(name = "Test", email = "t@t", time = 1000000, offset = -480)
        val line = formatSignatureLine(sig)
        assertTrue(line.contains("-0800"))
    }

    @Test
    fun testParitySignatureZeroOffset() {
        val sig = Signature(name = "Test", email = "t@t", time = 1000000, offset = 0)
        val line = formatSignatureLine(sig)
        assertTrue(line.contains("+0000"))
    }

    @Test
    fun testParitySignatureLargeOffset() {
        val sig = Signature(name = "Test", email = "t@t", time = 1000000, offset = 765)
        val line = formatSignatureLine(sig)
        assertTrue(line.contains("+1245"))
    }

    @Test
    fun testParitySignatureSingleCharName() {
        val sig = Signature(name = "X", email = "x@x", time = 0, offset = 0)
        val line = formatSignatureLine(sig)
        assertTrue(line.startsWith("X <x@x>"))
    }

    // Commit parity (libgit2 object/validate.c)

    @Test
    fun testParityCommitNoParents() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val sig = Signature(name = "A", email = "a@a", time = 1000000, offset = 0)
        val data = serializeCommit(treeId = treeId, parentIds = emptyList(), author = sig, committer = sig, message = "init\n")
        val oid = OID.hashObject(ObjectType.COMMIT, data)
        val parsed = parseCommit(oid, data)
        assertEquals(treeId, parsed.treeId)
        assertTrue(parsed.parentIds.isEmpty())
        assertEquals("A", parsed.author.name)
        assertEquals("init\n", parsed.message)
    }

    @Test
    fun testParityCommitMultipleParents() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val p1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val p2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val sig = Signature(name = "M", email = "m@m", time = 1000000, offset = 0)
        val data = serializeCommit(treeId = treeId, parentIds = listOf(p1, p2), author = sig, committer = sig, message = "merge\n")
        val oid = OID.hashObject(ObjectType.COMMIT, data)
        val parsed = parseCommit(oid, data)
        assertEquals(2, parsed.parentIds.size)
        assertEquals(p1, parsed.parentIds[0])
        assertEquals(p2, parsed.parentIds[1])
    }

    @Test
    fun testParityCommitWithEncoding() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val sig = Signature(name = "E", email = "e@e", time = 1000000, offset = 0)
        val data = serializeCommit(treeId = treeId, parentIds = emptyList(), author = sig, committer = sig, message = "enc\n", messageEncoding = "ISO-8859-1")
        val oid = OID.hashObject(ObjectType.COMMIT, data)
        val parsed = parseCommit(oid, data)
        assertEquals("ISO-8859-1", parsed.messageEncoding)
    }

    @Test
    fun testParityCommitRoundtripPreservesOID() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val parent = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val sig = Signature(name = "R", email = "r@r", time = 1000000, offset = 60)
        val data1 = serializeCommit(treeId = treeId, parentIds = listOf(parent), author = sig, committer = sig, message = "roundtrip\n")
        val oid1 = OID.hashObject(ObjectType.COMMIT, data1)
        val parsed = parseCommit(oid1, data1)
        val data2 = serializeCommit(treeId = parsed.treeId, parentIds = parsed.parentIds, author = parsed.author, committer = parsed.committer, message = parsed.message, messageEncoding = parsed.messageEncoding)
        val oid2 = OID.hashObject(ObjectType.COMMIT, data2)
        assertEquals(oid1, oid2)
    }

    // Tree parity (libgit2 object/tree/parse.c)

    @Test
    fun testParityTreeEmpty() {
        val data = serializeTree(emptyList())
        assertTrue(data.isEmpty())
        val oid = OID.hashObject(ObjectType.TREE, data)
        val parsed = parseTree(oid, data)
        assertTrue(parsed.entries.isEmpty())
        val oid2 = OID.hashObject(ObjectType.TREE, ByteArray(0))
        assertEquals(oid, oid2)
    }

    @Test
    fun testParityTreeSingleBlob() {
        val blobOid = OID("ae90f12eea699729ed24555e40b9fd669da12a12")
        val entries = listOf(TreeEntry(mode = FileMode.BLOB, name = "foo", oid = blobOid))
        val data = serializeTree(entries)
        val parsed = parseTree(OID.ZERO, data)
        assertEquals(1, parsed.entries.size)
        assertEquals("foo", parsed.entries[0].name)
        assertEquals(FileMode.BLOB, parsed.entries[0].mode)
        assertEquals(blobOid, parsed.entries[0].oid)
    }

    @Test
    fun testParityTreeSingleSubtree() {
        val treeOid = OID("ae90f12eea699729ed24555e40b9fd669da12a12")
        val entries = listOf(TreeEntry(mode = FileMode.TREE, name = "subdir", oid = treeOid))
        val data = serializeTree(entries)
        val parsed = parseTree(OID.ZERO, data)
        assertEquals(1, parsed.entries.size)
        assertTrue(parsed.entries[0].isTree)
        assertTrue(!parsed.entries[0].isBlob)
    }

    @Test
    fun testParityTreeMultipleModes() {
        val oid1 = OID("ae90f12eea699729ed24555e40b9fd669da12a12")
        val oid2 = OID("e8bfe5af39579a7e4898bb23f3a76a72c368cee6")
        val entries = listOf(
            TreeEntry(mode = FileMode.BLOB, name = "file.txt", oid = oid1),
            TreeEntry(mode = FileMode.BLOB_EXE, name = "run.sh", oid = oid2),
            TreeEntry(mode = FileMode.LINK, name = "sym", oid = oid1),
            TreeEntry(mode = FileMode.TREE, name = "dir", oid = oid2),
        )
        val data = serializeTree(entries)
        val parsed = parseTree(OID.ZERO, data)
        assertEquals(4, parsed.entries.size)
        assertEquals("dir", parsed.entries[0].name)
        assertEquals(FileMode.TREE, parsed.entries[0].mode)
        assertEquals("file.txt", parsed.entries[1].name)
        assertEquals("run.sh", parsed.entries[2].name)
        assertEquals("sym", parsed.entries[3].name)
    }

    @Test
    fun testParityTreeRoundtripPreservesOID() {
        val oid = OID("ce013625030ba8dba906f756967f9e9ca394464a")
        val entries = listOf(
            TreeEntry(mode = FileMode.BLOB, name = "hello.txt", oid = oid),
            TreeEntry(mode = FileMode.BLOB_EXE, name = "script.sh", oid = oid),
        )
        val data1 = serializeTree(entries)
        val treeOid1 = OID.hashObject(ObjectType.TREE, data1)
        val parsed = parseTree(treeOid1, data1)
        val data2 = serializeTree(parsed.entries)
        val treeOid2 = OID.hashObject(ObjectType.TREE, data2)
        assertEquals(treeOid1, treeOid2)
    }

    // Tag parity

    @Test
    fun testParityTagTargetingDifferentTypes() {
        val target = OID("ae90f12eea699729ed24555e40b9fd669da12a12")
        val tagger = Signature(name = "T", email = "t@t", time = 0, offset = 0)
        for (objType in listOf(ObjectType.COMMIT, ObjectType.TREE, ObjectType.BLOB)) {
            val data = serializeTag(targetId = target, targetType = objType, tagName = "v1.0", tagger = tagger, message = "tag msg\n")
            val oid = OID.hashObject(ObjectType.TAG, data)
            val parsed = parseTag(oid, data)
            assertEquals(objType, parsed.targetType)
            assertEquals("v1.0", parsed.tagName)
        }
    }

    @Test
    fun testParityTagWithoutTagger() {
        val target = OID("ae90f12eea699729ed24555e40b9fd669da12a12")
        val data = serializeTag(targetId = target, targetType = ObjectType.COMMIT, tagName = "lightweight", tagger = null, message = "no tagger\n")
        val oid = OID.hashObject(ObjectType.TAG, data)
        val parsed = parseTag(oid, data)
        assertNull(parsed.tagger)
        assertEquals("lightweight", parsed.tagName)
    }

    // Config parity (libgit2 config/read.c)

    @Test
    fun testParityConfigBooleanValues() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_parity_cfg_bool")
        tmp.mkdirs()
        try {
            val cfgFile = java.io.File(tmp, "config")
            cfgFile.writeText("[core]\n\tfilemode = true\n\tbare = false\n\tyes = yes\n\tno = no\n\ton = on\n\toff = off\n\tone = 1\n\tzero = 0\n")
            val cfg = Config.load(cfgFile.path)
            assertEquals(true, cfg.getBool("core", "filemode"))
            assertEquals(false, cfg.getBool("core", "bare"))
            assertEquals(true, cfg.getBool("core", "yes"))
            assertEquals(false, cfg.getBool("core", "no"))
            assertEquals(true, cfg.getBool("core", "on"))
            assertEquals(false, cfg.getBool("core", "off"))
            assertEquals(true, cfg.getBool("core", "one"))
            assertEquals(false, cfg.getBool("core", "zero"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testParityConfigIntSuffixes() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_parity_cfg_int")
        tmp.mkdirs()
        try {
            val cfgFile = java.io.File(tmp, "config")
            cfgFile.writeText("[core]\n\tplain = 42\n\tkilo = 2k\n\tmega = 3m\n\tgiga = 1g\n")
            val cfg = Config.load(cfgFile.path)
            assertEquals(42, cfg.getInt("core", "plain"))
            assertEquals(2048, cfg.getInt("core", "kilo"))
            assertEquals(3145728, cfg.getInt("core", "mega"))
            assertEquals(1073741824, cfg.getInt("core", "giga"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testParityConfigCaseInsensitive() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_parity_cfg_case")
        tmp.mkdirs()
        try {
            val cfgFile = java.io.File(tmp, "config")
            cfgFile.writeText("[Core]\n\tFileMode = true\n")
            val cfg = Config.load(cfgFile.path)
            assertEquals(true, cfg.getBool("core", "filemode"))
            assertEquals(true, cfg.getBool("CORE", "FILEMODE"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testParityConfigComments() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_parity_cfg_comments")
        tmp.mkdirs()
        try {
            val cfgFile = java.io.File(tmp, "config")
            cfgFile.writeText("# comment\n[core]\n; another comment\n\tfilemode = true\n")
            val cfg = Config.load(cfgFile.path)
            assertEquals(true, cfg.getBool("core", "filemode"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Blob OID parity

    @Test
    fun testParityBlobEmptyOID() {
        val oid = OID.hashObject(ObjectType.BLOB, ByteArray(0))
        assertEquals("e69de29bb2d1d6434b8b29ae775ad8c2e48c5391", oid.hex)
    }

    @Test
    fun testParityBlobKnownContent() {
        val oid = OID.hashObject(ObjectType.BLOB, "hello\n".toByteArray())
        assertEquals("ce013625030ba8dba906f756967f9e9ca394464a", oid.hex)
    }

    @Test
    fun testParityBlobNewlineOnly() {
        val oid = OID.hashObject(ObjectType.BLOB, "\n".toByteArray())
        assertEquals("8b137891791fe96927ad78e64b0aad7bded08bdc", oid.hex)
    }

    // Index parity

    @Test
    fun testParityIndexSortedByPath() {
        val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val index = Index()
        index.add(IndexEntry(mode = FileMode.BLOB, fileSize = 0, oid = oid, path = "z.txt"))
        index.add(IndexEntry(mode = FileMode.BLOB, fileSize = 0, oid = oid, path = "a.txt"))
        index.add(IndexEntry(mode = FileMode.BLOB, fileSize = 0, oid = oid, path = "m.txt"))
        assertEquals("a.txt", index.entries[0].path)
        assertEquals("m.txt", index.entries[1].path)
        assertEquals("z.txt", index.entries[2].path)
    }

    @Test
    fun testParityIndexDuplicatePathReplaces() {
        val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val index = Index()
        index.add(IndexEntry(mode = FileMode.BLOB, fileSize = 10, oid = oid1, path = "file.txt"))
        index.add(IndexEntry(mode = FileMode.BLOB, fileSize = 20, oid = oid2, path = "file.txt"))
        assertEquals(1, index.entries.size)
        assertEquals(oid2, index.entries[0].oid)
    }

    @Test
    fun testParityIndexManyEntriesRoundtrip() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_parity_idx_many")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val index = Index()
            for (i in 0 until 100) {
                index.add(IndexEntry(mode = FileMode.BLOB, fileSize = i, oid = oid, path = "src/file_%04d.txt".format(i)))
            }
            writeIndex(repo.gitDir, index)
            val loaded = readIndex(repo.gitDir)
            assertEquals(100, loaded.entries.size)
            assertEquals("src/file_0000.txt", loaded.entries[0].path)
            assertEquals("src/file_0099.txt", loaded.entries[99].path)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // Diff parity

    @Test
    fun testParityDiffSortedOutput() {
        val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val old = listOf(
            TreeEntry(mode = FileMode.BLOB, name = "a.txt", oid = oid1),
            TreeEntry(mode = FileMode.BLOB, name = "c.txt", oid = oid1),
            TreeEntry(mode = FileMode.BLOB, name = "e.txt", oid = oid1),
        )
        val new = listOf(
            TreeEntry(mode = FileMode.BLOB, name = "b.txt", oid = oid2),
            TreeEntry(mode = FileMode.BLOB, name = "c.txt", oid = oid2),
            TreeEntry(mode = FileMode.BLOB, name = "d.txt", oid = oid2),
        )
        val deltas = diffTrees(old, new)
        val paths = deltas.map { it.path }
        assertEquals(listOf("a.txt", "b.txt", "c.txt", "d.txt", "e.txt"), paths)
        assertEquals(DiffStatus.DELETED, deltas[0].status)
        assertEquals(DiffStatus.ADDED, deltas[1].status)
        assertEquals(DiffStatus.MODIFIED, deltas[2].status)
        assertEquals(DiffStatus.ADDED, deltas[3].status)
        assertEquals(DiffStatus.DELETED, deltas[4].status)
    }

    @Test
    fun testParityDiffModeChangeIsModified() {
        val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val old = listOf(TreeEntry(mode = FileMode.BLOB, name = "f", oid = oid))
        val new = listOf(TreeEntry(mode = FileMode.BLOB_EXE, name = "f", oid = oid))
        val deltas = diffTrees(old, new)
        assertEquals(1, deltas.size)
        assertEquals(DiffStatus.MODIFIED, deltas[0].status)
    }

    // Delta parity

    @Test
    fun testParityDeltaEmptyInsert() {
        val base = "base".toByteArray()
        val delta = byteArrayOf(4, 3, 3, 'a'.code.toByte(), 'b'.code.toByte(), 'c'.code.toByte())
        val result = applyDelta(base, delta)
        assertEquals("abc", result.decodeToString())
    }

    @Test
    fun testParityDeltaInvalidOpcodeZero() {
        val base = "base".toByteArray()
        val delta = byteArrayOf(4, 4, 0)
        assertFailsWith<Exception> { applyDelta(base, delta) }
    }

    // SHA NIST vectors

    @Test
    fun testParitySHA1NISTVectors() {
        val digest = SHA1.hash("abc")
        val hex = digest.joinToString("") { "%02x".format(it) }
        assertEquals("a9993e364706816aba3e25717850c26c9cd0d89d", hex)
    }

    @Test
    fun testParitySHA256NISTVectors() {
        val digest = SHA256Hash.hash("abc")
        val hex = digest.joinToString("") { "%02x".format(it) }
        assertEquals("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", hex)
    }

    // Repository parity

    @Test
    fun testParityRepoInitCreatesStructure() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_parity_repo_init")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            assertTrue(repo.gitDir.exists())
            assertTrue(java.io.File(repo.gitDir, "objects").isDirectory)
            assertTrue(java.io.File(repo.gitDir, "refs").isDirectory)
            assertTrue(java.io.File(repo.gitDir, "HEAD").exists())
            val head = java.io.File(repo.gitDir, "HEAD").readText().trim()
            assertTrue(head.startsWith("ref:"))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testParityRepoInitBare() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_parity_repo_bare")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path, bare = true)
            assertTrue(repo.isBare)
            assertNull(repo.workdir)
            assertTrue(java.io.File(repo.gitDir, "objects").isDirectory)
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testParityRepoReinitPreserves() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_parity_repo_reinit")
        tmp.deleteRecursively()
        try {
            val repo1 = Repository.init(tmp.path)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val index = Index()
            index.add(IndexEntry(mode = FileMode.BLOB, fileSize = 5, oid = oid, path = "test.txt"))
            writeIndex(repo1.gitDir, index)
            val repo2 = Repository.init(tmp.path)
            val loaded = readIndex(repo2.gitDir)
            assertEquals(1, loaded.entries.size)
            assertEquals("test.txt", loaded.entries[0].path)
        } finally {
            tmp.deleteRecursively()
        }
    }

    // ============================================================
    // Performance tests
    // ============================================================

    private fun measureMs(block: () -> Unit): Double {
        val start = System.nanoTime()
        block()
        return (System.nanoTime() - start) / 1_000_000.0
    }

    @Test
    fun testPerfSHA1Throughput1MB() {
        val data = ByteArray(1_000_000) { 0xAB.toByte() }
        val ms = measureMs { SHA1.hash(data) }
        println("[perf] SHA-1 1MB: ${"%.2f".format(ms)}ms")
        assertTrue(ms < 500.0, "SHA-1 1MB took ${ms}ms, expected < 120000ms")
    }

    @Test
    fun testPerfSHA256Throughput1MB() {
        val data = ByteArray(1_000_000) { 0xAB.toByte() }
        val ms = measureMs { SHA256Hash.hash(data) }
        println("[perf] SHA-256 1MB: ${"%.2f".format(ms)}ms")
        assertTrue(ms < 500.0, "SHA-256 1MB took ${ms}ms, expected < 120000ms")
    }

    @Test
    fun testPerfOIDCreation10K() {
        val ms = measureMs {
            for (i in 0 until 10_000) {
                OID.hashObject(ObjectType.BLOB, "blob content $i".toByteArray())
            }
        }
        println("[perf] OID creation 10K: ${"%.2f".format(ms)}ms")
        assertTrue(ms < 5000.0, "OID creation 10K took ${ms}ms, expected < 5000ms")
    }

    @Test
    fun testPerfTreeSerialize1KEntries() {
        val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val entries = (0 until 1000).map { TreeEntry(mode = FileMode.BLOB, name = "file_%04d.txt".format(it), oid = oid) }
        val ms = measureMs {
            val data = serializeTree(entries)
            parseTree(OID.ZERO, data)
        }
        println("[perf] Tree serialize+parse 1K: ${"%.2f".format(ms)}ms")
        assertTrue(ms < 2000.0, "Tree 1K took ${ms}ms, expected < 2000ms")
    }

    @Test
    fun testPerfCommitSerialize10K() {
        val treeId = OID("4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        val sig = Signature(name = "Perf Test", email = "perf@test", time = 1000000, offset = 0)
        val ms = measureMs {
            for (i in 0 until 10_000) {
                serializeCommit(treeId = treeId, parentIds = emptyList(), author = sig, committer = sig, message = "commit $i\n")
            }
        }
        println("[perf] Commit serialize 10K: ${"%.2f".format(ms)}ms")
        assertTrue(ms < 5000.0, "Commit serialize 10K took ${ms}ms, expected < 5000ms")
    }

    @Test
    fun testPerfIndexReadWrite1K() {
        val tmp = java.io.File(System.getProperty("java.io.tmpdir"), "muongit_kotlin_perf_index")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val oid = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            val index = Index()
            for (i in 0 until 1000) {
                index.add(IndexEntry(mode = FileMode.BLOB, fileSize = i, oid = oid, path = "src/file_%04d.txt".format(i)))
            }
            val ms = measureMs {
                writeIndex(repo.gitDir, index)
                readIndex(repo.gitDir)
            }
            println("[perf] Index write+read 1K: ${"%.2f".format(ms)}ms")
            assertTrue(ms < 5000.0, "Index 1K took ${ms}ms, expected < 5000ms")
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testPerfDiffLargeTrees() {
        val oid1 = OID("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val oid2 = OID("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        val old = (0 until 1000).map { TreeEntry(mode = FileMode.BLOB, name = "file_%04d.txt".format(it), oid = oid1) }
        val new = (0 until 1000).map { i ->
            val oid = if (i % 10 == 0) oid2 else oid1
            TreeEntry(mode = FileMode.BLOB, name = "file_%04d.txt".format(i), oid = oid)
        }
        val ms = measureMs {
            val deltas = diffTrees(old, new)
            assertEquals(100, deltas.size)
        }
        println("[perf] Diff 1K-entry trees: ${"%.2f".format(ms)}ms")
        assertTrue(ms < 2000.0, "Diff 1K took ${ms}ms, expected < 2000ms")
    }

    @Test
    fun testPerfBlobHashing10K() {
        val ms = measureMs {
            for (i in 0 until 10_000) {
                OID.hashObject(ObjectType.BLOB, "line $i\nmore content here\n".toByteArray())
            }
        }
        println("[perf] Blob hashing 10K: ${"%.2f".format(ms)}ms")
        assertTrue(ms < 5000.0, "Blob hashing 10K took ${ms}ms, expected < 5000ms")
    }

    @Test
    fun testPerfSHA1vsSHA256Comparison() {
        val data = ByteArray(1_000_000) { 0xAB.toByte() }
        val msSha1 = measureMs { SHA1.hash(data) }
        val msSha256 = measureMs { SHA256Hash.hash(data) }
        println("[perf] SHA-1 1MB: ${"%.2f".format(msSha1)}ms, SHA-256 1MB: ${"%.2f".format(msSha256)}ms, ratio: ${"%.2f".format(msSha256 / msSha1.coerceAtLeast(0.001))}x")
        assertTrue(msSha1 < 500.0)
        assertTrue(msSha256 < 500.0)
    }
}
