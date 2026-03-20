/// MuonGit - First-class branch API
import Foundation

public enum BranchType: Sendable {
    case local
    case remote
}

public struct BranchUpstream: Sendable, Equatable {
    public let remoteName: String
    public let mergeRef: String

    public init(remoteName: String, mergeRef: String) {
        self.remoteName = remoteName
        self.mergeRef = mergeRef
    }
}

public struct Branch: Sendable, Equatable {
    public let name: String
    public let referenceName: String
    public let target: OID?
    public let kind: BranchType
    public let isHEAD: Bool
    public let upstream: BranchUpstream?
}

public func createBranch(
    gitDir: String,
    name: String,
    target: OID? = nil,
    force: Bool = false
) throws -> Branch {
    let refName = localBranchRef(name)
    let refdb = RefDb(gitDir: gitDir)

    if referenceExists(gitDir: gitDir, name: refName) {
        if !force {
            throw MuonGitError.conflict("branch '\(name)' already exists")
        }
        _ = try refdb.delete(name: refName)
    }

    let targetOID = try target ?? headTargetOID(gitDir: gitDir)
    try refdb.write(name: refName, oid: targetOID)
    return try lookupBranch(gitDir: gitDir, name: name, kind: .local)
}

public func lookupBranch(gitDir: String, name: String, kind: BranchType) throws -> Branch {
    try buildBranch(gitDir: gitDir, refName: branchRefName(name: name, kind: kind), kind: kind)
}

public func listBranches(gitDir: String, kind: BranchType? = nil) throws -> [Branch] {
    let refs = try listReferences(gitDir: gitDir)
    var branches: [Branch] = []

    for (refName, _) in refs {
        if let branchType = branchType(for: refName) {
            if kind == nil || kind == branchType {
                branches.append(try buildBranch(gitDir: gitDir, refName: refName, kind: branchType))
            }
        }
    }

    return branches.sorted { $0.referenceName < $1.referenceName }
}

public func renameBranch(
    gitDir: String,
    oldName: String,
    newName: String,
    force: Bool = false
) throws -> Branch {
    let oldRef = localBranchRef(oldName)
    let newRef = localBranchRef(newName)
    if oldRef == newRef {
        return try lookupBranch(gitDir: gitDir, name: newName, kind: .local)
    }

    let refdb = RefDb(gitDir: gitDir)
    let oldBranch = try refdb.read(name: oldRef)

    if referenceExists(gitDir: gitDir, name: newRef) {
        if !force {
            throw MuonGitError.conflict("branch '\(newName)' already exists")
        }
        if try currentHEADRef(gitDir: gitDir) == newRef {
            throw MuonGitError.conflict("cannot replace checked out branch '\(newName)'")
        }
        _ = try deleteBranch(gitDir: gitDir, name: newName, kind: .local)
    }

    if let symbolicTarget = oldBranch.symbolicTarget {
        try refdb.writeSymbolic(name: newRef, target: symbolicTarget)
    } else if let target = oldBranch.target {
        try refdb.write(name: newRef, oid: target)
    } else {
        throw MuonGitError.invalidObject("branch '\(oldName)' has no target")
    }
    _ = try refdb.delete(name: oldRef)
    try moveBranchUpstream(gitDir: gitDir, oldName: oldName, newName: newName)

    if try currentHEADRef(gitDir: gitDir) == oldRef {
        try refdb.writeSymbolic(name: "HEAD", target: newRef)
    }

    return try lookupBranch(gitDir: gitDir, name: newName, kind: .local)
}

@discardableResult
public func deleteBranch(gitDir: String, name: String, kind: BranchType) throws -> Bool {
    let refName = branchRefName(name: name, kind: kind)
    let headRef = try currentHEADRef(gitDir: gitDir)
    if kind == .local && headRef == refName {
        throw MuonGitError.conflict("cannot delete checked out branch '\(name)'")
    }

    let deleted = try RefDb(gitDir: gitDir).delete(name: refName)
    if deleted && kind == .local {
        try clearBranchUpstream(gitDir: gitDir, name: name)
    }
    return deleted
}

public func branchUpstream(gitDir: String, name: String) throws -> BranchUpstream? {
    let config = try loadRepoConfig(gitDir: gitDir)
    let section = branchSection(name)
    let remote = config.get(section: section, key: "remote")
    let merge = config.get(section: section, key: "merge")

    switch (remote, merge) {
    case let (remoteName?, mergeRef?):
        return BranchUpstream(remoteName: remoteName, mergeRef: mergeRef)
    case (nil, nil):
        return nil
    default:
        throw MuonGitError.invalidSpec("branch '\(name)' has incomplete upstream config")
    }
}

public func setBranchUpstream(gitDir: String, name: String, upstream: BranchUpstream?) throws {
    let refName = localBranchRef(name)
    guard referenceExists(gitDir: gitDir, name: refName) else {
        throw MuonGitError.notFound("branch '\(name)' not found")
    }

    let config = try loadRepoConfig(gitDir: gitDir)
    let section = branchSection(name)
    if let upstream {
        config.set(section: section, key: "remote", value: upstream.remoteName)
        config.set(section: section, key: "merge", value: upstream.mergeRef)
    } else {
        config.unset(section: section, key: "remote")
        config.unset(section: section, key: "merge")
    }
    try config.save()
}

public extension Repository {
    func createBranch(name: String, target: OID? = nil, force: Bool = false) throws -> Branch {
        try MuonGit.createBranch(gitDir: gitDir, name: name, target: target, force: force)
    }

    func lookupBranch(name: String, kind: BranchType) throws -> Branch {
        try MuonGit.lookupBranch(gitDir: gitDir, name: name, kind: kind)
    }

    func listBranches(kind: BranchType? = nil) throws -> [Branch] {
        try MuonGit.listBranches(gitDir: gitDir, kind: kind)
    }
}

private func buildBranch(gitDir: String, refName: String, kind: BranchType) throws -> Branch {
    let reference = try RefDb(gitDir: gitDir).read(name: refName)
    let target = reference.isSymbolic ? (try? resolveReference(gitDir: gitDir, name: refName)) : reference.target
    guard let shortName = shortBranchName(refName: refName, kind: kind) else {
        throw MuonGitError.invalidSpec("not a branch reference: \(refName)")
    }

    return Branch(
        name: shortName,
        referenceName: refName,
        target: target,
        kind: kind,
        isHEAD: try currentHEADRef(gitDir: gitDir) == refName,
        upstream: kind == .local ? try branchUpstream(gitDir: gitDir, name: shortName) : nil
    )
}

private func branchRefName(name: String, kind: BranchType) -> String {
    switch kind {
    case .local: return localBranchRef(name)
    case .remote: return "refs/remotes/\(name)"
    }
}

private func localBranchRef(_ name: String) -> String {
    "refs/heads/\(name)"
}

private func shortBranchName(refName: String, kind: BranchType) -> String? {
    switch kind {
    case .local:
        return refName.hasPrefix("refs/heads/") ? String(refName.dropFirst("refs/heads/".count)) : nil
    case .remote:
        return refName.hasPrefix("refs/remotes/") ? String(refName.dropFirst("refs/remotes/".count)) : nil
    }
}

private func branchType(for refName: String) -> BranchType? {
    if refName.hasPrefix("refs/heads/") {
        return .local
    }
    if refName.hasPrefix("refs/remotes/") {
        return .remote
    }
    return nil
}

private func currentHEADRef(gitDir: String) throws -> String? {
    let head = try readReference(gitDir: gitDir, name: "HEAD")
    guard head.hasPrefix("ref: ") else { return nil }
    let target = String(head.dropFirst(5)).trimmingCharacters(in: .whitespacesAndNewlines)
    return target.hasPrefix("refs/heads/") ? target : nil
}

private func headTargetOID(gitDir: String) throws -> OID {
    let head = try readReference(gitDir: gitDir, name: "HEAD")
    if head.hasPrefix("ref: ") {
        do {
            return try resolveReference(gitDir: gitDir, name: "HEAD")
        } catch MuonGitError.notFound(_) {
            throw MuonGitError.unbornBranch
        }
    }
    return OID(hex: head.trimmingCharacters(in: .whitespacesAndNewlines))
}

private func branchSection(_ name: String) -> String {
    "branch.\(name)"
}

private func loadRepoConfig(gitDir: String) throws -> Config {
    let configPath = (gitDir as NSString).appendingPathComponent("config")
    if FileManager.default.fileExists(atPath: configPath) {
        return try Config.load(from: configPath)
    }
    return Config(path: configPath)
}

private func clearBranchUpstream(gitDir: String, name: String) throws {
    let configPath = (gitDir as NSString).appendingPathComponent("config")
    guard FileManager.default.fileExists(atPath: configPath) else {
        return
    }
    let config = try Config.load(from: configPath)
    let section = branchSection(name)
    config.unset(section: section, key: "remote")
    config.unset(section: section, key: "merge")
    try config.save()
}

private func moveBranchUpstream(gitDir: String, oldName: String, newName: String) throws {
    let upstream = try branchUpstream(gitDir: gitDir, name: oldName)
    try clearBranchUpstream(gitDir: gitDir, name: oldName)
    if let upstream {
        try setBranchUpstream(gitDir: gitDir, name: newName, upstream: upstream)
    }
}

private func referenceExists(gitDir: String, name: String) -> Bool {
    (try? readReference(gitDir: gitDir, name: name)) != nil
}
