package ai.muonium.muongit

// MuonGit - Native Kotlin port of libgit2
// API parity target: libgit2 v1.9.0

// -- Core Types --

/** Object identifier (SHA-1 / SHA-256) */
data class OID(val raw: ByteArray) {

    constructor(hex: String) : this(
        hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    )

    val hex: String get() = raw.joinToString("") { "%02x".format(it) }

    override fun equals(other: Any?): Boolean =
        other is OID && raw.contentEquals(other.raw)

    override fun hashCode(): Int = raw.contentHashCode()

    override fun toString(): String = hex

    companion object
}

/** Git object types */
enum class ObjectType(val value: Int) {
    COMMIT(1),
    TREE(2),
    BLOB(3),
    TAG(4);
}

/** Git signature (author/committer) */
data class Signature(
    val name: String,
    val email: String,
    val time: Long = 0L,
    val offset: Int = 0
)

// -- Error Handling --

/** Errors from MuonGit operations */
sealed class MuonGitException(message: String) : Exception(message) {
    class NotFound(msg: String) : MuonGitException(msg)
    class InvalidObject(msg: String) : MuonGitException(msg)
    class Ambiguous(msg: String) : MuonGitException(msg)
    class BufferTooShort : MuonGitException("buffer too short")
    class BareRepo : MuonGitException("operation not allowed on bare repo")
    class UnbornBranch : MuonGitException("unborn branch")
    class Unmerged : MuonGitException("unmerged entries exist")
    class NotFastForward : MuonGitException("not fast-forward")
    class InvalidSpec(msg: String) : MuonGitException(msg)
    class Conflict(msg: String) : MuonGitException(msg)
    class Locked(msg: String) : MuonGitException(msg)
    class Auth(msg: String) : MuonGitException(msg)
    class Certificate(msg: String) : MuonGitException(msg)
}

// -- Version --

/** Library version information */
object MuonGitVersion {
    const val MAJOR = 0
    const val MINOR = 1
    const val PATCH = 0
    const val STRING = "$MAJOR.$MINOR.$PATCH"
    const val LIBGIT2_PARITY = "1.9.0"
}
