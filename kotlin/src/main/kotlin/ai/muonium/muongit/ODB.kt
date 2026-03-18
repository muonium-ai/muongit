package ai.muonium.muongit

import java.io.File
import java.util.zip.Deflater
import java.util.zip.Inflater

/// Loose object read/write for the git object database.
/// Parity: libgit2 src/libgit2/odb_loose.c

/**
 * Read a loose object from the object database.
 *
 * @param gitDir Path to the .git directory
 * @param oid Object identifier to read
 * @return Pair of (ObjectType, content bytes)
 * @throws MuonGitException.NotFound if the object does not exist
 * @throws MuonGitException.InvalidObject if the object is malformed
 */
fun readLooseObject(gitDir: File, oid: OID): Pair<ObjectType, ByteArray> {
    val hex = oid.hex
    val objectFile = File(gitDir, "objects/${hex.substring(0, 2)}/${hex.substring(2)}")

    if (!objectFile.exists()) {
        throw MuonGitException.NotFound("object not found: $hex")
    }

    val compressed = objectFile.readBytes()

    // Decompress using Inflater
    val inflater = Inflater()
    inflater.setInput(compressed)
    val outputBuffer = ByteArray(compressed.size * 4) // initial estimate
    var totalSize = 0
    var result = ByteArray(outputBuffer.size)

    try {
        while (!inflater.finished()) {
            if (totalSize == result.size) {
                result = result.copyOf(result.size * 2)
            }
            val count = inflater.inflate(result, totalSize, result.size - totalSize)
            if (count == 0 && !inflater.finished()) {
                // Need more output space
                result = result.copyOf(result.size * 2)
            }
            totalSize += count
        }
    } finally {
        inflater.end()
    }

    val decompressed = result.copyOf(totalSize)

    // Parse header: "{type} {size}\0{content}"
    val nullIndex = decompressed.indexOf(0.toByte())
    if (nullIndex < 0) {
        throw MuonGitException.InvalidObject("malformed object header: missing null byte")
    }

    val header = String(decompressed, 0, nullIndex, Charsets.US_ASCII)
    val spaceIndex = header.indexOf(' ')
    if (spaceIndex < 0) {
        throw MuonGitException.InvalidObject("malformed object header: missing space")
    }

    val typeName = header.substring(0, spaceIndex)
    val size = header.substring(spaceIndex + 1).toLongOrNull()
        ?: throw MuonGitException.InvalidObject("malformed object header: invalid size")

    val objectType = when (typeName) {
        "commit" -> ObjectType.COMMIT
        "tree" -> ObjectType.TREE
        "blob" -> ObjectType.BLOB
        "tag" -> ObjectType.TAG
        else -> throw MuonGitException.InvalidObject("unknown object type: $typeName")
    }

    val content = decompressed.copyOfRange(nullIndex + 1, decompressed.size)

    if (content.size.toLong() != size) {
        throw MuonGitException.InvalidObject(
            "object size mismatch: header says $size, actual ${content.size}"
        )
    }

    return Pair(objectType, content)
}

/**
 * Write a loose object to the object database.
 *
 * @param gitDir Path to the .git directory
 * @param type The object type
 * @param data The raw content bytes
 * @return The OID of the written object
 */
fun writeLooseObject(gitDir: File, type: ObjectType, data: ByteArray): OID {
    // Compute OID using the existing hashObject extension
    val oid = OID.hashObject(type, data)
    val hex = oid.hex

    val objectDir = File(gitDir, "objects/${hex.substring(0, 2)}")
    val objectFile = File(objectDir, hex.substring(2))

    // If the object already exists, no need to write again
    if (objectFile.exists()) {
        return oid
    }

    // Build the full object: "{type} {size}\0{content}"
    val typeName = when (type) {
        ObjectType.COMMIT -> "commit"
        ObjectType.TREE -> "tree"
        ObjectType.BLOB -> "blob"
        ObjectType.TAG -> "tag"
    }
    val header = "$typeName ${data.size}\u0000".toByteArray(Charsets.US_ASCII)
    val fullObject = header + data

    // Compress with Deflater
    val deflater = Deflater()
    deflater.setInput(fullObject)
    deflater.finish()

    val compressedBuffer = ByteArray(fullObject.size + 64)
    var totalCompressed = 0

    try {
        var compressed = compressedBuffer
        while (!deflater.finished()) {
            if (totalCompressed == compressed.size) {
                compressed = compressed.copyOf(compressed.size * 2)
            }
            val count = deflater.deflate(compressed, totalCompressed, compressed.size - totalCompressed)
            totalCompressed += count
        }
        val compressedData = compressed.copyOf(totalCompressed)

        // Write to disk
        objectDir.mkdirs()
        objectFile.writeBytes(compressedData)
    } finally {
        deflater.end()
    }

    return oid
}
