package ai.muonium.muongit

/** A parsed git blob object */
data class Blob(
    val oid: OID,
    val data: ByteArray,
    val size: Int = data.size
) {
    override fun equals(other: Any?): Boolean =
        other is Blob && oid == other.oid && data.contentEquals(other.data)

    override fun hashCode(): Int = oid.hashCode()
}

/** Read a blob from the object database */
fun readBlob(gitDir: java.io.File, oid: OID): Blob {
    val (objType, data) = readLooseObject(gitDir, oid)
    if (objType != ObjectType.BLOB) {
        throw MuonGitException.InvalidObject("expected blob, got $objType")
    }
    return Blob(oid = oid, data = data)
}

/** Write data as a blob to the object database, returns the OID */
fun writeBlob(gitDir: java.io.File, data: ByteArray): OID {
    return writeLooseObject(gitDir, ObjectType.BLOB, data)
}

/** Write a file's contents as a blob to the object database */
fun writeBlobFromFile(gitDir: java.io.File, path: String): OID {
    val data = java.io.File(path).readBytes()
    return writeBlob(gitDir, data)
}

/** Compute the blob OID for data without writing to the ODB (hash-object --stdin) */
fun hashBlob(data: ByteArray): OID {
    return OID.hashObject(ObjectType.BLOB, data)
}
