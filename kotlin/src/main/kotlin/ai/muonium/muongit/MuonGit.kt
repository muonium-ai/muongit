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

// -- Repository --

/** A Git repository */
class Repository private constructor(
    /** Path to the .git directory */
    val gitDir: java.io.File,
    /** Path to the working directory (null for bare repos) */
    val workdir: java.io.File?,
    /** Whether this is a bare repository */
    val isBare: Boolean
) {

    /** Read HEAD reference */
    fun head(): String = java.io.File(gitDir, "HEAD").readText().trim()

    /** Check if HEAD is unborn */
    val isHeadUnborn: Boolean
        get() {
            val headContent = try { head() } catch (_: Exception) { return true }
            if (headContent.startsWith("ref: ")) {
                val refPath = java.io.File(gitDir, headContent.removePrefix("ref: "))
                return !refPath.exists()
            }
            return false
        }

    companion object {
        /** Open an existing repository at the given path */
        fun open(path: String): Repository {
            val dir = java.io.File(path)

            // Check if path itself is a bare repo
            if (isGitDir(dir)) {
                return Repository(gitDir = dir, workdir = null, isBare = true)
            }

            // Check for .git directory
            val gitDir = java.io.File(dir, ".git")
            if (isGitDir(gitDir)) {
                return Repository(gitDir = gitDir, workdir = dir, isBare = false)
            }

            throw MuonGitException.NotFound("could not find repository at '$path'")
        }

        /** Discover a repository by walking up from the given path */
        fun discover(path: String): Repository {
            var current = java.io.File(path).canonicalFile
            while (true) {
                try { return open(current.path) } catch (_: MuonGitException.NotFound) {}
                val parent = current.parentFile ?: break
                if (parent == current) break
                current = parent
            }
            throw MuonGitException.NotFound("could not find repository in any parent directory")
        }

        /** Initialize a new repository */
        fun init(path: String, bare: Boolean = false): Repository {
            val dir = java.io.File(path)
            val gitDir = if (bare) dir else java.io.File(dir, ".git")

            gitDir.mkdirs()
            java.io.File(gitDir, "objects").mkdirs()
            java.io.File(gitDir, "refs/heads").mkdirs()
            java.io.File(gitDir, "refs/tags").mkdirs()

            java.io.File(gitDir, "HEAD").writeText("ref: refs/heads/main\n")

            val config = if (bare) {
                "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n"
            } else {
                "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n"
            }
            java.io.File(gitDir, "config").writeText(config)

            return Repository(gitDir = gitDir, workdir = if (bare) null else dir, isBare = bare)
        }

        /** Clone a repository from a URL */
        fun clone(url: String, path: String): Repository {
            TODO("implement clone - requires network transport")
        }

        private fun isGitDir(dir: java.io.File): Boolean =
            java.io.File(dir, "HEAD").exists() &&
            java.io.File(dir, "objects").isDirectory &&
            java.io.File(dir, "refs").isDirectory
    }
}

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
    val MAJOR: Int get() = GeneratedVersion.MAJOR
    val MINOR: Int get() = GeneratedVersion.MINOR
    val PATCH: Int get() = GeneratedVersion.PATCH
    val STRING: String get() = GeneratedVersion.STRING
    const val LIBGIT2_PARITY = "1.9.0"
}
