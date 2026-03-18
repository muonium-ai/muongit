package ai.muonium.muongit

/// SHA-256 implementation using java.security.MessageDigest for hardware-accelerated hashing.
/// Parity: libgit2 EXPERIMENTAL_SHA256 uses SHA-256 for object IDs

import java.security.MessageDigest

@OptIn(ExperimentalUnsignedTypes::class)
class SHA256Hash {
    private val digest: MessageDigest = MessageDigest.getInstance("SHA-256")

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
            return MessageDigest.getInstance("SHA-256").digest(data)
        }

        fun hash(string: String): ByteArray = hash(string.encodeToByteArray())
    }
}

/** Hash algorithm selection (matching libgit2 EXPERIMENTAL_SHA256) */
enum class HashAlgorithm(val digestLength: Int) {
    SHA1(20),
    SHA256(32);

    /** Hex string length */
    val hexLength: Int get() = digestLength * 2
}

// OID SHA-256 extensions
fun OID.Companion.hashObjectSHA256(type: ObjectType, data: ByteArray): OID {
    val typeName = when (type) {
        ObjectType.COMMIT -> "commit"
        ObjectType.TREE -> "tree"
        ObjectType.BLOB -> "blob"
        ObjectType.TAG -> "tag"
    }

    val header = "$typeName ${data.size}\u0000"
    val sha = SHA256Hash()
    sha.update(header.encodeToByteArray())
    sha.update(data)
    return OID(sha.finalize())
}

val OID.Companion.SHA256_LENGTH get() = 32
val OID.Companion.SHA256_HEX_LENGTH get() = 64
val OID.Companion.ZERO_SHA256 get() = OID(ByteArray(32))
