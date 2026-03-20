// Fetch.kt - Fetch, push, and clone operations
// Parity: libgit2 src/libgit2/fetch.c, push.c, clone.c

package ai.muonium.muongit

import java.io.ByteArrayOutputStream
import java.io.File

// --- Fetch ---

/** Result of computing fetch wants. */
data class FetchNegotiation(
    val wants: List<OID>,
    val haves: List<OID>,
    val matchedRefs: List<MatchedRef>,
)

/** A remote ref matched against a fetch refspec. */
data class MatchedRef(
    val remoteName: String,
    val localName: String,
    val oid: OID,
)

/** Match a ref name against a refspec pattern (supports trailing glob). */
internal fun refspecMatch(name: String, pattern: String): String? {
    if (pattern.endsWith("*")) {
        val prefix = pattern.dropLast(1)
        return if (name.startsWith(prefix)) name.removePrefix(prefix) else null
    }
    return if (name == pattern) "" else null
}

/** Apply a refspec to map a remote ref name to a local ref name. */
fun applyRefspec(remoteName: String, refspec: String): String? {
    val (_, src, dst) = parseRefspec(refspec) ?: return null
    val matched = refspecMatch(remoteName, src) ?: return null

    return if (dst.endsWith("*")) {
        "${dst.dropLast(1)}$matched"
    } else {
        dst
    }
}

/** Compute which objects we need to fetch from the remote. */
fun computeFetchWants(
    remoteRefs: List<RemoteRef>,
    refspecs: List<String>,
    gitDir: File,
): FetchNegotiation {
    val wants = mutableListOf<OID>()
    val matchedRefs = mutableListOf<MatchedRef>()
    val seen = mutableSetOf<OID>()

    for (rref in remoteRefs) {
        for (refspec in refspecs) {
            val localName = applyRefspec(rref.name, refspec) ?: continue
            matchedRefs.add(
                MatchedRef(
                    remoteName = rref.name,
                    localName = localName,
                    oid = rref.oid,
                )
            )

            val alreadyHave = runCatching { resolveReference(gitDir, localName) }
                .getOrNull() == rref.oid

            if (!alreadyHave && seen.add(rref.oid)) {
                wants.add(rref.oid)
            }
        }
    }

    return FetchNegotiation(
        wants = wants,
        haves = collectLocalRefs(gitDir),
        matchedRefs = matchedRefs,
    )
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
                runCatching { OID(hex) }.getOrNull()?.let(oids::add)
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
    val force: Boolean,
)

/** Compute push updates. */
fun computePushUpdates(
    pushRefspecs: List<String>,
    gitDir: File,
    remoteRefs: List<RemoteRef>,
): List<PushUpdate> {
    val updates = mutableListOf<PushUpdate>()

    for (refspec in pushRefspecs) {
        val (force, src, dst) = parseRefspec(refspec)
            ?: throw MuonGitException.InvalidObject("invalid push refspec: $refspec")

        val srcOid = resolveReference(gitDir, src)
        val dstOid = remoteRefs.firstOrNull { it.name == dst }?.oid ?: OID.ZERO

        updates.add(
            PushUpdate(
                srcRef = src,
                dstRef = dst,
                srcOid = srcOid,
                dstOid = dstOid,
                force = force,
            )
        )
    }

    return updates
}

/** Build a push report string. */
fun buildPushReport(updates: List<PushUpdate>): String {
    return updates.joinToString("") { update ->
        "${update.dstOid.hex} ${update.srcOid.hex} ${update.dstRef}\n"
    }
}

data class FetchOptions(
    val refspecs: List<String>? = null,
    val transport: TransportOptions = TransportOptions(),
)

data class FetchResult(
    val advertisedRefs: List<RemoteRef>,
    val capabilities: ServerCapabilities,
    val matchedRefs: List<MatchedRef>,
    val updatedRefs: Int,
    val indexedPack: IndexedPack?,
)

data class PushOptions(
    val refspecs: List<String>? = null,
    val transport: TransportOptions = TransportOptions(),
)

data class PushResult(
    val advertisedRefs: List<RemoteRef>,
    val updatedTrackingRefs: Int,
    val report: String,
)

// --- Clone ---

/** Options for clone. */
data class CloneOptions(
    val remoteName: String = "origin",
    val branch: String? = null,
    val bare: Boolean = false,
    val transport: TransportOptions = TransportOptions(),
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

fun cloneRepository(
    url: String,
    path: String,
    options: CloneOptions = CloneOptions(),
): Repository {
    val repo = cloneSetup(path, url, options)
    val fetch = fetchRemote(
        repository = repo,
        remoteName = options.remoteName,
        options = FetchOptions(transport = options.transport),
    )
    val (branch, headOid) = resolveCloneHead(fetch, options.branch)
    cloneFinish(repo.gitDir, options.remoteName, branch, headOid)
    if (!options.bare) {
        reset(repo.gitDir, repo.workdir, "HEAD", ResetMode.HARD)
    }
    return repo
}

fun fetchRemote(
    repository: Repository,
    remoteName: String,
    options: FetchOptions = FetchOptions(),
): FetchResult {
    val remote = getRemote(repository.gitDir, remoteName)
    val advertisement = advertiseUploadPack(remote.url, options.transport)
    val refspecs = options.refspecs ?: remote.fetchRefspecs
    val negotiation = computeFetchWants(advertisement.refs, refspecs, repository.gitDir)

    if (negotiation.wants.isEmpty()) {
        val updated = updateRefsFromFetch(repository.gitDir, negotiation.matchedRefs)
        return FetchResult(
            advertisedRefs = advertisement.refs,
            capabilities = advertisement.capabilities,
            matchedRefs = negotiation.matchedRefs,
            updatedRefs = updated,
            indexedPack = null,
        )
    }

    val request = buildWantHave(
        wants = negotiation.wants,
        haves = negotiation.haves,
        caps = fetchCapabilities(advertisement.capabilities),
    )
    val response = uploadPack(remote.url, request, options.transport)
    val indexedPack = extractPackFromFetchResponse(response)
        ?.let { indexPackToODB(repository.gitDir, it) }
    val updated = updateRefsFromFetch(repository.gitDir, negotiation.matchedRefs)

    return FetchResult(
        advertisedRefs = advertisement.refs,
        capabilities = advertisement.capabilities,
        matchedRefs = negotiation.matchedRefs,
        updatedRefs = updated,
        indexedPack = indexedPack,
    )
}

fun pushRemote(
    repository: Repository,
    remoteName: String,
    options: PushOptions = PushOptions(),
): PushResult {
    val remote = getRemote(repository.gitDir, remoteName)
    val advertisement = advertiseReceivePack(remote.url, options.transport)
    val refspecs = options.refspecs ?: defaultPushRefspecs(repository.gitDir)
    val updates = computePushUpdates(refspecs, repository.gitDir, advertisement.refs)

    for (update in updates) {
        if (!update.force && !isFastForward(repository.gitDir, update.dstOid, update.srcOid)) {
            throw MuonGitException.NotFastForward()
        }
    }

    val pack = buildPackFromOIDs(
        gitDir = repository.gitDir,
        roots = updates.map { it.srcOid },
        exclude = advertisement.refs.map { it.oid },
    )
    val request = buildPushRequest(updates, pack, advertisement.capabilities)
    val response = receivePack(remote.url, request, options.transport)
    val report = parsePushResponse(response)
    val updatedTrackingRefs = updateTrackingRefsAfterPush(repository.gitDir, remoteName, updates)

    return PushResult(
        advertisedRefs = advertisement.refs,
        updatedTrackingRefs = updatedTrackingRefs,
        report = report,
    )
}

fun Repository.fetch(
    remoteName: String,
    options: FetchOptions = FetchOptions(),
): FetchResult = fetchRemote(this, remoteName, options)

fun Repository.push(
    remoteName: String,
    options: PushOptions = PushOptions(),
): PushResult = pushRemote(this, remoteName, options)

private fun fetchCapabilities(caps: ServerCapabilities): List<String> {
    val requested = mutableListOf<String>()
    if (caps.has("side-band-64k")) {
        requested.add("side-band-64k")
    } else if (caps.has("side-band")) {
        requested.add("side-band")
    }
    if (caps.has("ofs-delta")) {
        requested.add("ofs-delta")
    }
    if (caps.has("include-tag")) {
        requested.add("include-tag")
    }
    return requested
}

private fun extractPackFromFetchResponse(response: ByteArray): ByteArray? {
    val (lines, consumed) = pktLineDecode(response)
    val pack = ByteArrayOutputStream()

    for (line in lines) {
        if (line !is PktLine.Data) {
            continue
        }
        if (startsWithBytes(line.bytes, "ACK ".toByteArray()) || line.bytes.contentEquals("NAK\n".toByteArray())) {
            continue
        }
        when (line.bytes.firstOrNull()?.toInt()) {
            1 -> pack.write(line.bytes, 1, line.bytes.size - 1)
            2 -> Unit
            3 -> throw MuonGitException.InvalidObject(
                String(line.bytes.copyOfRange(1, line.bytes.size), Charsets.UTF_8).trim()
            )
            else -> Unit
        }
    }

    if (pack.size() > 0) {
        return pack.toByteArray()
    }

    if (consumed < response.size) {
        val trailing = response.copyOfRange(consumed, response.size)
        if (startsWithBytes(trailing, "PACK".toByteArray())) {
            return trailing
        }
    }

    return null
}

private fun buildPushRequest(
    updates: List<PushUpdate>,
    pack: ByteArray,
    capabilities: ServerCapabilities,
): ByteArray {
    val requestedCaps = mutableListOf("report-status")
    if (capabilities.has("ofs-delta")) {
        requestedCaps.add("ofs-delta")
    }

    val out = ByteArrayOutputStream()
    for ((index, update) in updates.withIndex()) {
        val line = if (index == 0) {
            "${update.dstOid.hex} ${update.srcOid.hex} ${update.dstRef}\u0000${requestedCaps.joinToString(" ")}\n"
        } else {
            "${update.dstOid.hex} ${update.srcOid.hex} ${update.dstRef}\n"
        }
        out.write(pktLineEncode(line.toByteArray(Charsets.UTF_8)))
    }
    out.write(pktLineFlush())
    out.write(pack)
    return out.toByteArray()
}

private fun parsePushResponse(response: ByteArray): String {
    val (lines, consumed) = pktLineDecode(response)
    val text = StringBuilder()

    for (line in lines) {
        if (line !is PktLine.Data) {
            continue
        }

        val payload = when (line.bytes.firstOrNull()?.toInt()) {
            1, 2 -> line.bytes.copyOfRange(1, line.bytes.size)
            3 -> throw MuonGitException.InvalidObject(
                String(line.bytes.copyOfRange(1, line.bytes.size), Charsets.UTF_8).trim()
            )
            else -> line.bytes
        }
        text.append(String(payload, Charsets.UTF_8))
    }

    if (consumed < response.size) {
        text.append(String(response.copyOfRange(consumed, response.size), Charsets.UTF_8))
    }

    for (line in text.lines()) {
        if (line.startsWith("unpack ") && line != "unpack ok") {
            throw MuonGitException.InvalidObject(line)
        }
        if (line.startsWith("ng ")) {
            throw MuonGitException.InvalidObject(line.removePrefix("ng "))
        }
    }

    return text.toString()
}

private fun resolveCloneHead(fetch: FetchResult, branch: String?): Pair<String, OID> {
    if (branch != null) {
        val refName = "refs/heads/$branch"
        val oid = fetch.advertisedRefs.firstOrNull { it.name == refName }?.oid
            ?: throw MuonGitException.NotFound("remote branch '$branch' not found")
        return Pair(branch, oid)
    }

    defaultBranchFromCaps(fetch.capabilities)?.let { defaultBranch ->
        val refName = "refs/heads/$defaultBranch"
        fetch.advertisedRefs.firstOrNull { it.name == refName }?.oid?.let { oid ->
            return Pair(defaultBranch, oid)
        }
    }

    val headOid = fetch.advertisedRefs.firstOrNull { it.name == "HEAD" }?.oid
    if (headOid != null) {
        fetch.advertisedRefs.firstOrNull {
            it.name.startsWith("refs/heads/") && it.oid == headOid
        }?.let { branchRef ->
            return Pair(branchRef.name.removePrefix("refs/heads/"), headOid)
        }
    }

    for (candidate in listOf("main", "master")) {
        val refName = "refs/heads/$candidate"
        fetch.advertisedRefs.firstOrNull { it.name == refName }?.oid?.let { oid ->
            return Pair(candidate, oid)
        }
    }

    throw MuonGitException.NotFound("could not determine remote default branch")
}

private fun defaultPushRefspecs(gitDir: File): List<String> {
    val head = readReference(gitDir, "HEAD")
    if (!head.startsWith("ref: ")) {
        throw MuonGitException.InvalidSpec("HEAD is detached; provide push refspecs")
    }
    val target = head.removePrefix("ref: ").trim()
    return listOf("$target:$target")
}

private fun isFastForward(gitDir: File, oldOID: OID, newOID: OID): Boolean {
    if (oldOID.isZero) {
        return true
    }
    return mergeBase(gitDir, oldOID, newOID) == oldOID
}

private fun updateTrackingRefsAfterPush(
    gitDir: File,
    remoteName: String,
    updates: List<PushUpdate>,
): Int {
    var updated = 0
    for (update in updates) {
        if (!update.dstRef.startsWith("refs/heads/")) {
            continue
        }
        val branch = update.dstRef.removePrefix("refs/heads/")
        writeReference(gitDir, "refs/remotes/$remoteName/$branch", update.srcOid)
        updated++
    }
    return updated
}

private fun startsWithBytes(bytes: ByteArray, prefix: ByteArray): Boolean {
    if (bytes.size < prefix.size) {
        return false
    }
    for (index in prefix.indices) {
        if (bytes[index] != prefix[index]) {
            return false
        }
    }
    return true
}
