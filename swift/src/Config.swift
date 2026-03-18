/// MuonGit - Git config file read/write
/// Parity: libgit2 src/libgit2/config_file.c
import Foundation

/// A parsed git config file
public final class Config {
    /// Entries stored as (section, key, value) triples.
    /// Section includes subsection: "section" or "section.subsection"
    private var entries: [(section: String, key: String, value: String)]
    /// Path to the config file (nil for in-memory configs)
    public let path: String?

    public init(path: String? = nil) {
        self.entries = []
        self.path = path
    }

    // MARK: - Parsing

    /// Load a config file from disk
    public static func load(from path: String) throws -> Config {
        let content = try String(contentsOfFile: path, encoding: .utf8)
        let config = Config(path: path)
        config.entries = parseConfig(content)
        return config
    }

    // MARK: - Get

    /// Get a config value by section and key (e.g., section="core", key="bare")
    /// For subsections: section="remote.origin", key="url"
    public func get(section: String, key: String) -> String? {
        let sectionLower = section.lowercased()
        let keyLower = key.lowercased()
        // Return last matching entry (last wins)
        for entry in entries.reversed() {
            if entry.section.lowercased() == sectionLower && entry.key.lowercased() == keyLower {
                return entry.value
            }
        }
        return nil
    }

    /// Get a boolean config value
    public func getBool(section: String, key: String) -> Bool? {
        guard let value = get(section: section, key: key) else { return nil }
        switch value.lowercased() {
        case "true", "yes", "on", "1": return true
        case "false", "no", "off", "0", "": return false
        default: return nil
        }
    }

    /// Get an integer config value
    public func getInt(section: String, key: String) -> Int? {
        guard let value = get(section: section, key: key) else { return nil }
        return parseConfigInt(value)
    }

    // MARK: - Set

    /// Set a config value. Updates existing entry or appends new one.
    public func set(section: String, key: String, value: String) {
        let sectionLower = section.lowercased()
        let keyLower = key.lowercased()
        // Update last matching entry if exists
        for i in stride(from: entries.count - 1, through: 0, by: -1) {
            if entries[i].section.lowercased() == sectionLower &&
               entries[i].key.lowercased() == keyLower {
                entries[i] = (section: section, key: key, value: value)
                return
            }
        }
        // Append new entry
        entries.append((section: section, key: key, value: value))
    }

    /// Remove all entries matching section and key
    public func unset(section: String, key: String) {
        let sectionLower = section.lowercased()
        let keyLower = key.lowercased()
        entries.removeAll {
            $0.section.lowercased() == sectionLower && $0.key.lowercased() == keyLower
        }
    }

    // MARK: - Enumerate

    /// Get all entries as (section, key, value) tuples
    public var allEntries: [(section: String, key: String, value: String)] {
        entries
    }

    /// Get all entries in a given section
    public func entries(inSection section: String) -> [(key: String, value: String)] {
        let sectionLower = section.lowercased()
        return entries
            .filter { $0.section.lowercased() == sectionLower }
            .map { (key: $0.key, value: $0.value) }
    }

    // MARK: - Save

    /// Serialize and write back to disk (requires path)
    public func save() throws {
        guard let path = path else {
            throw MuonGitError.invalid("config has no file path")
        }
        let content = serializeConfig(entries)
        try content.write(toFile: path, atomically: true, encoding: .utf8)
    }
}

// MARK: - Parsing helpers

func parseConfig(_ content: String) -> [(section: String, key: String, value: String)] {
    var result: [(section: String, key: String, value: String)] = []
    var currentSection = ""

    for line in content.components(separatedBy: .newlines) {
        let trimmed = line.trimmingCharacters(in: .whitespaces)

        // Skip empty lines and comments
        if trimmed.isEmpty || trimmed.hasPrefix("#") || trimmed.hasPrefix(";") {
            continue
        }

        // Section header: [section] or [section "subsection"]
        if trimmed.hasPrefix("[") && trimmed.hasSuffix("]") {
            let inner = String(trimmed.dropFirst().dropLast())
            if let quoteStart = inner.firstIndex(of: "\"") {
                let sectionName = inner[inner.startIndex..<quoteStart].trimmingCharacters(in: .whitespaces)
                let subsection = inner[inner.index(after: quoteStart)...]
                    .trimmingCharacters(in: .whitespaces)
                    .replacingOccurrences(of: "\"", with: "")
                currentSection = "\(sectionName).\(subsection)"
            } else {
                currentSection = inner.trimmingCharacters(in: .whitespaces)
            }
            continue
        }

        // Key = value
        if let eqIndex = trimmed.firstIndex(of: "=") {
            let key = trimmed[trimmed.startIndex..<eqIndex].trimmingCharacters(in: .whitespaces)
            let value = trimmed[trimmed.index(after: eqIndex)...].trimmingCharacters(in: .whitespaces)
            if !key.isEmpty {
                result.append((section: currentSection, key: key, value: value))
            }
        } else {
            // Boolean key (no = sign means true)
            if !trimmed.isEmpty {
                result.append((section: currentSection, key: trimmed, value: "true"))
            }
        }
    }

    return result
}

func serializeConfig(_ entries: [(section: String, key: String, value: String)]) -> String {
    var lines: [String] = []
    var currentSection = ""

    for entry in entries {
        if entry.section != currentSection {
            currentSection = entry.section
            // Check for subsection
            if let dotIndex = currentSection.firstIndex(of: ".") {
                let section = currentSection[currentSection.startIndex..<dotIndex]
                let subsection = currentSection[currentSection.index(after: dotIndex)...]
                lines.append("[\(section) \"\(subsection)\"]")
            } else {
                lines.append("[\(currentSection)]")
            }
        }
        lines.append("\t\(entry.key) = \(entry.value)")
    }

    return lines.joined(separator: "\n") + "\n"
}

/// Parse git config integer with optional suffix (k, m, g)
func parseConfigInt(_ s: String) -> Int? {
    let trimmed = s.trimmingCharacters(in: .whitespaces).lowercased()
    if trimmed.isEmpty { return nil }

    let last = trimmed.last!
    if last == "k" {
        guard let n = Int(trimmed.dropLast()) else { return nil }
        return n * 1024
    } else if last == "m" {
        guard let n = Int(trimmed.dropLast()) else { return nil }
        return n * 1024 * 1024
    } else if last == "g" {
        guard let n = Int(trimmed.dropLast()) else { return nil }
        return n * 1024 * 1024 * 1024
    }
    return Int(trimmed)
}
