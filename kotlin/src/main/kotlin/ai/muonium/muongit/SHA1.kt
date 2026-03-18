package ai.muonium.muongit

/// SHA-1 implementation using java.security.MessageDigest for hardware-accelerated hashing.
/// Parity: libgit2 uses SHA-1 for object IDs (see src/util/hash/sha1/)

import java.security.MessageDigest

class SHA1 {
    private val digest: MessageDigest = MessageDigest.getInstance("SHA-1")

    fun update(data: ByteArray) {
        digest.update(data)
    }

    fun update(string: String) {
        update(string.encodeToByteArray())
    }

    fun finalize(): ByteArray {
        return digest.digest()
    }

    companion object {
        fun hash(data: ByteArray): ByteArray {
            return MessageDigest.getInstance("SHA-1").digest(data)
        }

        fun hash(string: String): ByteArray = hash(string.encodeToByteArray())
    }
}

// OID SHA-1 extensions
fun OID.Companion.hashObject(type: ObjectType, data: ByteArray): OID {
    val typeName = when (type) {
        ObjectType.COMMIT -> "commit"
        ObjectType.TREE -> "tree"
        ObjectType.BLOB -> "blob"
        ObjectType.TAG -> "tag"
    }

    val header = "$typeName ${data.size}\u0000"
    val sha = SHA1()
    sha.update(header.encodeToByteArray())
    sha.update(data)
    return OID(sha.finalize())
}

val OID.Companion.SHA1_LENGTH get() = 20
val OID.Companion.SHA1_HEX_LENGTH get() = 40
val OID.isZero get() = raw.all { it == 0.toByte() }
val OID.Companion.ZERO get() = OID(ByteArray(20))
