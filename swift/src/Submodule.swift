// Git submodule support
// Parity: libgit2 src/libgit2/submodule.c

import Foundation

/// A parsed submodule entry from .gitmodules.
public struct Submodule: Sendable, Equatable {
    /// Submodule name (from the section header).
    public let name: String
    /// Path relative to the repository root.
    public let path: String
    /// Remote URL.
    public let url: String
    /// Branch to track (if specified).
    public let branch: String?
    /// Whether the submodule should be fetched shallowly.
    public let shallow: Bool
    /// Update strategy (checkout, rebase, merge, none).
    public let update: String?
    /// Whether fetchRecurseSubmodules is set.
    public let fetchRecurse: Bool?
}

/// Parse a .gitmodules file content and return all submodule entries.
public func parseGitmodules(content: String) -> [Submodule] {
    let config = Config()
    parseConfigInto(config: config, content: content)
    return extractSubmodules(config: config)
}

/// Load submodules from a repository's .gitmodules file.
public func loadSubmodules(workdir: String) -> [Submodule] {
    let path = (workdir as NSString).appendingPathComponent(".gitmodules")
    guard FileManager.default.fileExists(atPath: path),
          let config = try? Config.load(from: path) else {
        return []
    }
    return extractSubmodules(config: config)
}

/// Get a specific submodule by name.
public func getSubmodule(workdir: String, name: String) throws -> Submodule {
    let submodules = loadSubmodules(workdir: workdir)
    guard let sub = submodules.first(where: { $0.name == name }) else {
        throw MuonGitError.notFound("submodule '\(name)'")
    }
    return sub
}

/// Initialize submodule config in .git/config from .gitmodules.
public func submoduleInit(gitDir: String, workdir: String, names: [String] = []) throws -> Int {
    let submodules = loadSubmodules(workdir: workdir)
    let configPath = (gitDir as NSString).appendingPathComponent("config")
    let config: Config
    if FileManager.default.fileExists(atPath: configPath) {
        config = try Config.load(from: configPath)
    } else {
        config = Config(path: configPath)
    }

    var count = 0
    for sub in submodules {
        if !names.isEmpty && !names.contains(sub.name) {
            continue
        }
        let section = "submodule.\(sub.name)"
        if config.get(section: section, key: "url") == nil {
            config.set(section: section, key: "url", value: sub.url)
            config.set(section: section, key: "active", value: "true")
            count += 1
        }
    }

    if count > 0 {
        try config.save()
    }

    return count
}

/// Write a .gitmodules file from a list of submodules.
public func writeGitmodules(workdir: String, submodules: [Submodule]) throws {
    var content = ""
    for sub in submodules {
        content += "[submodule \"\(sub.name)\"]\n"
        content += "\tpath = \(sub.path)\n"
        content += "\turl = \(sub.url)\n"
        if let branch = sub.branch {
            content += "\tbranch = \(branch)\n"
        }
        if sub.shallow {
            content += "\tshallow = true\n"
        }
        if let update = sub.update {
            content += "\tupdate = \(update)\n"
        }
        if let fetchRecurse = sub.fetchRecurse {
            content += "\tfetchRecurseSubmodules = \(fetchRecurse ? "true" : "false")\n"
        }
    }
    let path = (workdir as NSString).appendingPathComponent(".gitmodules")
    try content.write(toFile: path, atomically: true, encoding: .utf8)
}

private func parseConfigInto(config: Config, content: String) {
    // Manually parse the content into the config object
    var currentSection = ""
    for line in content.components(separatedBy: "\n") {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty || trimmed.hasPrefix("#") || trimmed.hasPrefix(";") {
            continue
        }
        if trimmed.hasPrefix("[") && trimmed.hasSuffix("]") {
            let inner = String(trimmed.dropFirst().dropLast())
            if let quoteIdx = inner.firstIndex(of: "\"") {
                let sectionName = inner[inner.startIndex..<quoteIdx].trimmingCharacters(in: .whitespaces)
                let subsection = inner[inner.index(after: quoteIdx)...].replacingOccurrences(of: "\"", with: "").trimmingCharacters(in: .whitespaces)
                currentSection = "\(sectionName).\(subsection)"
            } else {
                currentSection = inner.trimmingCharacters(in: .whitespaces)
            }
            continue
        }
        if let eqIdx = trimmed.firstIndex(of: "=") {
            let key = trimmed[trimmed.startIndex..<eqIdx].trimmingCharacters(in: .whitespaces)
            let value = trimmed[trimmed.index(after: eqIdx)...].trimmingCharacters(in: .whitespaces)
            if !key.isEmpty {
                config.set(section: currentSection, key: String(key), value: String(value))
            }
        }
    }
}

private func extractSubmodules(config: Config) -> [Submodule] {
    var names: [String] = []
    for (section, _, _) in config.allEntries {
        if section.hasPrefix("submodule.") {
            let rest = String(section.dropFirst("submodule.".count))
            if !rest.isEmpty && !names.contains(rest) {
                names.append(rest)
            }
        }
    }

    var submodules: [Submodule] = []
    for name in names {
        let section = "submodule.\(name)"
        let path = config.get(section: section, key: "path") ?? ""
        let url = config.get(section: section, key: "url") ?? ""

        if path.isEmpty && url.isEmpty { continue }

        let branch = config.get(section: section, key: "branch")
        let shallow = config.getBool(section: section, key: "shallow") ?? false
        let update = config.get(section: section, key: "update")
        let fetchRecurse = config.getBool(section: section, key: "fetchRecurseSubmodules")

        submodules.append(Submodule(
            name: name,
            path: path.isEmpty ? name : path,
            url: url,
            branch: branch,
            shallow: shallow,
            update: update,
            fetchRecurse: fetchRecurse
        ))
    }

    return submodules
}
