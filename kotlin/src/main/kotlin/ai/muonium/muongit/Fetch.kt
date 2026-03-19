// Fetch.kt - Fetch, push, and clone operations
// Parity: libgit2 src/libgit2/fetch.c, push.c, clone.c

package ai.muonium.muongit

import java.io.File

// --- Fetch ---

/** Result of computing fetch wants. */
data class FetchNegotiation(
    val wants: List<OID>,
    val haves: List<OID>,
    val matchedRefs: List<MatchedRef>
)

/** A remote ref matched against a fetch refspec. */
data class MatchedRef(
    val remoteName: String,
    val localName: String,
    val oid: OID
)

/** Match a ref name against a refspec pattern (supports trailing glob). */
internal fun refspecMatch(name: String, pattern: String): String? {
    if (pattern.endsWith("*")) {
        val prefix = pattern.dropLast(1)
        return if (name.startsWith(prefix)) name.removePrefix(prefix.toString()) else null
    }
    return if (name == pattern) "" else null
}

/** Apply a refspec to map a remote ref name to a local ref name. */
fun applyRefspec(remoteName: String, refspec: String): String? {
    val parsed = parseRefspec(refspec) ?: return null
    val (_, src, dst) = parsed
    val matched = refspecMatch(remoteName, src) ?: return null

    return if (dst.endsWith("*")) {
        val dstPrefix = dst.dropLast(1)
        "$dstPrefix$matched"
    } else {
        dst
    }
}

/** Compute which objects we need to fetch from the remote. */
fun computeFetchWants(
    remoteRefs: List<RemoteRef>,
    refspecs: List<String>,
    gitDir: File
): FetchNegotiation {
    val wants = mutableListOf<OID>()
    val matchedRefs = mutableListOf<MatchedRef>()
    val seen = mutableSetOf<String>()

    for (rref in remoteRefs) {
        for (refspec in refspecs) {
            val localName = applyRefspec(rref.name, refspec) ?: continue
            matchedRefs.add(MatchedRef(
                remoteName = rref.name,
                localName = localName,
                oid = rref.oid
            ))

            val alreadyHave = try {
                val localOid = resolveReference(gitDir, localName)
                localOid == rref.oid
            } catch (_: Exception) {
                false
            }

            if (!alreadyHave && seen.add(rref.oid.hex)) {
                wants.add(rref.oid)
            }
        }
    }

    val haves = collectLocalRefs(gitDir)
    return FetchNegotiation(wants, haves, matchedRefs)
}

/** Collect all local ref OIDs for negotiation. */
internal fun collectLocalRefs(gitDir: File): List<OID> {
    val oids = mutableListOf<OID>()
    for (dir in listOf("refs/heads", "refs/remotes")) {
        val refDir = File(gitDir, dir)
        if (refDir.isDirectory) {
            collectRefsRecursive(refDir, oids)
        }
    }
    return oids
}

private fun collectRefsRecursive(dir: File, oids: MutableList<OID>) {
    val files = dir.listFiles() ?: return
    for (file in files) {
        if (file.isDirectory) {
            collectRefsRecursive(file, oids)
        } else {
            val hex = file.readText().trim()
            if (hex.length == 40) {
                try {
                    oids.add(OID(hex))
                } catch (_: Exception) {}
            }
        }
    }
}

/** Update local refs after a successful fetch. */
fun updateRefsFromFetch(gitDir: File, matchedRefs: List<MatchedRef>): Int {
    var updated = 0
    for (mref in matchedRefs) {
        writeReference(gitDir, mref.localName, mref.oid)
        updated++
    }
    return updated
}

// --- Push ---

/** A ref update for push. */
data class PushUpdate(
    val srcRef: String,
    val dstRef: String,
    val srcOid: OID,
    val dstOid: OID,
    val force: Boolean
)

/** Compute push updates. */
fun computePushUpdates(
    pushRefspecs: List<String>,
    gitDir: File,
    remoteRefs: List<RemoteRef>
): List<PushUpdate> {
    val updates = mutableListOf<PushUpdate>()

    for (refspec in pushRefspecs) {
        val parsed = parseRefspec(refspec)
            ?: throw MuonGitException.InvalidObject("invalid push refspec: $refspec")
        val (force, src, dst) = parsed

        val srcOid = resolveReference(gitDir, src)
        val dstOid = remoteRefs.firstOrNull { it.name == dst }?.oid ?: OID(ByteArray(20))

        updates.add(PushUpdate(
            srcRef = src,
            dstRef = dst,
            srcOid = srcOid,
            dstOid = dstOid,
            force = force
        ))
    }

    return updates
}

/** Build a push report string. */
fun buildPushReport(updates: List<PushUpdate>): String {
    return updates.joinToString("") { u ->
        "${u.dstOid.hex} ${u.srcOid.hex} ${u.dstRef}\n"
    }
}

// --- Clone ---

/** Options for clone. */
data class CloneOptions(
    val remoteName: String = "origin",
    val branch: String? = null,
    val bare: Boolean = false
)

/** Set up a new repository for clone. */
fun cloneSetup(path: String, url: String, options: CloneOptions = CloneOptions()): Repository {
    val repo = Repository.init(path, options.bare)
    addRemote(repo.gitDir, options.remoteName, url)

    if (options.branch != null) {
        val target = "refs/heads/${options.branch}"
        writeSymbolicReference(repo.gitDir, "HEAD", target)
    }

    return repo
}

/** After fetching, set up HEAD and the default branch for a clone. */
fun cloneFinish(gitDir: File, remoteName: String, defaultBranch: String, headOid: OID) {
    val localBranch = "refs/heads/$defaultBranch"
    val remoteRef = "refs/remotes/$remoteName/$defaultBranch"

    writeReference(gitDir, localBranch, headOid)
    writeReference(gitDir, remoteRef, headOid)
    writeSymbolicReference(gitDir, "HEAD", localBranch)
}

/** Extract the default branch from server capabilities. */
fun defaultBranchFromCaps(caps: ServerCapabilities): String? {
    val symref = caps.get("symref") ?: return null
    val colonIdx = symref.indexOf(':')
    if (colonIdx < 0) return null
    val headPart = symref.substring(0, colonIdx)
    val target = symref.substring(colonIdx + 1)
    if (headPart != "HEAD") return null
    return if (target.startsWith("refs/heads/")) {
        target.removePrefix("refs/heads/")
    } else {
        null
    }
}
