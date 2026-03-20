package ai.muonium.muongit

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ObjectTest {
    @Test
    fun testReadLooseObjectAndConvertToBlob() {
        val tmp = testDirectory("kotlin_object_loose_lookup")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val blobData = "object api loose blob\n".toByteArray()
            val blobOid = writeBlob(repo.gitDir, blobData)

            val obj = repo.readObject(blobOid)
            assertEquals(blobOid, obj.oid)
            assertEquals(ObjectType.BLOB, obj.objectType)
            assertEquals(blobData.size, obj.size)

            val blob = obj.asBlob()
            assertTrue(blobData.contentEquals(blob.data))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testReadPackedObjectByOID() {
        val tmp = testDirectory("kotlin_object_pack_lookup")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val packDir = File(repo.gitDir, "objects/pack")
            packDir.mkdirs()

            val blobData = "packed object payload\n".toByteArray()
            val blobOid = OID.hashObject(ObjectType.BLOB, blobData)
            val packData = buildTestPack(listOf(Pair(ObjectType.BLOB, blobData)))
            val idxData = buildPackIndex(listOf(blobOid), intArrayOf(0), longArrayOf(12))

            File(packDir, "test.pack").writeBytes(packData)
            File(packDir, "test.idx").writeBytes(idxData)

            val obj = readObject(repo.gitDir, blobOid)
            assertEquals(ObjectType.BLOB, obj.objectType)
            assertEquals(blobData.size, obj.size)
            assertTrue(blobData.contentEquals(obj.data))
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testPeelTagToTargetObject() {
        val tmp = testDirectory("kotlin_object_peel_tag")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val blobData = "peeled blob\n".toByteArray()
            val blobOid = writeBlob(repo.gitDir, blobData)
            val tagData = serializeTag(
                targetId = blobOid,
                targetType = ObjectType.BLOB,
                tagName = "v1.0",
                tagger = null,
                message = "annotated blob tag\n",
            )
            val tagOid = writeLooseObject(repo.gitDir, ObjectType.TAG, tagData)

            val tagObject = readObject(repo.gitDir, tagOid)
            val tag = tagObject.asTag()
            assertEquals(blobOid, tag.targetId)
            assertEquals(ObjectType.BLOB, tag.targetType)

            val peeled = tagObject.peel(repo.gitDir)
            assertEquals(blobOid, peeled.oid)
            assertEquals(ObjectType.BLOB, peeled.objectType)
            assertTrue(blobData.contentEquals(peeled.data))
        } finally {
            tmp.deleteRecursively()
        }
    }

    private fun testDirectory(name: String): File =
        File(System.getProperty("user.dir")).resolve("../tmp/$name")
}
