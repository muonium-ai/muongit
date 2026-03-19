/// MuonGit - Remote management
/// Parity: libgit2 src/libgit2/remote.c
import Foundation

/// A git remote (e.g. "origin").
public struct Remote: Equatable, Sendable {
    public let name: String
    public let url: String
    public let pushUrl: String?
    public let fetchRefspecs: [String]
}

/// List all remote names from the repository config.
public func listRemotes(gitDir: String) throws -> [String] {
    let configPath = (gitDir as NSString).appendingPathComponent("config")
    let config = try Config.load(from: configPath)
    var names: [String] = []

    for entry in config.allEntries {
        let sLower = entry.section.lowercased()
        if sLower.hasPrefix("remote.") && entry.key.lowercased() == "url" {
            let name = String(entry.section.dropFirst("remote.".count))
            if !name.isEmpty && !names.contains(name) {
                names.append(name)
            }
        }
    }

    return names
}

/// Get a remote by name from the repository config.
public func getRemote(gitDir: String, name: String) throws -> Remote {
    let configPath = (gitDir as NSString).appendingPathComponent("config")
    let config = try Config.load(from: configPath)
    let section = "remote.\(name)"

    guard let url = config.get(section: section, key: "url") else {
        throw MuonGitError.notFound("remote '\(name)' not found")
    }

    let pushUrl = config.get(section: section, key: "pushurl")

    var fetchRefspecs: [String] = []
    for entry in config.allEntries {
        if entry.section.lowercased() == section.lowercased() && entry.key.lowercased() == "fetch" {
            fetchRefspecs.append(entry.value)
        }
    }

    return Remote(name: name, url: url, pushUrl: pushUrl, fetchRefspecs: fetchRefspecs)
}

/// Add a new remote to the repository config.
@discardableResult
public func addRemote(gitDir: String, name: String, url: String) throws -> Remote {
    let configPath = (gitDir as NSString).appendingPathComponent("config")
    let config = try Config.load(from: configPath)
    let section = "remote.\(name)"

    if config.get(section: section, key: "url") != nil {
        throw MuonGitError.invalid("remote '\(name)' already exists")
    }

    let fetchRefspec = "+refs/heads/*:refs/remotes/\(name)/*"
    config.set(section: section, key: "url", value: url)
    config.set(section: section, key: "fetch", value: fetchRefspec)
    try config.save()

    return Remote(name: name, url: url, pushUrl: nil, fetchRefspecs: [fetchRefspec])
}

/// Remove a remote from the repository config.
public func removeRemote(gitDir: String, name: String) throws {
    let configPath = (gitDir as NSString).appendingPathComponent("config")
    let config = try Config.load(from: configPath)
    let section = "remote.\(name)"

    guard config.get(section: section, key: "url") != nil else {
        throw MuonGitError.notFound("remote '\(name)' not found")
    }

    config.unset(section: section, key: "url")
    config.unset(section: section, key: "pushurl")
    config.unset(section: section, key: "fetch")
    try config.save()
}

/// Rename a remote in the repository config.
public func renameRemote(gitDir: String, oldName: String, newName: String) throws {
    let remote = try getRemote(gitDir: gitDir, name: oldName)

    let configPath = (gitDir as NSString).appendingPathComponent("config")
    let config = try Config.load(from: configPath)
    let oldSection = "remote.\(oldName)"
    let newSection = "remote.\(newName)"

    if config.get(section: newSection, key: "url") != nil {
        throw MuonGitError.invalid("remote '\(newName)' already exists")
    }

    config.unset(section: oldSection, key: "url")
    config.unset(section: oldSection, key: "pushurl")
    config.unset(section: oldSection, key: "fetch")

    config.set(section: newSection, key: "url", value: remote.url)
    if let pushUrl = remote.pushUrl {
        config.set(section: newSection, key: "pushurl", value: pushUrl)
    }
    let newFetch = "+refs/heads/*:refs/remotes/\(newName)/*"
    config.set(section: newSection, key: "fetch", value: newFetch)
    try config.save()
}

/// Parse a refspec string into its components.
/// Format: [+]<src>:<dst>
/// Returns (force, src, dst).
public func parseRefspec(_ refspec: String) -> (force: Bool, src: String, dst: String)? {
    var rest = refspec
    var force = false
    if rest.hasPrefix("+") {
        force = true
        rest = String(rest.dropFirst())
    }
    guard let colonIdx = rest.firstIndex(of: ":") else { return nil }
    let src = String(rest[rest.startIndex..<colonIdx])
    let dst = String(rest[rest.index(after: colonIdx)...])
    return (force, src, dst)
}
