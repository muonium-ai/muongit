package ai.muonium.muongit

/// Pure Kotlin SHA-1 implementation
/// Parity: libgit2 uses SHA-1 for object IDs (see src/util/hash/sha1/)

class SHA1 {
    private var h0: UInt = 0x67452301u
    private var h1: UInt = 0xEFCDAB89u
    private var h2: UInt = 0x98BADCFEu
    private var h3: UInt = 0x10325476u
    private var h4: UInt = 0xC3D2E1F0u

    private val buffer = mutableListOf<Byte>()
    private var totalLength: Long = 0L

    fun update(data: ByteArray) {
        buffer.addAll(data.toList())
        totalLength += data.size

        while (buffer.size >= 64) {
            val block = buffer.subList(0, 64).toByteArray()
            processBlock(block)
            repeat(64) { buffer.removeFirst() }
        }
    }

    fun update(string: String) {
        update(string.encodeToByteArray())
    }

    fun finalize(): ByteArray {
        val padded = buffer.toMutableList()
        padded.add(0x80.toByte())

        while (padded.size % 64 != 56) {
            padded.add(0x00.toByte())
        }

        val bitLength = totalLength * 8
        for (i in 7 downTo 0) {
            padded.add(((bitLength shr (i * 8)) and 0xFF).toByte())
        }

        var offset = 0
        while (offset < padded.size) {
            val block = padded.subList(offset, offset + 64).toByteArray()
            processBlock(block)
            offset += 64
        }

        val digest = ByteArray(20)
        for ((i, h) in listOf(h0, h1, h2, h3, h4).withIndex()) {
            digest[i * 4]     = ((h shr 24) and 0xFFu).toByte()
            digest[i * 4 + 1] = ((h shr 16) and 0xFFu).toByte()
            digest[i * 4 + 2] = ((h shr 8) and 0xFFu).toByte()
            digest[i * 4 + 3] = (h and 0xFFu).toByte()
        }
        return digest
    }

    private fun processBlock(block: ByteArray) {
        val w = UIntArray(80)

        for (i in 0 until 16) {
            w[i] = ((block[i * 4].toUInt() and 0xFFu) shl 24) or
                   ((block[i * 4 + 1].toUInt() and 0xFFu) shl 16) or
                   ((block[i * 4 + 2].toUInt() and 0xFFu) shl 8) or
                   (block[i * 4 + 3].toUInt() and 0xFFu)
        }

        for (i in 16 until 80) {
            w[i] = (w[i - 3] xor w[i - 8] xor w[i - 14] xor w[i - 16]).rotateLeft(1)
        }

        var a = h0; var b = h1; var c = h2; var d = h3; var e = h4

        for (i in 0 until 80) {
            val (f, k) = when (i) {
                in 0..19 -> Pair((b and c) or (b.inv() and d), 0x5A827999u)
                in 20..39 -> Pair(b xor c xor d, 0x6ED9EBA1u)
                in 40..59 -> Pair((b and c) or (b and d) or (c and d), 0x8F1BBCDCu)
                else -> Pair(b xor c xor d, 0xCA62C1D6u)
            }

            val temp = a.rotateLeft(5) + f + e + k + w[i]
            e = d
            d = c
            c = b.rotateLeft(30)
            b = a
            a = temp
        }

        h0 += a; h1 += b; h2 += c; h3 += d; h4 += e
    }

    companion object {
        fun hash(data: ByteArray): ByteArray {
            val sha = SHA1()
            sha.update(data)
            return sha.finalize()
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
