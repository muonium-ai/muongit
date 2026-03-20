package ai.muonium.muongit

import java.io.ByteArrayOutputStream
import java.io.File
import java.util.TreeMap
import java.util.TreeSet
import kotlin.system.exitProcess

private data class Checkpoint(val name: String, val repo: String)

private data class Snapshot(
    val repoKind: String,
    val head: String,
    val headOid: String,
    val refs: List<String>,
    val localBranches: List<String>,
    val remoteBranches: List<String>,
    val remotes: List<String>,
    val revisions: List<String>,
    val walks: List<String>,
    val headCommit: String,
    val treeEntries: List<String>,
    val worktreeFiles: List<String>,
    val indexEntries: List<String>,
    val status: List<String>,
    val helloPatch: String,
)

private data class SnapshotEntry(val oid: OID, val mode: Int)

fun main(args: Array<String>) {
    try {
        when (args.firstOrNull()) {
            "write-scenario" -> {
                require(args.size == 3) {
                    "usage: muongit-conformance write-scenario <root> <fixture-script>"
                }
                printManifest(writeScenario(File(args[1]), File(args[2])))
            }
            "snapshot" -> {
                require(args.size == 2) {
                    "usage: muongit-conformance snapshot <repo>"
                }
                printSnapshot(snapshotRepository(File(args[1])))
            }
            null -> error("usage: muongit-conformance <write-scenario|snapshot> ...")
            else -> error("unknown command: ${args[0]}")
        }
    } catch (error: Throwable) {
        System.err.println(error.message ?: error.toString())
        exitProcess(1)
    }
}

private fun writeScenario(root: File, fixtureScript: File): List<Checkpoint> {
    root.deleteRecursively()
    root.mkdirs()

    val checkpointsRoot = root.resolve("checkpoints").also { it.mkdirs() }
    val baseRepoPath = root.resolve("workspace")
    val repo = Repository.init(baseRepoPath.path, bare = false)
    val workdir = repo.workdir ?: error("expected workdir repository")

    writeText(workdir.resolve("hello.txt"), "hello base\n")
    writeText(workdir.resolve("docs/guide.txt"), "guide v1\n")
    writeText(workdir.resolve("remove-me.txt"), "remove me\n")
    repo.add(listOf("hello.txt", "docs/guide.txt", "remove-me.txt"))
    repo.commit("initial", commitOptions(1))

    repo.createBranch("feature")
    repo.switchBranch("feature")

    writeText(workdir.resolve("hello.txt"), "hello feature\n")
    writeText(workdir.resolve("notes/ideas.txt"), "ideas v1\n")
    repo.remove(listOf("remove-me.txt"))
    repo.add(listOf("hello.txt", "notes/ideas.txt"))
    repo.commit("feature-work", commitOptions(2))

    val oldHello = workdir.resolve("hello.txt").readText()
    val patch = Patch.fromText(
        oldPath = "hello.txt",
        newPath = "hello.txt",
        oldText = oldHello,
        newText = "hello patched\nfeature line\n",
        context = 3,
    )
    repo.applyPatch(patch)
    repo.add(listOf("hello.txt"))
    repo.commit("patch-apply", commitOptions(3))

    val featureClean = checkpointsRoot.resolve("feature-clean")
    copyTree(baseRepoPath, featureClean)

    val detachedCheckout = checkpointsRoot.resolve("detached-checkout")
    copyTree(featureClean, detachedCheckout)
    val detachedRepo = Repository.open(detachedCheckout.path)
    detachedRepo.checkoutRevision("HEAD~1")
    detachedRepo.createBranch("detached-copy")

    val restoreDirty = checkpointsRoot.resolve("restore-dirty")
    copyTree(featureClean, restoreDirty)
    val restoreRepo = Repository.open(restoreDirty.path)
    val restoreWorkdir = restoreRepo.workdir ?: error("expected restore workdir")
    writeText(restoreWorkdir.resolve("hello.txt"), "hello dirty\n")
    writeText(restoreWorkdir.resolve("staged-only.txt"), "staged only\n")
    restoreRepo.add(listOf("hello.txt", "staged-only.txt"))
    restoreRepo.restore(listOf("hello.txt"), RestoreOptions(staged = true, worktree = true))
    writeText(restoreWorkdir.resolve("scratch.txt"), "scratch\n")

    val resetHard = checkpointsRoot.resolve("reset-hard")
    copyTree(featureClean, resetHard)
    val resetRepo = Repository.open(resetHard.path)
    resetRepo.reset("HEAD~1", ResetMode.HARD)

    val remoteRoot = checkpointsRoot.resolve("remote-scenario").also { it.mkdirs() }
    val fixture = GitFixture(remoteRoot)
    val fixtureProcess = FixtureProcess.http(fixtureScript, fixture.remoteGitDir, "alice", "s3cret")
    val transport = TransportOptions(auth = RemoteAuth.Basic("alice", "s3cret"))
    try {
        val remoteClone = checkpointsRoot.resolve("remote-clone")
        val remoteRepo = Repository.clone(
            fixtureProcess.url,
            remoteClone.path,
            CloneOptions(transport = transport),
        )
        fixture.commitAndPush("hello.txt", "hello remote\n", "remote update")
        remoteRepo.fetch("origin", FetchOptions(transport = transport))
        remoteRepo.reset("refs/remotes/origin/main", ResetMode.HARD)
        val remoteWorkdir = remoteRepo.workdir ?: error("expected clone workdir")
        writeText(remoteWorkdir.resolve("local.txt"), "local push\n")
        remoteRepo.add(listOf("local.txt"))
        remoteRepo.commit("local push", commitOptions(4))
        remoteRepo.push("origin", PushOptions(transport = transport))

        return listOf(
            Checkpoint("feature-clean", featureClean.path),
            Checkpoint("detached-checkout", detachedCheckout.path),
            Checkpoint("restore-dirty", restoreDirty.path),
            Checkpoint("reset-hard", resetHard.path),
            Checkpoint("remote-clone", remoteClone.path),
            Checkpoint("remote-bare", fixture.remoteGitDir.path),
        )
    } finally {
        fixtureProcess.stop()
    }
}

private fun snapshotRepository(path: File): Snapshot {
    val repo = Repository.open(path.path)
    val gitDir = repo.gitDir

    return Snapshot(
        repoKind = if (repo.isBare) "bare" else "worktree",
        head = runCatching { repo.head() }.getOrElse { "" },
        headOid = runCatching { repo.refdb().resolve("HEAD").hex }.getOrElse { "" },
        refs = snapshotRefs(repo),
        localBranches = snapshotBranches(repo, BranchType.LOCAL),
        remoteBranches = snapshotBranches(repo, BranchType.REMOTE),
        remotes = snapshotRemotes(gitDir),
        revisions = snapshotRevisions(gitDir),
        walks = snapshotWalks(gitDir),
        headCommit = snapshotHeadCommit(gitDir),
        treeEntries = snapshotTreeEntries(gitDir),
        worktreeFiles = snapshotWorktreeFiles(repo.workdir),
        indexEntries = snapshotIndexEntries(gitDir),
        status = snapshotStatus(gitDir, repo.workdir),
        helloPatch = snapshotHelloPatch(gitDir),
    )
}

private fun snapshotRefs(repo: Repository): List<String> =
    repo.refdb().list().map { "${it.name}|${it.value}" }.sorted()

private fun snapshotBranches(repo: Repository, kind: BranchType): List<String> =
    repo.listBranches(kind).map { branch ->
        val target = branch.target?.hex ?: ""
        val upstream = branch.upstream?.let { "${it.remoteName}/${it.mergeRef}" } ?: ""
        "${branch.name}|$target|${if (branch.isHead) "head" else ""}|$upstream"
    }.sorted()

private fun snapshotRemotes(gitDir: File): List<String> =
    runCatching { listRemotes(gitDir) }.getOrElse { emptyList() }
        .mapNotNull { name ->
            runCatching { getRemote(gitDir, name) }.getOrNull()?.let { "${it.name}|${it.url}" }
        }
        .sorted()

private fun snapshotRevisions(gitDir: File): List<String> =
    listOf("HEAD", "HEAD~1", "main", "feature", "detached-copy", "refs/remotes/origin/main").map { spec ->
        val value = runCatching { resolveRevision(gitDir, spec).hex }.getOrElse { "!" }
        "$spec|$value"
    }

private fun snapshotWalks(gitDir: File): List<String> {
    val head = snapshotWalk(gitDir) { walk -> walk.pushHead() }
    val firstParent = snapshotWalk(gitDir) { walk ->
        walk.pushHead()
        walk.simplifyFirstParent()
    }
    val topoTime = snapshotWalk(gitDir) { walk ->
        walk.pushHead()
        walk.sorting(Revwalk.SORT_TOPOLOGICAL or Revwalk.SORT_TIME)
    }
    val mainToFeature = snapshotWalk(gitDir) { walk -> walk.pushRange("main..feature") }
    val symmetric = snapshotWalk(gitDir) { walk -> walk.pushRange("main...feature") }
    return listOf(
        "HEAD|$head",
        "HEAD:first-parent|$firstParent",
        "HEAD:topo-time|$topoTime",
        "main..feature|$mainToFeature",
        "main...feature|$symmetric",
    )
}

private fun snapshotWalk(gitDir: File, configure: (Revwalk) -> Unit): String =
    runCatching {
        Revwalk(gitDir).also(configure).allOids().joinToString(",") { it.hex }
    }.getOrElse { "!" }

private fun snapshotHeadCommit(gitDir: File): String {
    val head = runCatching { resolveRevision(gitDir, "HEAD") }.getOrNull() ?: return ""
    val commit = readObject(gitDir, head).asCommit()
    val parents = commit.parentIds.joinToString(",") { it.hex }
    return "${commit.oid.hex}|${commit.treeId.hex}|$parents|${hex(commit.message.toByteArray())}"
}

private fun snapshotTreeEntries(gitDir: File): List<String> {
    val head = runCatching { resolveRevision(gitDir, "HEAD") }.getOrNull() ?: return emptyList()
    val commit = readObject(gitDir, head).asCommit()
    val entries = mutableListOf<String>()
    collectTreeEntries(gitDir, commit.treeId, "", entries)
    return entries.sorted()
}

private fun collectTreeEntries(gitDir: File, treeOid: OID, prefix: String, entries: MutableList<String>) {
    val tree = readObject(gitDir, treeOid).asTree()
    for (entry in tree.entries) {
        val path = if (prefix.isEmpty()) entry.name else "$prefix/${entry.name}"
        if (entry.isTree) {
            collectTreeEntries(gitDir, entry.oid, path, entries)
        } else {
            val blob = readObject(gitDir, entry.oid).asBlob()
            entries += "${entry.mode.toString(8)}|$path|${entry.oid.hex}|${hex(blob.data)}"
        }
    }
}

private fun snapshotWorktreeFiles(workdir: File?): List<String> {
    if (workdir == null) {
        return emptyList()
    }
    val files = mutableListOf<String>()
    collectWorktreeFiles(workdir, workdir, files)
    return files.sorted()
}

private fun collectWorktreeFiles(root: File, dir: File, files: MutableList<String>) {
    val children = dir.listFiles()?.sortedBy { it.name } ?: return
    for (child in children) {
        if (child.name == ".git") {
            continue
        }
        if (child.isDirectory) {
            collectWorktreeFiles(root, child, files)
        } else {
            val relative = child.relativeTo(root).invariantSeparatorsPath
            files += "$relative|${hex(child.readBytes())}"
        }
    }
}

private fun snapshotIndexEntries(gitDir: File): List<String> =
    readIndex(gitDir).entries
        .map { "${it.mode.toString(8)}|${it.path}|${it.oid.hex}" }
        .sorted()

private fun snapshotStatus(gitDir: File, workdir: File?): List<String> {
    if (workdir == null) {
        return emptyList()
    }

    val headEntries = headIndexEntries(gitDir)
    val index = readIndex(gitDir)
    val staged = TreeMap<String, Char>()
    val paths = TreeSet<String>()

    for ((path, headEntry) in headEntries) {
        paths += path
        val indexEntry = index.find(path)
        when {
            indexEntry == null -> staged[path] = 'D'
            indexEntry.oid != headEntry.oid || indexEntry.mode != headEntry.mode -> staged[path] = 'M'
        }
    }
    for (entry in index.entries) {
        paths += entry.path
        if (!headEntries.containsKey(entry.path)) {
            staged[entry.path] = 'A'
        }
    }

    val unstaged = TreeMap<String, Char>()
    for (entry in workdirStatus(gitDir, workdir)) {
        paths += entry.path
        unstaged[entry.path] = when (entry.status) {
            FileStatus.DELETED -> 'D'
            FileStatus.NEW -> '?'
            FileStatus.MODIFIED -> 'M'
        }
    }

    return paths.mapNotNull { path ->
        val stagedCode = staged[path] ?: ' '
        val unstagedCode = unstaged[path] ?: ' '
        val code = if (stagedCode == ' ' && unstagedCode == '?') "??" else "$stagedCode$unstagedCode"
        if (code.isBlank()) null else "$code|$path"
    }
}

private fun snapshotHelloPatch(gitDir: File): String {
    val head = runCatching { resolveRevision(gitDir, "HEAD") }.getOrNull() ?: return ""
    val previous = runCatching { resolveRevision(gitDir, "HEAD~1") }.getOrNull() ?: return ""
    val oldText = treeBlobText(gitDir, previous, "hello.txt")
    val newText = treeBlobText(gitDir, head, "hello.txt")
    if (oldText == newText) {
        return ""
    }
    return hex(
        Patch.fromText(
            oldPath = "hello.txt",
            newPath = "hello.txt",
            oldText = oldText,
            newText = newText,
            context = 3,
        ).format().toByteArray()
    )
}

private fun treeBlobText(gitDir: File, commitOid: OID, path: String): String {
    val commit = readObject(gitDir, commitOid).asCommit()
    val treeMap = materializeTreeMap(gitDir, commit.treeId, "")
    val blobOid = treeMap[path] ?: throw MuonGitException.NotFound(path)
    return readObject(gitDir, blobOid).asBlob().data.decodeToString()
}

private fun materializeTreeMap(gitDir: File, treeOid: OID, prefix: String): Map<String, OID> {
    val map = linkedMapOf<String, OID>()
    val tree = readObject(gitDir, treeOid).asTree()
    for (entry in tree.entries) {
        val path = if (prefix.isEmpty()) entry.name else "$prefix/${entry.name}"
        if (entry.isTree) {
            map.putAll(materializeTreeMap(gitDir, entry.oid, path))
        } else {
            map[path] = entry.oid
        }
    }
    return map
}

private fun headIndexEntries(gitDir: File): Map<String, SnapshotEntry> {
    val head = runCatching { resolveRevision(gitDir, "HEAD") }.getOrNull() ?: return emptyMap()
    val commit = readObject(gitDir, head).asCommit()
    return materializeHeadEntries(gitDir, commit.treeId, "")
}

private fun materializeHeadEntries(gitDir: File, treeOid: OID, prefix: String): Map<String, SnapshotEntry> {
    val entries = linkedMapOf<String, SnapshotEntry>()
    val tree = readObject(gitDir, treeOid).asTree()
    for (entry in tree.entries) {
        val path = if (prefix.isEmpty()) entry.name else "$prefix/${entry.name}"
        if (entry.isTree) {
            entries.putAll(materializeHeadEntries(gitDir, entry.oid, path))
        } else {
            entries[path] = SnapshotEntry(entry.oid, entry.mode)
        }
    }
    return entries
}

private fun commitOptions(time: Long): CommitOptions {
    val signature = Signature(
        name = "Muon Conformance",
        email = "conformance@muon.ai",
        time = time,
        offset = 0,
    )
    return CommitOptions(author = signature, committer = signature)
}

private class GitFixture(root: File) {
    val remoteGitDir = root.resolve("remote.git")
    private val seedWorkdir = root.resolve("seed")

    init {
        runCommand(listOf("/usr/bin/git", "init", "--bare", remoteGitDir.path), root)
        runCommand(listOf("/usr/bin/git", "init", seedWorkdir.path), root)
        runCommand(listOf("/usr/bin/git", "config", "user.name", "MuonGit Fixture"), seedWorkdir)
        runCommand(listOf("/usr/bin/git", "config", "user.email", "fixture@muon.ai"), seedWorkdir)
        writeText(seedWorkdir.resolve("hello.txt"), "hello\n")
        runCommand(listOf("/usr/bin/git", "add", "hello.txt"), seedWorkdir)
        runCommand(listOf("/usr/bin/git", "commit", "-m", "initial"), seedWorkdir)
        runCommand(listOf("/usr/bin/git", "branch", "-M", "main"), seedWorkdir)
        runCommand(listOf("/usr/bin/git", "remote", "add", "origin", remoteGitDir.path), seedWorkdir)
        runCommand(listOf("/usr/bin/git", "push", "origin", "main"), seedWorkdir)
        runCommand(
            listOf("/usr/bin/git", "--git-dir", remoteGitDir.path, "symbolic-ref", "HEAD", "refs/heads/main"),
            root,
        )
    }

    fun commitAndPush(fileName: String, contents: String, message: String) {
        writeText(seedWorkdir.resolve(fileName), contents)
        runCommand(listOf("/usr/bin/git", "add", fileName), seedWorkdir)
        runCommand(listOf("/usr/bin/git", "commit", "-m", message), seedWorkdir)
        runCommand(listOf("/usr/bin/git", "push", "origin", "main"), seedWorkdir)
        runCommand(
            listOf("/usr/bin/git", "--git-dir", remoteGitDir.path, "symbolic-ref", "HEAD", "refs/heads/main"),
            remoteGitDir.parentFile ?: File("."),
        )
    }
}

private class FixtureProcess private constructor(
    private val process: Process,
    val url: String,
) {
    companion object {
        fun http(fixtureScript: File, repo: File, username: String, secret: String): FixtureProcess {
            val process = ProcessBuilder(
                "/usr/bin/python3",
                fixtureScript.path,
                "serve-http",
                "--repo",
                repo.path,
                "--auth",
                "basic",
                "--username",
                username,
                "--secret",
                secret,
            )
                .redirectError(ProcessBuilder.Redirect.INHERIT)
                .start()

            val line = process.inputStream.bufferedReader().readLine()
                ?: error("fixture produced no startup line")
            val url = "\"url\"\\s*:\\s*\"([^\"]+)\"".toRegex().find(line)?.groupValues?.get(1)
                ?: error("unexpected fixture output: $line")
            return FixtureProcess(process, url)
        }
    }

    fun stop() {
        process.destroy()
        process.waitFor()
    }
}

private fun writeText(path: File, content: String) {
    path.parentFile?.mkdirs()
    path.writeText(content)
}

private fun copyTree(source: File, destination: File) {
    destination.deleteRecursively()
    source.copyRecursively(destination, overwrite = true)
}

private fun runCommand(command: List<String>, currentDirectory: File, input: ByteArray? = null): ByteArray {
    val process = ProcessBuilder(command)
        .directory(currentDirectory)
        .redirectErrorStream(false)
        .start()

    if (input != null) {
        process.outputStream.use { it.write(input) }
    } else {
        process.outputStream.close()
    }

    val stdout = process.inputStream.readBytes()
    val stderr = process.errorStream.readBytes()
    if (process.waitFor() != 0) {
        val message = (if (stderr.isNotEmpty()) stderr else stdout).decodeToString().trim()
        error(if (message.isEmpty()) "command failed: ${command.joinToString(" ")}" else message)
    }
    return stdout
}

private fun printManifest(checkpoints: List<Checkpoint>) {
    val body = checkpoints.joinToString(",") { checkpoint ->
        "{\"name\":\"${escapeJson(checkpoint.name)}\",\"repo\":\"${escapeJson(checkpoint.repo)}\"}"
    }
    println("{\"checkpoints\":[${body}]}")
}

private fun printSnapshot(snapshot: Snapshot) {
    println(
        buildString {
            append("{")
            appendJsonField("repo_kind", snapshot.repoKind, true)
            appendJsonField("head", snapshot.head, true)
            appendJsonField("head_oid", snapshot.headOid, true)
            appendJsonArray("refs", snapshot.refs, true)
            appendJsonArray("local_branches", snapshot.localBranches, true)
            appendJsonArray("remote_branches", snapshot.remoteBranches, true)
            appendJsonArray("remotes", snapshot.remotes, true)
            appendJsonArray("revisions", snapshot.revisions, true)
            appendJsonArray("walks", snapshot.walks, true)
            appendJsonField("head_commit", snapshot.headCommit, true)
            appendJsonArray("tree_entries", snapshot.treeEntries, true)
            appendJsonArray("worktree_files", snapshot.worktreeFiles, true)
            appendJsonArray("index_entries", snapshot.indexEntries, true)
            appendJsonArray("status", snapshot.status, true)
            appendJsonField("hello_patch", snapshot.helloPatch, false)
            append("}")
        }
    )
}

private fun StringBuilder.appendJsonField(name: String, value: String, trailingComma: Boolean) {
    append("\"")
    append(name)
    append("\":\"")
    append(escapeJson(value))
    append("\"")
    if (trailingComma) {
        append(",")
    }
}

private fun StringBuilder.appendJsonArray(name: String, values: List<String>, trailingComma: Boolean) {
    append("\"")
    append(name)
    append("\":[")
    values.forEachIndexed { index, value ->
        if (index > 0) {
            append(",")
        }
        append("\"")
        append(escapeJson(value))
        append("\"")
    }
    append("]")
    if (trailingComma) {
        append(",")
    }
}

private fun escapeJson(value: String): String =
    value
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")

private fun hex(bytes: ByteArray): String = buildString(bytes.size * 2) {
    for (byte in bytes) {
        append("%02x".format(byte))
    }
}
