// Filter.swift - Clean/smudge filter system
// Parity: libgit2 src/libgit2/filter.c, crlf.c, ident.c

import Foundation

/// Direction of filtering.
/// Parity: git_filter_mode_t
public enum FilterMode {
    /// Working directory → ODB (clean)
    case toOdb
    /// ODB → working directory (smudge)
    case toWorktree
}

/// Metadata about the file being filtered.
/// Parity: git_filter_source
public struct FilterSource {
    public let path: String
    public let mode: FilterMode
    public let oid: OID?

    public init(path: String, mode: FilterMode, oid: OID? = nil) {
        self.path = path
        self.mode = mode
        self.oid = oid
    }
}

/// Result of applying a filter.
public enum FilterResult {
    case applied(Data)
    case passthrough
}

/// A single filter implementation.
public protocol FilterProtocol {
    var name: String { get }
    func check(source: FilterSource, attrs: Attributes) -> Bool
    func apply(input: Data, source: FilterSource) -> FilterResult
}

/// A chain of filters to apply to a file.
/// Parity: git_filter_list
public class FilterList {
    private var filters: [FilterProtocol] = []
    public let source: FilterSource

    init(source: FilterSource) {
        self.source = source
    }

    /// Load the applicable filters for a path in a repository.
    /// Parity: git_filter_list_load
    public static func load(
        gitDir: String,
        workdir: String?,
        path: String,
        mode: FilterMode,
        oid: OID? = nil
    ) -> FilterList {
        let attrs = Attributes.loadForRepo(gitDir: gitDir, workdir: workdir)
        let source = FilterSource(path: path, mode: mode, oid: oid)
        let list = FilterList(source: source)

        let crlf = CrlfFilter(gitDir: gitDir)
        let ident = IdentFilter()

        switch mode {
        case .toWorktree:
            // Smudge: CRLF(0) → Ident(100)
            if crlf.check(source: source, attrs: attrs) {
                list.filters.append(crlf)
            }
            if ident.check(source: source, attrs: attrs) {
                list.filters.append(ident)
            }
        case .toOdb:
            // Clean: Ident(100) → CRLF(0) (reverse order)
            if ident.check(source: source, attrs: attrs) {
                list.filters.append(ident)
            }
            if crlf.check(source: source, attrs: attrs) {
                list.filters.append(crlf)
            }
        }

        return list
    }

    /// Apply all filters in the chain to the input data.
    /// Parity: git_filter_list_apply_to_buffer
    public func apply(_ input: Data) -> Data {
        var data = input
        for filter in filters {
            switch filter.apply(input: data, source: source) {
            case .applied(let output):
                data = output
            case .passthrough:
                break
            }
        }
        return data
    }

    /// Number of active filters.
    public var count: Int { filters.count }

    /// Whether the filter list is empty.
    public var isEmpty: Bool { filters.isEmpty }

    /// Check if a named filter is in the list.
    public func contains(_ name: String) -> Bool {
        filters.contains { $0.name == name }
    }
}

// MARK: - CRLF Filter (priority 0)
// Parity: libgit2 src/libgit2/crlf.c

/// Resolved CRLF action.
private enum CrlfAction {
    case none
    case crlfToLf
    case lfToCrlf
    case auto
}

/// End-of-line style.
private enum EolStyle {
    case lf
    case crlf
    case native
}

/// CRLF / text / eol filter.
public class CrlfFilter: FilterProtocol {
    private let autoCrlf: String?
    private let coreEol: String?

    public init(gitDir: String) {
        let configPath = (gitDir as NSString).appendingPathComponent("config")
        let config = (try? Config.load(from: configPath)) ?? Config()
        self.autoCrlf = config.get(section: "core", key: "autocrlf")
        self.coreEol = config.get(section: "core", key: "eol")
    }

    init(autoCrlf: String?, coreEol: String?) {
        self.autoCrlf = autoCrlf
        self.coreEol = coreEol
    }

    public var name: String { "crlf" }

    public func check(source: FilterSource, attrs: Attributes) -> Bool {
        let action = resolveAction(attrs: attrs, path: source.path, mode: source.mode)
        return action != .none
    }

    public func apply(input: Data, source: FilterSource) -> FilterResult {
        if isBinary(input) {
            return .passthrough
        }
        switch source.mode {
        case .toOdb:
            return crlfToLf(input)
        case .toWorktree:
            return lfToCrlf(input)
        }
    }

    private func resolveAction(attrs: Attributes, path: String, mode: FilterMode) -> CrlfAction {
        let textAttr = attrs.get(path, attr: "text")
        let crlfAttr = attrs.get(path, attr: "crlf")
        let eolAttr = attrs.get(path, attr: "eol")

        // text attribute takes priority
        var isText: Bool? = nil
        switch textAttr {
        case .set:
            isText = true
        case .unset:
            isText = false
        case .value(let v) where v == "auto":
            return .auto
        default:
            break
        }

        // Fall back to crlf attribute
        if isText == nil {
            switch crlfAttr {
            case .set:
                isText = true
            case .unset:
                isText = false
            default:
                break
            }
        }

        // Fall back to core.autocrlf config
        if isText == nil {
            switch autoCrlf {
            case "true", "input":
                isText = true
            default:
                break
            }
        }

        guard isText == true else {
            return .none
        }

        let outputEol = resolveEol(eolAttr: eolAttr, mode: mode)

        switch mode {
        case .toOdb:
            return .crlfToLf
        case .toWorktree:
            switch outputEol {
            case .crlf:
                return .lfToCrlf
            case .lf:
                return .none
            case .native:
                #if os(Windows)
                return .lfToCrlf
                #else
                return .none
                #endif
            }
        }
    }

    private func resolveEol(eolAttr: AttrValue?, mode: FilterMode) -> EolStyle {
        if case .value(let v) = eolAttr {
            switch v {
            case "lf": return .lf
            case "crlf": return .crlf
            default: return .native
            }
        }

        if mode == .toOdb, autoCrlf == "input" {
            return .lf
        }

        switch coreEol {
        case "lf": return .lf
        case "crlf": return .crlf
        default: return .native
        }
    }
}

// MARK: - Ident Filter (priority 100)
// Parity: libgit2 src/libgit2/ident.c

/// $Id$ expansion/contraction filter.
public class IdentFilter: FilterProtocol {
    public init() {}

    public var name: String { "ident" }

    public func check(source: FilterSource, attrs: Attributes) -> Bool {
        attrs.get(source.path, attr: "ident") == .set
    }

    public func apply(input: Data, source: FilterSource) -> FilterResult {
        if isBinary(input) {
            return .passthrough
        }
        switch source.mode {
        case .toWorktree:
            return identSmudge(input, oid: source.oid)
        case .toOdb:
            return identClean(input)
        }
    }
}

// MARK: - Internal Helpers

/// Convert CRLF to LF (clean direction).
func crlfToLf(_ input: Data) -> FilterResult {
    let bytes = Array(input)
    guard bytes.contains(where: { $0 == 0x0D }) else {
        // Quick check: no CR at all
        return .passthrough
    }

    var hasCrlf = false
    for i in 0..<(bytes.count - 1) {
        if bytes[i] == 0x0D && bytes[i + 1] == 0x0A {
            hasCrlf = true
            break
        }
    }
    guard hasCrlf else {
        return .passthrough
    }

    var output = Data()
    output.reserveCapacity(bytes.count)
    var i = 0
    while i < bytes.count {
        if i + 1 < bytes.count && bytes[i] == 0x0D && bytes[i + 1] == 0x0A {
            output.append(0x0A)
            i += 2
        } else {
            output.append(bytes[i])
            i += 1
        }
    }
    return .applied(output)
}

/// Convert LF to CRLF (smudge direction).
func lfToCrlf(_ input: Data) -> FilterResult {
    let bytes = Array(input)
    var hasBareLf = false
    for i in 0..<bytes.count {
        if bytes[i] == 0x0A && (i == 0 || bytes[i - 1] != 0x0D) {
            hasBareLf = true
            break
        }
    }
    guard hasBareLf else {
        return .passthrough
    }

    var output = Data()
    output.reserveCapacity(bytes.count + bytes.count / 10)
    for i in 0..<bytes.count {
        if bytes[i] == 0x0A && (i == 0 || bytes[i - 1] != 0x0D) {
            output.append(0x0D)
        }
        output.append(bytes[i])
    }
    return .applied(output)
}

/// Simple binary detection: check for NUL bytes in the first 8000 bytes.
func isBinary(_ data: Data) -> Bool {
    let checkLen = min(data.count, 8000)
    return data.prefix(checkLen).contains(0)
}

/// Smudge: Replace `$Id$` with `$Id: <hex> $`
func identSmudge(_ input: Data, oid: OID?) -> FilterResult {
    guard let oid = oid else { return .passthrough }
    guard let str = String(data: input, encoding: .utf8) else { return .passthrough }

    let needle = "$Id$"
    let replacement = "$Id: \(oid.hex) $"

    guard str.contains(needle) else { return .passthrough }

    let result = str.replacingOccurrences(of: needle, with: replacement)
    return .applied(Data(result.utf8))
}

/// Clean: Replace `$Id: <anything> $` back to `$Id$`
func identClean(_ input: Data) -> FilterResult {
    guard let str = String(data: input, encoding: .utf8) else { return .passthrough }
    guard str.contains("$Id:") else { return .passthrough }

    var output = ""
    var remaining = str[str.startIndex...]

    while let startRange = remaining.range(of: "$Id:") {
        output += remaining[remaining.startIndex..<startRange.lowerBound]
        let afterStart = remaining[startRange.upperBound...]

        if let endRange = afterStart.range(of: "$") {
            let content = afterStart[afterStart.startIndex..<endRange.lowerBound]
            if !content.contains("\n") {
                output += "$Id$"
                remaining = afterStart[endRange.upperBound...]
            } else {
                output += "$Id:"
                remaining = afterStart
            }
        } else {
            output += remaining[startRange.lowerBound...]
            remaining = remaining[remaining.endIndex...]
            break
        }
    }

    output += remaining

    let resultData = Data(output.utf8)
    if resultData == input {
        return .passthrough
    }
    return .applied(resultData)
}
