package ai.muonium.muongit

/** A parsed git annotated tag object */
data class Tag(
    val oid: OID,
    val targetId: OID,
    val targetType: ObjectType,
    val tagName: String,
    val tagger: Signature?,
    val message: String
)

/** Parse a tag object from its raw data content */
fun parseTag(oid: OID, data: ByteArray): Tag {
    val text = data.decodeToString()

    var targetId: OID? = null
    var targetType: ObjectType? = null
    var tagName: String? = null
    var tagger: Signature? = null

    val blankIdx = text.indexOf("\n\n")
    val headerSection = if (blankIdx >= 0) text.substring(0, blankIdx) else text
    val message = if (blankIdx >= 0) text.substring(blankIdx + 2) else ""

    for (line in headerSection.split("\n")) {
        when {
            line.startsWith("object ") -> targetId = OID(line.removePrefix("object "))
            line.startsWith("type ") -> targetType = parseObjectTypeName(line.removePrefix("type "))
            line.startsWith("tag ") -> tagName = line.removePrefix("tag ")
            line.startsWith("tagger ") -> tagger = parseSignatureLine(line.removePrefix("tagger "))
        }
    }

    return Tag(
        oid = oid,
        targetId = targetId ?: throw MuonGitException.InvalidObject("tag missing object"),
        targetType = targetType ?: throw MuonGitException.InvalidObject("tag missing type"),
        tagName = tagName ?: throw MuonGitException.InvalidObject("tag missing tag name"),
        tagger = tagger,
        message = message
    )
}

/** Serialize a tag to its raw data representation (without the object header) */
fun serializeTag(
    targetId: OID,
    targetType: ObjectType,
    tagName: String,
    tagger: Signature?,
    message: String
): ByteArray {
    val sb = StringBuilder()
    sb.append("object ").append(targetId.hex).append('\n')
    sb.append("type ").append(objectTypeName(targetType)).append('\n')
    sb.append("tag ").append(tagName).append('\n')
    if (tagger != null) {
        sb.append("tagger ").append(formatSignatureLine(tagger)).append('\n')
    }
    sb.append('\n')
    sb.append(message)
    return sb.toString().toByteArray()
}

internal fun objectTypeName(type: ObjectType): String = when (type) {
    ObjectType.COMMIT -> "commit"
    ObjectType.TREE -> "tree"
    ObjectType.BLOB -> "blob"
    ObjectType.TAG -> "tag"
}

internal fun parseObjectTypeName(name: String): ObjectType? = when (name) {
    "commit" -> ObjectType.COMMIT
    "tree" -> ObjectType.TREE
    "blob" -> ObjectType.BLOB
    "tag" -> ObjectType.TAG
    else -> null
}
