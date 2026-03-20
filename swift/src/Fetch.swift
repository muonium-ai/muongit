// Fetch.swift - Fetch, push, and clone operations
// Parity: libgit2 src/libgit2/fetch.c, push.c, clone.c

import Foundation

// MARK: - Fetch

/// Result of computing fetch wants.
public struct FetchNegotiation {
    /// OIDs we need to fetch.
    public let wants: [OID]
    /// OIDs we already have.
    public let haves: [OID]
    /// Remote refs that matched the fetch refspecs.
    public let matchedRefs: [MatchedRef]
}

/// A remote ref matched against a fetch refspec.
public struct MatchedRef {
    public let remoteName: String
    public let localName: String
    public let oid: OID
}

/// Match a ref name against a refspec pattern (supports trailing glob).
func refspecMatch(_ name: String, pattern: String) -> String? {
    if pattern.hasSuffix("*") {
        let prefix = String(pattern.dropLast())
        if name.hasPrefix(prefix) {
            return String(name.dropFirst(prefix.count))
        }
        return nil
    } else if name == pattern {
        return ""
    } else {
        return nil
    }
}

/// Apply a refspec to map a remote ref name to a local ref name.
public func applyRefspec(_ remoteName: String, refspec: String) -> String? {
    guard let (_, src, dst) = parseRefspec(refspec) else { return nil }
    guard let matched = refspecMatch(remoteName, pattern: src) else { return nil }

    if let dstPrefix = dst.hasSuffix("*") ? String(dst.dropLast()) : nil {
        return "\(dstPrefix)\(matched)"
    } else {
        return dst
    }
}

/// Compute which objects we need to fetch from the remote.
public func computeFetchWants(
    remoteRefs: [RemoteRef],
    refspecs: [String],
    gitDir: String
) -> FetchNegotiation {
    var wants: [OID] = []
    var matchedRefs: [MatchedRef] = []
    var seen = Set<String>()

    for rref in remoteRefs {
        for refspec in refspecs {
            if let localName = applyRefspec(rref.name, refspec: refspec) {
                matchedRefs.append(MatchedRef(
                    remoteName: rref.name,
                    localName: localName,
                    oid: rref.oid
                ))

                let alreadyHave: Bool
                if let localOid = try? resolveReference(gitDir: gitDir, name: localName) {
                    alreadyHave = localOid == rref.oid
                } else {
                    alreadyHave = false
                }

                if !alreadyHave && !seen.contains(rref.oid.hex) {
                    seen.insert(rref.oid.hex)
                    wants.append(rref.oid)
                }
            }
        }
    }

    let haves = collectLocalRefs(gitDir: gitDir)

    return FetchNegotiation(wants: wants, haves: haves, matchedRefs: matchedRefs)
}

/// Collect all local ref OIDs for negotiation.
func collectLocalRefs(gitDir: String) -> [OID] {
    var oids: [OID] = []
    let fm = FileManager.default
    for dir in ["refs/heads", "refs/remotes"] {
        let refDir = (gitDir as NSString).appendingPathComponent(dir)
        collectRefsRecursive(refDir, oids: &oids, fm: fm)
    }
    return oids
}

private func collectRefsRecursive(_ dir: String, oids: inout [OID], fm: FileManager) {
    guard let items = try? fm.contentsOfDirectory(atPath: dir) else { return }
    for item in items {
        let path = (dir as NSString).appendingPathComponent(item)
        var isDir: ObjCBool = false
        fm.fileExists(atPath: path, isDirectory: &isDir)
        if isDir.boolValue {
            collectRefsRecursive(path, oids: &oids, fm: fm)
        } else if let content = try? String(contentsOfFile: path, encoding: .utf8) {
            let hex = content.trimmingCharacters(in: .whitespacesAndNewlines)
            if hex.count == 40 {
                oids.append(OID(hex: hex))
            }
        }
    }
}

/// Update local refs after a successful fetch.
public func updateRefsFromFetch(gitDir: String, matchedRefs: [MatchedRef]) throws -> Int {
    var updated = 0
    for mref in matchedRefs {
        try writeReference(gitDir: gitDir, name: mref.localName, oid: mref.oid)
        updated += 1
    }
    return updated
}

// MARK: - Push

/// A ref update for push.
public struct PushUpdate: Equatable {
    public let srcRef: String
    public let dstRef: String
    public let srcOid: OID
    public let dstOid: OID
    public let force: Bool
}

/// Compute push updates.
public func computePushUpdates(
    pushRefspecs: [String],
    gitDir: String,
    remoteRefs: [RemoteRef]
) throws -> [PushUpdate] {
    var updates: [PushUpdate] = []

    for refspec in pushRefspecs {
        guard let (force, src, dst) = parseRefspec(refspec) else {
            throw MuonGitError.invalidObject("invalid push refspec: \(refspec)")
        }

        let srcOid = try resolveReference(gitDir: gitDir, name: src)
        let dstOid = remoteRefs.first(where: { $0.name == dst })?.oid ?? OID.zero

        updates.append(PushUpdate(
            srcRef: src,
            dstRef: dst,
            srcOid: srcOid,
            dstOid: dstOid,
            force: force
        ))
    }

    return updates
}

/// Build a push report string.
public func buildPushReport(_ updates: [PushUpdate]) -> String {
    var report = ""
    for u in updates {
        report += "\(u.dstOid.hex) \(u.srcOid.hex) \(u.dstRef)\n"
    }
    return report
}

public struct FetchOptions {
    public let refspecs: [String]?
    public let transport: TransportOptions

    public init(refspecs: [String]? = nil, transport: TransportOptions = TransportOptions()) {
        self.refspecs = refspecs
        self.transport = transport
    }
}

public struct FetchResult {
    public let advertisedRefs: [RemoteRef]
    public let capabilities: ServerCapabilities
    public let matchedRefs: [MatchedRef]
    public let updatedRefs: Int
    public let indexedPack: IndexedPack?
}

public struct PushOptions {
    public let refspecs: [String]?
    public let transport: TransportOptions

    public init(refspecs: [String]? = nil, transport: TransportOptions = TransportOptions()) {
        self.refspecs = refspecs
        self.transport = transport
    }
}

public struct PushResult {
    public let advertisedRefs: [RemoteRef]
    public let updatedTrackingRefs: Int
    public let report: String
}

// MARK: - Clone

/// Options for clone.
public struct CloneOptions {
    public let remoteName: String
    public let branch: String?
    public let bare: Bool
    public let transport: TransportOptions

    public init(
        remoteName: String = "origin",
        branch: String? = nil,
        bare: Bool = false,
        transport: TransportOptions = TransportOptions()
    ) {
        self.remoteName = remoteName
        self.branch = branch
        self.bare = bare
        self.transport = transport
    }
}

/// Set up a new repository for clone: init repo, add remote, configure HEAD.
public func cloneSetup(path: String, url: String, options: CloneOptions = CloneOptions()) throws -> Repository {
    let repo = try Repository.create(at: path, bare: options.bare)

    _ = try addRemote(gitDir: repo.gitDir, name: options.remoteName, url: url)

    if let branch = options.branch {
        let target = "refs/heads/\(branch)"
        try writeSymbolicReference(gitDir: repo.gitDir, name: "HEAD", target: target)
    }

    return repo
}

/// After fetching, set up HEAD and the default branch for a clone.
public func cloneFinish(gitDir: String, remoteName: String, defaultBranch: String, headOid: OID) throws {
    let localBranch = "refs/heads/\(defaultBranch)"
    let remoteRef = "refs/remotes/\(remoteName)/\(defaultBranch)"

    try writeReference(gitDir: gitDir, name: localBranch, oid: headOid)
    try writeReference(gitDir: gitDir, name: remoteRef, oid: headOid)
    try writeSymbolicReference(gitDir: gitDir, name: "HEAD", target: localBranch)
}

/// Extract the default branch from server capabilities.
public func defaultBranchFromCaps(_ caps: ServerCapabilities) -> String? {
    guard let symref = caps.get("symref") else { return nil }
    let parts = symref.split(separator: ":", maxSplits: 1).map(String.init)
    guard parts.count == 2, parts[0] == "HEAD" else { return nil }
    let target = parts[1]
    if target.hasPrefix("refs/heads/") {
        return String(target.dropFirst("refs/heads/".count))
    }
    return nil
}

public func cloneRepository(from url: String, to path: String, options: CloneOptions = CloneOptions()) throws -> Repository {
    let repo = try cloneSetup(path: path, url: url, options: options)
    let fetch = try fetchRemote(repository: repo, remoteName: options.remoteName, options: FetchOptions(transport: options.transport))
    let (branch, headOID) = try resolveCloneHead(fetch, branch: options.branch)
    try cloneFinish(gitDir: repo.gitDir, remoteName: options.remoteName, defaultBranch: branch, headOid: headOID)
    if !options.bare {
        _ = try reset(gitDir: repo.gitDir, workdir: repo.workdir, spec: "HEAD", mode: .hard)
    }
    return repo
}

public func fetchRemote(
    repository: Repository,
    remoteName: String,
    options: FetchOptions = FetchOptions()
) throws -> FetchResult {
    let remote = try getRemote(gitDir: repository.gitDir, name: remoteName)
    let advertisement = try advertiseUploadPack(url: remote.url, options: options.transport)
    let refspecs = options.refspecs ?? remote.fetchRefspecs
    let negotiation = computeFetchWants(remoteRefs: advertisement.refs, refspecs: refspecs, gitDir: repository.gitDir)

    if negotiation.wants.isEmpty {
        let updated = try updateRefsFromFetch(gitDir: repository.gitDir, matchedRefs: negotiation.matchedRefs)
        return FetchResult(
            advertisedRefs: advertisement.refs,
            capabilities: advertisement.capabilities,
            matchedRefs: negotiation.matchedRefs,
            updatedRefs: updated,
            indexedPack: nil
        )
    }

    let request = buildWantHave(
        wants: negotiation.wants,
        haves: negotiation.haves,
        caps: fetchCapabilities(advertisement.capabilities)
    )
    let response = try uploadPack(url: remote.url, request: request, options: options.transport)
    let indexedPack = try extractPackFromFetchResponse(response)
        .map { try indexPackToODB(gitDir: repository.gitDir, packBytes: Array($0)) }
    let updated = try updateRefsFromFetch(gitDir: repository.gitDir, matchedRefs: negotiation.matchedRefs)

    return FetchResult(
        advertisedRefs: advertisement.refs,
        capabilities: advertisement.capabilities,
        matchedRefs: negotiation.matchedRefs,
        updatedRefs: updated,
        indexedPack: indexedPack
    )
}

public func pushRemote(
    repository: Repository,
    remoteName: String,
    options: PushOptions = PushOptions()
) throws -> PushResult {
    let remote = try getRemote(gitDir: repository.gitDir, name: remoteName)
    let advertisement = try advertiseReceivePack(url: remote.url, options: options.transport)
    let refspecs = try options.refspecs ?? defaultPushRefspecs(gitDir: repository.gitDir)
    let updates = try computePushUpdates(pushRefspecs: refspecs, gitDir: repository.gitDir, remoteRefs: advertisement.refs)

    for update in updates {
        if update.force {
            continue
        }
        if !(try isFastForward(gitDir: repository.gitDir, oldOID: update.dstOid, newOID: update.srcOid)) {
            throw MuonGitError.notFastForward
        }
    }

    let pack = try buildPackFromOIDs(
        gitDir: repository.gitDir,
        roots: updates.map { $0.srcOid },
        exclude: advertisement.refs.map { $0.oid }
    )
    let request = buildPushRequest(updates: updates, pack: pack, capabilities: advertisement.capabilities)
    let response = try receivePack(url: remote.url, request: Data(request), options: options.transport)
    let report = try parsePushResponse(response)
    let updatedTrackingRefs = try updateTrackingRefsAfterPush(gitDir: repository.gitDir, remoteName: remoteName, updates: updates)

    return PushResult(
        advertisedRefs: advertisement.refs,
        updatedTrackingRefs: updatedTrackingRefs,
        report: report
    )
}

public extension Repository {
    func fetch(remoteName: String, options: FetchOptions = FetchOptions()) throws -> FetchResult {
        try fetchRemote(repository: self, remoteName: remoteName, options: options)
    }

    func push(remoteName: String, options: PushOptions = PushOptions()) throws -> PushResult {
        try pushRemote(repository: self, remoteName: remoteName, options: options)
    }
}

private func fetchCapabilities(_ caps: ServerCapabilities) -> [String] {
    var requested: [String] = []
    if caps.has("side-band-64k") {
        requested.append("side-band-64k")
    } else if caps.has("side-band") {
        requested.append("side-band")
    }
    if caps.has("ofs-delta") {
        requested.append("ofs-delta")
    }
    if caps.has("include-tag") {
        requested.append("include-tag")
    }
    return requested
}

private func extractPackFromFetchResponse(_ response: Data) throws -> Data? {
    let (lines, consumed) = try unwrap(pktLineDecode(response))
    var pack = Data()

    for line in lines {
        guard case let .data(data) = line else { continue }
        if data.starts(with: Data("ACK ".utf8)) || data == Data("NAK\n".utf8) {
            continue
        }
        guard let first = data.first else { continue }
        switch first {
        case 1:
            pack.append(data.dropFirst())
        case 2:
            break
        case 3:
            throw MuonGitError.invalid(String(decoding: data.dropFirst(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
        default:
            break
        }
    }

    if !pack.isEmpty {
        return pack
    }

    if consumed < response.count {
        let trailing = response.subdata(in: consumed..<response.count)
        if trailing.starts(with: Data("PACK".utf8)) {
            return trailing
        }
    }

    return nil
}

private func buildPushRequest(updates: [PushUpdate], pack: [UInt8], capabilities: ServerCapabilities) -> [UInt8] {
    var requestedCaps = ["report-status"]
    if capabilities.has("ofs-delta") {
        requestedCaps.append("ofs-delta")
    }

    var out = Data()
    for (index, update) in updates.enumerated() {
        let line: String
        if index == 0 {
            line = "\(update.dstOid.hex) \(update.srcOid.hex) \(update.dstRef)\u{0}\(requestedCaps.joined(separator: " "))\n"
        } else {
            line = "\(update.dstOid.hex) \(update.srcOid.hex) \(update.dstRef)\n"
        }
        out.append(pktLineEncode(Data(line.utf8)))
    }
    out.append(pktLineFlush())
    out.append(Data(pack))
    return Array(out)
}

private func parsePushResponse(_ response: Data) throws -> String {
    let (lines, consumed) = try unwrap(pktLineDecode(response))
    var text = ""

    for line in lines {
        guard case let .data(data) = line else { continue }
        let payload: Data
        if let first = data.first, first == 1 || first == 2 {
            payload = data.dropFirst()
        } else if let first = data.first, first == 3 {
            throw MuonGitError.invalid(String(decoding: data.dropFirst(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
        } else {
            payload = data
        }
        text += String(decoding: payload, as: UTF8.self)
    }

    if consumed < response.count {
        text += String(decoding: response.subdata(in: consumed..<response.count), as: UTF8.self)
    }

    for line in text.split(separator: "\n").map(String.init) {
        if line.hasPrefix("unpack "), line != "unpack ok" {
            throw MuonGitError.invalid(line)
        }
        if line.hasPrefix("ng ") {
            throw MuonGitError.invalid(String(line.dropFirst(3)))
        }
    }

    return text
}

private func resolveCloneHead(_ fetch: FetchResult, branch: String?) throws -> (String, OID) {
    if let branch {
        let refName = "refs/heads/\(branch)"
        guard let oid = fetch.advertisedRefs.first(where: { $0.name == refName })?.oid else {
            throw MuonGitError.notFound("remote branch '\(branch)' not found")
        }
        return (branch, oid)
    }

    if let branch = defaultBranchFromCaps(fetch.capabilities) {
        let refName = "refs/heads/\(branch)"
        if let oid = fetch.advertisedRefs.first(where: { $0.name == refName })?.oid {
            return (branch, oid)
        }
    }

    if let headOID = fetch.advertisedRefs.first(where: { $0.name == "HEAD" })?.oid,
       let branchRef = fetch.advertisedRefs.first(where: { $0.name.hasPrefix("refs/heads/") && $0.oid == headOID }) {
        return (String(branchRef.name.dropFirst("refs/heads/".count)), headOID)
    }

    for candidate in ["main", "master"] {
        let refName = "refs/heads/\(candidate)"
        if let oid = fetch.advertisedRefs.first(where: { $0.name == refName })?.oid {
            return (candidate, oid)
        }
    }

    throw MuonGitError.notFound("could not determine remote default branch")
}

private func defaultPushRefspecs(gitDir: String) throws -> [String] {
    let head = try readReference(gitDir: gitDir, name: "HEAD")
    guard head.hasPrefix("ref: ") else {
        throw MuonGitError.invalidSpec("HEAD is detached; provide push refspecs")
    }
    let target = String(head.dropFirst(5)).trimmingCharacters(in: .whitespacesAndNewlines)
    return ["\(target):\(target)"]
}

private func isFastForward(gitDir: String, oldOID: OID, newOID: OID) throws -> Bool {
    if oldOID == .zero {
        return true
    }
    return try mergeBase(gitDir: gitDir, oid1: oldOID, oid2: newOID) == oldOID
}

private func updateTrackingRefsAfterPush(gitDir: String, remoteName: String, updates: [PushUpdate]) throws -> Int {
    var updated = 0
    for update in updates where update.dstRef.hasPrefix("refs/heads/") {
        let branch = update.dstRef.dropFirst("refs/heads/".count)
        try writeReference(gitDir: gitDir, name: "refs/remotes/\(remoteName)/\(branch)", oid: update.srcOid)
        updated += 1
    }
    return updated
}

private func unwrap<T>(_ result: Result<T, MuonGitError>) throws -> T {
    switch result {
    case let .success(value): return value
    case let .failure(error): throw error
    }
}
