package ai.muonium.muongit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.test.assertFailsWith

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
}
