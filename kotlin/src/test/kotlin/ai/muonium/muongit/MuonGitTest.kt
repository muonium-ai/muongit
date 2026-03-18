package ai.muonium.muongit

import kotlin.test.Test
import kotlin.test.assertEquals
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
}
