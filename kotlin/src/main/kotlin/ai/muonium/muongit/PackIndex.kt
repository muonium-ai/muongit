package ai.muonium.muongit

import java.io.File

private val IDX_MAGIC = byteArrayOf(0xFF.toByte(), 0x74, 0x4F, 0x63) // "\377tOc"
private const val IDX_VERSION = 2
private const val FANOUT_COUNT = 256

/** A parsed pack index file */
data class PackIndex(
    val count: Int,
    val fanout: IntArray,
    val oids: List<OID>,
    val crcs: IntArray,
    val offsets: LongArray,
) {
    /** Look up an OID in the index. Returns the pack file offset if found. */
    fun find(oid: OID): Long? {
        val raw = oid.raw
        if (raw.isEmpty()) return null
        val firstByte = raw[0].toInt() and 0xFF

        val start = if (firstByte == 0) 0 else fanout[firstByte - 1]
        val end = fanout[firstByte]

        // Binary search within the range
        var lo = start
        var hi = end
        while (lo < hi) {
            val mid = lo + (hi - lo) / 2
            val cmp = compareBytes(oids[mid].raw, raw)
            if (cmp < 0) lo = mid + 1
            else if (cmp > 0) hi = mid
            else return offsets[mid]
        }
        return null
    }

    /** Check if the index contains a given OID. */
    fun contains(oid: OID): Boolean = find(oid) != null

    override fun equals(other: Any?): Boolean =
        other is PackIndex && count == other.count &&
        fanout.contentEquals(other.fanout) && oids == other.oids &&
        crcs.contentEquals(other.crcs) && offsets.contentEquals(other.offsets)

    override fun hashCode(): Int = count.hashCode()
}

internal fun compareBytes(a: ByteArray, b: ByteArray): Int {
    for (i in 0 until minOf(a.size, b.size)) {
        val av = a[i].toInt() and 0xFF
        val bv = b[i].toInt() and 0xFF
        if (av < bv) return -1
        if (av > bv) return 1
    }
    return a.size - b.size
}

internal fun readPackU32(data: ByteArray, offset: Int): Int =
    ((data[offset].toInt() and 0xFF) shl 24) or
    ((data[offset + 1].toInt() and 0xFF) shl 16) or
    ((data[offset + 2].toInt() and 0xFF) shl 8) or
    (data[offset + 3].toInt() and 0xFF)

/** Parse a pack index file from disk. */
fun readPackIndex(path: String): PackIndex {
    val data = File(path).readBytes()
    return parsePackIndex(data)
}

/** Parse pack index bytes. */
fun parsePackIndex(data: ByteArray): PackIndex {
    if (data.size < 1072) throw MuonGitException.InvalidObject("pack index too short")

    if (data[0] != 0xFF.toByte() || data[1] != 0x74.toByte() ||
        data[2] != 0x4F.toByte() || data[3] != 0x63.toByte()) {
        throw MuonGitException.InvalidObject("bad pack index magic")
    }
    val version = readPackU32(data, 4)
    if (version != IDX_VERSION) throw MuonGitException.InvalidObject("unsupported pack index version $version")

    val fanout = IntArray(FANOUT_COUNT)
    for (i in 0 until FANOUT_COUNT) {
        fanout[i] = readPackU32(data, 8 + i * 4)
    }
    val count = fanout[255]

    val oidTableStart = 8 + FANOUT_COUNT * 4
    val crcTableStart = oidTableStart + count * 20
    val offsetTableStart = crcTableStart + count * 4
    val minSize = offsetTableStart + count * 4 + 40
    if (data.size < minSize) throw MuonGitException.InvalidObject("pack index truncated")

    val oids = mutableListOf<OID>()
    for (i in 0 until count) {
        val start = oidTableStart + i * 20
        oids.add(OID(data.copyOfRange(start, start + 20)))
    }

    val crcs = IntArray(count)
    for (i in 0 until count) {
        crcs[i] = readPackU32(data, crcTableStart + i * 4)
    }

    val largeOffsetStart = offsetTableStart + count * 4
    val offsets = LongArray(count)
    for (i in 0 until count) {
        val rawOffset = readPackU32(data, offsetTableStart + i * 4)
        if (rawOffset and 0x80000000.toInt() != 0) {
            val largeIdx = rawOffset and 0x7FFFFFFF
            val lo = largeOffsetStart + largeIdx * 8
            if (lo + 8 > data.size) throw MuonGitException.InvalidObject("pack index large offset out of bounds")
            var val64 = 0L
            for (j in 0 until 8) {
                val64 = (val64 shl 8) or (data[lo + j].toLong() and 0xFF)
            }
            offsets[i] = val64
        } else {
            offsets[i] = rawOffset.toLong() and 0xFFFFFFFFL
        }
    }

    return PackIndex(count = count, fanout = fanout, oids = oids, crcs = crcs, offsets = offsets)
}

/** Build a pack index from components (for testing). */
internal fun buildPackIndex(oids: List<OID>, crcs: IntArray, offsets: LongArray): ByteArray {
    return buildPackIndexWithChecksums(
        oids = oids,
        crcs = crcs,
        offsets = offsets,
        packChecksum = ByteArray(20)
    )
}

internal fun buildPackIndexWithChecksums(
    oids: List<OID>,
    crcs: IntArray,
    offsets: LongArray,
    packChecksum: ByteArray,
): ByteArray {
    val buf = java.io.ByteArrayOutputStream()

    buf.write(IDX_MAGIC)
    buf.write(writePackIdxU32(IDX_VERSION))

    // Build fanout table
    val fanout = IntArray(FANOUT_COUNT)
    for (oid in oids) {
        val first = oid.raw[0].toInt() and 0xFF
        for (j in first until FANOUT_COUNT) {
            fanout[j]++
        }
    }
    for (f in fanout) {
        buf.write(writePackIdxU32(f))
    }

    for (oid in oids) {
        buf.write(oid.raw)
    }

    for (crc in crcs) {
        buf.write(writePackIdxU32(crc))
    }

    val largeOffsets = mutableListOf<Long>()
    for (offset in offsets) {
        if (offset > 0x7FFF_FFFFL) {
            val idx = largeOffsets.size
            buf.write(writePackIdxU32(0x80000000.toInt() or idx))
            largeOffsets.add(offset)
        } else {
            buf.write(writePackIdxU32((offset and 0xFFFFFFFFL).toInt()))
        }
    }

    for (offset in largeOffsets) {
        val raw = ByteArray(8)
        var value = offset
        for (i in 7 downTo 0) {
            raw[i] = (value and 0xFF).toByte()
            value = value shr 8
        }
        buf.write(raw)
    }

    buf.write(packChecksum)

    val checksum = SHA1.hash(buf.toByteArray())
    buf.write(checksum)

    return buf.toByteArray()
}

private fun writePackIdxU32(value: Int): ByteArray = byteArrayOf(
    ((value shr 24) and 0xFF).toByte(),
    ((value shr 16) and 0xFF).toByte(),
    ((value shr 8) and 0xFF).toByte(),
    (value and 0xFF).toByte(),
)
