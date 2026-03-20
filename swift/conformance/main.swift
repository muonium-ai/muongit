import Foundation
import MuonGit

private struct Checkpoint: Codable {
    let name: String
    let repo: String
}

private struct Manifest: Codable {
    let checkpoints: [Checkpoint]
}

private struct Snapshot: Codable {
    let repoKind: String
    let head: String
    let headOid: String
    let refs: [String]
    let localBranches: [String]
    let remoteBranches: [String]
    let remotes: [String]
    let revisions: [String]
    let walks: [String]
    let headCommit: String
    let treeEntries: [String]
    let worktreeFiles: [String]
    let indexEntries: [String]
    let status: [String]
    let helloPatch: String
}

private struct MaterializedEntry {
    let oid: OID
    let mode: UInt32
}

private enum ConformanceError: Error, CustomStringConvertible {
    case usage(String)
    case invalid(String)

    var description: String {
        switch self {
        case let .usage(message), let .invalid(message):
            return message
        }
    }
}

@main
private enum MuonGitConformanceMain {
    static func main() throws {
        do {
            let args = Array(CommandLine.arguments.dropFirst())
            guard let command = args.first else {
                throw ConformanceError.usage("usage: muongit-conformance <write-scenario|snapshot> ...")
            }

            switch command {
            case "write-scenario":
                guard args.count == 3 else {
                    throw ConformanceError.usage("usage: muongit-conformance write-scenario <root> <fixture-script>")
                }
                try emitJSON(Manifest(checkpoints: try writeScenario(root: args[1], fixtureScript: args[2])))
            case "snapshot":
                guard args.count == 2 else {
                    throw ConformanceError.usage("usage: muongit-conformance snapshot <repo>")
                }
                try emitJSON(snapshotRepository(path: args[1]))
            default:
                throw ConformanceError.invalid("unknown command: \(command)")
            }
        } catch {
            fputs("\(error)\n", stderr)
            Foundation.exit(1)
        }
    }
}

private func writeScenario(root: String, fixtureScript: String) throws -> [Checkpoint] {
    try removeIfExists(root)
    try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)

    let checkpointsRoot = pathJoin(root, "checkpoints")
    try FileManager.default.createDirectory(atPath: checkpointsRoot, withIntermediateDirectories: true)

    let baseRepoPath = pathJoin(root, "workspace")
    let repo = try Repository.create(at: baseRepoPath, bare: false)
    guard let workdir = repo.workdir else {
        throw ConformanceError.invalid("expected workdir repository")
    }

    try writeText(pathJoin(workdir, "hello.txt"), "hello base\n")
    try writeText(pathJoin(workdir, "docs/guide.txt"), "guide v1\n")
    try writeText(pathJoin(workdir, "remove-me.txt"), "remove me\n")
    _ = try repo.add(paths: ["hello.txt", "docs/guide.txt", "remove-me.txt"])
    _ = try repo.commit(message: "initial", options: commitOptions(time: 1))

    _ = try repo.createBranch(name: "feature")
    _ = try repo.switchBranch(name: "feature")

    try writeText(pathJoin(workdir, "hello.txt"), "hello feature\n")
    try writeText(pathJoin(workdir, "notes/ideas.txt"), "ideas v1\n")
    _ = try repo.remove(paths: ["remove-me.txt"])
    _ = try repo.add(paths: ["hello.txt", "notes/ideas.txt"])
    _ = try repo.commit(message: "feature-work", options: commitOptions(time: 2))

    let oldHello = try String(contentsOfFile: pathJoin(workdir, "hello.txt"), encoding: .utf8)
    let patch = Patch.fromText(
        oldPath: "hello.txt",
        newPath: "hello.txt",
        oldText: oldHello,
        newText: "hello patched\nfeature line\n",
        context: 3
    )
    _ = try repo.applyPatch(patch)
    _ = try repo.add(paths: ["hello.txt"])
    _ = try repo.commit(message: "patch-apply", options: commitOptions(time: 3))

    let featureClean = pathJoin(checkpointsRoot, "feature-clean")
    try copyTree(from: baseRepoPath, to: featureClean)

    let detachedCheckout = pathJoin(checkpointsRoot, "detached-checkout")
    try copyTree(from: featureClean, to: detachedCheckout)
    let detachedRepo = try Repository.open(at: detachedCheckout)
    _ = try detachedRepo.checkoutRevision(spec: "HEAD~1")
    _ = try detachedRepo.createBranch(name: "detached-copy")

    let restoreDirty = pathJoin(checkpointsRoot, "restore-dirty")
    try copyTree(from: featureClean, to: restoreDirty)
    let restoreRepo = try Repository.open(at: restoreDirty)
    guard let restoreWorkdir = restoreRepo.workdir else {
        throw ConformanceError.invalid("expected restore workdir")
    }
    try writeText(pathJoin(restoreWorkdir, "hello.txt"), "hello dirty\n")
    try writeText(pathJoin(restoreWorkdir, "staged-only.txt"), "staged only\n")
    _ = try restoreRepo.add(paths: ["hello.txt", "staged-only.txt"])
    _ = try restoreRepo.restore(
        paths: ["hello.txt"],
        options: RestoreOptions(source: nil, staged: true, worktree: true)
    )
    try writeText(pathJoin(restoreWorkdir, "scratch.txt"), "scratch\n")

    let resetHard = pathJoin(checkpointsRoot, "reset-hard")
    try copyTree(from: featureClean, to: resetHard)
    let resetRepo = try Repository.open(at: resetHard)
    _ = try resetRepo.reset(spec: "HEAD~1", mode: .hard)

    let remoteRoot = pathJoin(checkpointsRoot, "remote-scenario")
    try FileManager.default.createDirectory(atPath: remoteRoot, withIntermediateDirectories: true)
    let fixture = try GitFixture(root: remoteRoot)
    let fixtureProcess = try FixtureProcess.http(
        fixtureScript: fixtureScript,
        repo: fixture.remoteGitDir,
        username: "alice",
        secret: "s3cret"
    )
    defer { fixtureProcess.stop() }

    let transport = TransportOptions(auth: .basic(username: "alice", password: "s3cret"))
    let remoteClone = pathJoin(checkpointsRoot, "remote-clone")
    let remoteRepo = try Repository.clone(
        from: fixtureProcess.url,
        to: remoteClone,
        options: CloneOptions(transport: transport)
    )
    try fixture.commitAndPush(fileName: "hello.txt", contents: "hello remote\n", message: "remote update")
    _ = try remoteRepo.fetch(remoteName: "origin", options: FetchOptions(transport: transport))
    _ = try remoteRepo.reset(spec: "refs/remotes/origin/main", mode: .hard)
    guard let remoteWorkdir = remoteRepo.workdir else {
        throw ConformanceError.invalid("expected clone workdir")
    }
    try writeText(pathJoin(remoteWorkdir, "local.txt"), "local push\n")
    _ = try remoteRepo.add(paths: ["local.txt"])
    _ = try remoteRepo.commit(message: "local push", options: commitOptions(time: 4))
    _ = try remoteRepo.push(remoteName: "origin", options: PushOptions(transport: transport))

    return [
        Checkpoint(name: "feature-clean", repo: featureClean),
        Checkpoint(name: "detached-checkout", repo: detachedCheckout),
        Checkpoint(name: "restore-dirty", repo: restoreDirty),
        Checkpoint(name: "reset-hard", repo: resetHard),
        Checkpoint(name: "remote-clone", repo: remoteClone),
        Checkpoint(name: "remote-bare", repo: fixture.remoteGitDir),
    ]
}

private func snapshotRepository(path: String) throws -> Snapshot {
    let repo = try Repository.open(at: path)
    let gitDir = repo.gitDir

    return Snapshot(
        repoKind: repo.isBare ? "bare" : "worktree",
        head: (try? repo.head()) ?? "",
        headOid: (try? repo.refdb.resolve(name: "HEAD").hex) ?? "",
        refs: try snapshotRefs(repo: repo),
        localBranches: try snapshotBranches(repo: repo, kind: .local),
        remoteBranches: try snapshotBranches(repo: repo, kind: .remote),
        remotes: snapshotRemotes(gitDir: gitDir),
        revisions: snapshotRevisions(gitDir: gitDir),
        walks: snapshotWalks(gitDir: gitDir),
        headCommit: try snapshotHeadCommit(gitDir: gitDir),
        treeEntries: try snapshotTreeEntries(gitDir: gitDir),
        worktreeFiles: try snapshotWorktreeFiles(workdir: repo.workdir),
        indexEntries: try snapshotIndexEntries(gitDir: gitDir),
        status: try snapshotStatus(gitDir: gitDir, workdir: repo.workdir),
        helloPatch: try snapshotHelloPatch(gitDir: gitDir)
    )
}

private func snapshotRefs(repo: Repository) throws -> [String] {
    try repo.refdb.list()
        .map { "\($0.name)|\($0.value)" }
        .sorted()
}

private func snapshotBranches(repo: Repository, kind: BranchType) throws -> [String] {
    try repo.listBranches(kind: kind)
        .map { branch in
            let target = branch.target?.hex ?? ""
            let upstream = branch.upstream.map { "\($0.remoteName)/\($0.mergeRef)" } ?? ""
            return "\(branch.name)|\(target)|\(branch.isHEAD ? "head" : "")|\(upstream)"
        }
        .sorted()
}

private func snapshotRemotes(gitDir: String) -> [String] {
    let names = (try? listRemotes(gitDir: gitDir)) ?? []
    return names.compactMap { name in
        guard let remote = try? getRemote(gitDir: gitDir, name: name) else {
            return nil
        }
        return "\(remote.name)|\(remote.url)"
    }.sorted()
}

private func snapshotRevisions(gitDir: String) -> [String] {
    ["HEAD", "HEAD~1", "main", "feature", "detached-copy", "refs/remotes/origin/main"].map { spec in
        let value = (try? resolveRevision(gitDir: gitDir, spec: spec).hex) ?? "!"
        return "\(spec)|\(value)"
    }
}

private func snapshotWalks(gitDir: String) -> [String] {
    let head = snapshotWalk(gitDir: gitDir) { walk in
        try walk.pushHead()
    }
    let firstParent = snapshotWalk(gitDir: gitDir) { walk in
        try walk.pushHead()
        walk.simplifyFirstParent()
    }
    let topoTime = snapshotWalk(gitDir: gitDir) { walk in
        try walk.pushHead()
        walk.sorting([.topological, .time])
    }
    let mainToFeature = snapshotWalk(gitDir: gitDir) { walk in
        try walk.pushRange("main..feature")
    }
    let symmetric = snapshotWalk(gitDir: gitDir) { walk in
        try walk.pushRange("main...feature")
    }
    return [
        "HEAD|\(head)",
        "HEAD:first-parent|\(firstParent)",
        "HEAD:topo-time|\(topoTime)",
        "main..feature|\(mainToFeature)",
        "main...feature|\(symmetric)",
    ]
}

private func snapshotWalk(gitDir: String, configure: (Revwalk) throws -> Void) -> String {
    let walk = Revwalk(gitDir: gitDir)
    do {
        try configure(walk)
        return joinOids(try walk.allOids())
    } catch {
        return "!"
    }
}

private func snapshotHeadCommit(gitDir: String) throws -> String {
    guard let head = try? resolveRevision(gitDir: gitDir, spec: "HEAD") else {
        return ""
    }
    let commit = try readObject(gitDir: gitDir, oid: head).asCommit()
    let parents = commit.parentIds.map(\.hex).joined(separator: ",")
    return "\(commit.oid.hex)|\(commit.treeId.hex)|\(parents)|\(hex(Data(commit.message.utf8)))"
}

private func snapshotTreeEntries(gitDir: String) throws -> [String] {
    guard let head = try? resolveRevision(gitDir: gitDir, spec: "HEAD") else {
        return []
    }
    let commit = try readObject(gitDir: gitDir, oid: head).asCommit()
    var entries: [String] = []
    try collectTreeEntries(gitDir: gitDir, treeOID: commit.treeId, prefix: "", entries: &entries)
    return entries.sorted()
}

private func collectTreeEntries(gitDir: String, treeOID: OID, prefix: String, entries: inout [String]) throws {
    let tree = try readObject(gitDir: gitDir, oid: treeOID).asTree()
    for entry in tree.entries {
        let path = prefix.isEmpty ? entry.name : "\(prefix)/\(entry.name)"
        if entry.isTree {
            try collectTreeEntries(gitDir: gitDir, treeOID: entry.oid, prefix: path, entries: &entries)
        } else {
            let blob = try readObject(gitDir: gitDir, oid: entry.oid).asBlob()
            entries.append(String(format: "%o", entry.mode) + "|\(path)|\(entry.oid.hex)|\(hex(blob.data))")
        }
    }
}

private func snapshotWorktreeFiles(workdir: String?) throws -> [String] {
    guard let workdir else {
        return []
    }
    var files: [String] = []
    try collectWorktreeFiles(root: workdir, dir: workdir, files: &files)
    return files.sorted()
}

private func collectWorktreeFiles(root: String, dir: String, files: inout [String]) throws {
    for child in try FileManager.default.contentsOfDirectory(atPath: dir).sorted() {
        if child == ".git" {
            continue
        }
        let fullPath = pathJoin(dir, child)
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: fullPath, isDirectory: &isDir) else {
            continue
        }
        if isDir.boolValue {
            try collectWorktreeFiles(root: root, dir: fullPath, files: &files)
        } else {
            let relative = relativePath(fullPath, from: root)
            files.append("\(relative)|\(hex(try Data(contentsOf: URL(fileURLWithPath: fullPath))))")
        }
    }
}

private func snapshotIndexEntries(gitDir: String) throws -> [String] {
    try readIndex(gitDir: gitDir).entries
        .map { String(format: "%o", $0.mode) + "|\($0.path)|\($0.oid.hex)" }
        .sorted()
}

private func snapshotStatus(gitDir: String, workdir: String?) throws -> [String] {
    guard let workdir else {
        return []
    }

    let headEntries = try headIndexEntries(gitDir: gitDir)
    let index = try readIndex(gitDir: gitDir)
    var staged: [String: Character] = [:]
    var paths = Set<String>()

    for (path, headEntry) in headEntries {
        paths.insert(path)
        if let indexEntry = index.find(path: path) {
            if indexEntry.oid != headEntry.oid || indexEntry.mode != headEntry.mode {
                staged[path] = "M"
            }
        } else {
            staged[path] = "D"
        }
    }

    for entry in index.entries {
        paths.insert(entry.path)
        if headEntries[entry.path] == nil {
            staged[entry.path] = "A"
        }
    }

    var unstaged: [String: Character] = [:]
    for entry in try workdirStatus(gitDir: gitDir, workdir: workdir) {
        paths.insert(entry.path)
        switch entry.status {
        case .deleted:
            unstaged[entry.path] = "D"
        case .new:
            unstaged[entry.path] = "?"
        case .modified:
            unstaged[entry.path] = "M"
        }
    }

    var lines: [String] = []
    for path in paths.sorted() {
        let stagedCode = staged[path] ?? " "
        let unstagedCode = unstaged[path] ?? " "
        let code = (stagedCode == " " && unstagedCode == "?") ? "??" : "\(stagedCode)\(unstagedCode)"
        if code.trimmingCharacters(in: .whitespaces).isEmpty {
            continue
        }
        lines.append("\(code)|\(path)")
    }
    return lines
}

private func snapshotHelloPatch(gitDir: String) throws -> String {
    guard let head = try? resolveRevision(gitDir: gitDir, spec: "HEAD"),
          let previous = try? resolveRevision(gitDir: gitDir, spec: "HEAD~1")
    else {
        return ""
    }

    let oldText = try treeBlobText(gitDir: gitDir, commitOID: previous, path: "hello.txt")
    let newText = try treeBlobText(gitDir: gitDir, commitOID: head, path: "hello.txt")
    if oldText == newText {
        return ""
    }
    return hex(Data(Patch.fromText(
        oldPath: "hello.txt",
        newPath: "hello.txt",
        oldText: oldText,
        newText: newText,
        context: 3
    ).format().utf8))
}

private func treeBlobText(gitDir: String, commitOID: OID, path: String) throws -> String {
    let commit = try readObject(gitDir: gitDir, oid: commitOID).asCommit()
    let treeMap = try materializeTreeMap(gitDir: gitDir, treeOID: commit.treeId, prefix: "")
    guard let blobOID = treeMap[path] else {
        throw MuonGitError.notFound(path)
    }
    let blob = try readObject(gitDir: gitDir, oid: blobOID).asBlob()
    return String(data: blob.data, encoding: .utf8) ?? ""
}

private func materializeTreeMap(gitDir: String, treeOID: OID, prefix: String) throws -> [String: OID] {
    var map: [String: OID] = [:]
    let tree = try readObject(gitDir: gitDir, oid: treeOID).asTree()
    for entry in tree.entries {
        let path = prefix.isEmpty ? entry.name : "\(prefix)/\(entry.name)"
        if entry.isTree {
            map.merge(try materializeTreeMap(gitDir: gitDir, treeOID: entry.oid, prefix: path)) { current, _ in
                current
            }
        } else {
            map[path] = entry.oid
        }
    }
    return map
}

private func headIndexEntries(gitDir: String) throws -> [String: MaterializedEntry] {
    guard let head = try? resolveRevision(gitDir: gitDir, spec: "HEAD") else {
        return [:]
    }
    let commit = try readObject(gitDir: gitDir, oid: head).asCommit()
    return try materializeHeadEntries(gitDir: gitDir, treeOID: commit.treeId, prefix: "")
}

private func materializeHeadEntries(gitDir: String, treeOID: OID, prefix: String) throws -> [String: MaterializedEntry] {
    var entries: [String: MaterializedEntry] = [:]
    let tree = try readObject(gitDir: gitDir, oid: treeOID).asTree()
    for entry in tree.entries {
        let path = prefix.isEmpty ? entry.name : "\(prefix)/\(entry.name)"
        if entry.isTree {
            entries.merge(try materializeHeadEntries(gitDir: gitDir, treeOID: entry.oid, prefix: path)) { current, _ in
                current
            }
        } else {
            entries[path] = MaterializedEntry(oid: entry.oid, mode: entry.mode)
        }
    }
    return entries
}

private func commitOptions(time: Int64) -> CommitOptions {
    let signature = Signature(
        name: "Muon Conformance",
        email: "conformance@muon.ai",
        time: time,
        offset: 0
    )
    return CommitOptions(author: signature, committer: signature)
}

private final class GitFixture {
    let remoteGitDir: String
    private let seedWorkdir: String

    init(root: String) throws {
        remoteGitDir = pathJoin(root, "remote.git")
        seedWorkdir = pathJoin(root, "seed")

        try runCommand(executable: "/usr/bin/git", arguments: ["init", "--bare", remoteGitDir], currentDirectory: root)
        try runCommand(executable: "/usr/bin/git", arguments: ["init", seedWorkdir], currentDirectory: root)
        try runCommand(executable: "/usr/bin/git", arguments: ["config", "user.name", "MuonGit Fixture"], currentDirectory: seedWorkdir)
        try runCommand(executable: "/usr/bin/git", arguments: ["config", "user.email", "fixture@muon.ai"], currentDirectory: seedWorkdir)
        try writeText(pathJoin(seedWorkdir, "hello.txt"), "hello\n")
        try runCommand(executable: "/usr/bin/git", arguments: ["add", "hello.txt"], currentDirectory: seedWorkdir)
        try runCommand(executable: "/usr/bin/git", arguments: ["commit", "-m", "initial"], currentDirectory: seedWorkdir)
        try runCommand(executable: "/usr/bin/git", arguments: ["branch", "-M", "main"], currentDirectory: seedWorkdir)
        try runCommand(executable: "/usr/bin/git", arguments: ["remote", "add", "origin", remoteGitDir], currentDirectory: seedWorkdir)
        try runCommand(executable: "/usr/bin/git", arguments: ["push", "origin", "main"], currentDirectory: seedWorkdir)
        try runCommand(
            executable: "/usr/bin/git",
            arguments: ["--git-dir", remoteGitDir, "symbolic-ref", "HEAD", "refs/heads/main"],
            currentDirectory: root
        )
    }

    func commitAndPush(fileName: String, contents: String, message: String) throws {
        try writeText(pathJoin(seedWorkdir, fileName), contents)
        try runCommand(executable: "/usr/bin/git", arguments: ["add", fileName], currentDirectory: seedWorkdir)
        try runCommand(executable: "/usr/bin/git", arguments: ["commit", "-m", message], currentDirectory: seedWorkdir)
        try runCommand(executable: "/usr/bin/git", arguments: ["push", "origin", "main"], currentDirectory: seedWorkdir)
        try runCommand(
            executable: "/usr/bin/git",
            arguments: ["--git-dir", remoteGitDir, "symbolic-ref", "HEAD", "refs/heads/main"],
            currentDirectory: (remoteGitDir as NSString).deletingLastPathComponent
        )
    }
}

private final class FixtureProcess {
    let process: Process
    let url: String

    init(process: Process, url: String) {
        self.process = process
        self.url = url
    }

    static func http(fixtureScript: String, repo: String, username: String, secret: String) throws -> FixtureProcess {
        let process = Process()
        let stdout = Pipe()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        process.arguments = [
            fixtureScript,
            "serve-http",
            "--repo", repo,
            "--auth", "basic",
            "--username", username,
            "--secret", secret,
        ]
        process.standardOutput = stdout
        process.standardError = FileHandle.standardError
        try process.run()

        let line = try readFirstLine(handle: stdout.fileHandleForReading)
        guard
            let data = line.data(using: .utf8),
            let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
            let url = json["url"] as? String
        else {
            throw ConformanceError.invalid("unexpected fixture output: \(line)")
        }
        return FixtureProcess(process: process, url: url)
    }

    func stop() {
        if process.isRunning {
            process.terminate()
            process.waitUntilExit()
        }
    }
}

private func emitJSON<T: Encodable>(_ value: T) throws {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    encoder.keyEncodingStrategy = .convertToSnakeCase
    let data = try encoder.encode(value)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0A]))
}

private func readFirstLine(handle: FileHandle) throws -> String {
    var buffer = Data()
    while true {
        let chunk = handle.availableData
        if chunk.isEmpty {
            break
        }
        buffer.append(chunk)
        if let newline = buffer.firstIndex(of: 0x0A) {
            return String(decoding: buffer.prefix(upTo: newline), as: UTF8.self)
        }
    }
    throw ConformanceError.invalid("fixture produced no startup line")
}

@discardableResult
private func runCommand(
    executable: String,
    arguments: [String],
    currentDirectory: String,
    input: Data? = nil
) throws -> Data {
    let process = Process()
    let stdout = Pipe()
    let stderr = Pipe()
    process.executableURL = URL(fileURLWithPath: executable)
    process.arguments = arguments
    process.currentDirectoryURL = URL(fileURLWithPath: currentDirectory)
    process.standardOutput = stdout
    process.standardError = stderr

    if input != nil {
        let stdin = Pipe()
        process.standardInput = stdin
        try process.run()
        stdin.fileHandleForWriting.write(input!)
        try stdin.fileHandleForWriting.close()
    } else {
        try process.run()
    }

    process.waitUntilExit()
    let out = stdout.fileHandleForReading.readDataToEndOfFile()
    let err = stderr.fileHandleForReading.readDataToEndOfFile()
    guard process.terminationStatus == 0 else {
        let message = String(decoding: err.isEmpty ? out : err, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        throw ConformanceError.invalid(message.isEmpty ? "command failed: \(arguments.joined(separator: " "))" : message)
    }
    return out
}

private func writeText(_ path: String, _ content: String) throws {
    let parent = (path as NSString).deletingLastPathComponent
    try FileManager.default.createDirectory(atPath: parent, withIntermediateDirectories: true)
    try content.write(toFile: path, atomically: true, encoding: .utf8)
}

private func copyTree(from source: String, to destination: String) throws {
    try removeIfExists(destination)
    try FileManager.default.copyItem(atPath: source, toPath: destination)
}

private func removeIfExists(_ path: String) throws {
    if FileManager.default.fileExists(atPath: path) {
        try FileManager.default.removeItem(atPath: path)
    }
}

private func relativePath(_ path: String, from root: String) -> String {
    URL(fileURLWithPath: path).path(percentEncoded: false).replacingOccurrences(of: root + "/", with: "")
}

private func pathJoin(_ lhs: String, _ rhs: String) -> String {
    (lhs as NSString).appendingPathComponent(rhs)
}

private func joinOids(_ oids: [OID]) -> String {
    oids.map(\.hex).joined(separator: ",")
}

private func hex(_ data: Data) -> String {
    data.map { String(format: "%02x", $0) }.joined()
}
