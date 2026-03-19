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

// MARK: - Clone

/// Options for clone.
public struct CloneOptions {
    public let remoteName: String
    public let branch: String?
    public let bare: Bool

    public init(remoteName: String = "origin", branch: String? = nil, bare: Bool = false) {
        self.remoteName = remoteName
        self.branch = branch
        self.bare = bare
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
