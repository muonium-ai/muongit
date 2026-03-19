// Transport.swift - Git smart protocol and transport abstractions
// Parity: libgit2 src/libgit2/transports/smart_pkt.c

import Foundation

// MARK: - Pkt-line encoding/decoding

/// Encode data as a pkt-line with 4-hex-digit length prefix.
public func pktLineEncode(_ data: Data) -> Data {
    let len = data.count + 4
    var out = String(format: "%04x", len).data(using: .ascii)!
    out.append(data)
    return out
}

/// Encode a flush packet (0000).
public func pktLineFlush() -> Data {
    return "0000".data(using: .ascii)!
}

/// Encode a delimiter packet (0001).
public func pktLineDelim() -> Data {
    return "0001".data(using: .ascii)!
}

/// Decoded pkt-line.
public enum PktLine: Equatable {
    case data(Data)
    case flush
    case delim
}

/// Parse pkt-lines from a byte buffer.
/// Returns the parsed lines and the number of bytes consumed.
public func pktLineDecode(_ input: Data) -> Result<([PktLine], Int), MuonGitError> {
    var lines: [PktLine] = []
    var pos = 0

    while pos + 4 <= input.count {
        guard let hex = String(data: input[pos..<pos+4], encoding: .ascii) else {
            return .failure(.invalidObject("invalid pkt-line header"))
        }

        if hex == "0000" {
            lines.append(.flush)
            pos += 4
            continue
        }
        if hex == "0001" {
            lines.append(.delim)
            pos += 4
            continue
        }

        guard let len = UInt(hex, radix: 16) else {
            return .failure(.invalidObject("invalid pkt-line length"))
        }
        let length = Int(len)

        if length < 4 {
            return .failure(.invalidObject("pkt-line length too small"))
        }

        if pos + length > input.count {
            break // Incomplete packet
        }

        let data = input[pos+4..<pos+length]
        lines.append(.data(Data(data)))
        pos += length
    }

    return .success((lines, pos))
}

// MARK: - Smart protocol reference advertisement

/// A remote reference from the smart protocol handshake.
public struct RemoteRef: Equatable {
    public let oid: OID
    public let name: String
}

/// Server capabilities from the reference advertisement.
public struct ServerCapabilities {
    public var capabilities: [String]

    public init(capabilities: [String] = []) {
        self.capabilities = capabilities
    }

    public func has(_ cap: String) -> Bool {
        return capabilities.contains { $0 == cap || $0.hasPrefix("\(cap)=") }
    }

    public func get(_ cap: String) -> String? {
        let prefix = "\(cap)="
        guard let found = capabilities.first(where: { $0.hasPrefix(prefix) }) else {
            return nil
        }
        return String(found.dropFirst(prefix.count))
    }
}

/// Parse the reference advertisement from the smart protocol v1 response.
public func parseRefAdvertisement(_ lines: [PktLine]) -> Result<([RemoteRef], ServerCapabilities), MuonGitError> {
    var refs: [RemoteRef] = []
    var caps = ServerCapabilities()

    for (i, line) in lines.enumerated() {
        switch line {
        case .flush:
            break
        case .delim:
            continue
        case .data(let data):
            guard var text = String(data: data, encoding: .utf8) else { continue }
            // Trim trailing newline
            if text.hasSuffix("\n") {
                text = String(text.dropLast())
            }

            // Skip comment lines
            if text.hasPrefix("#") {
                continue
            }

            // First ref line may contain capabilities after NUL
            let refPart: String
            let capPart: String?

            if let nulIndex = text.firstIndex(of: "\0") {
                refPart = String(text[text.startIndex..<nulIndex])
                capPart = String(text[text.index(after: nulIndex)...])
            } else {
                refPart = text
                capPart = nil
            }

            // Parse capabilities from first line
            if i == 0 || caps.capabilities.isEmpty {
                if let capStr = capPart {
                    caps.capabilities = capStr.split(separator: " ")
                        .map(String.init)
                        .filter { !$0.isEmpty }
                }
            }

            // Parse ref: "<oid> <refname>"
            if refPart.count >= 41 {
                let idx40 = refPart.index(refPart.startIndex, offsetBy: 40)
                if refPart[idx40] == " " {
                    let hex = String(refPart[refPart.startIndex..<idx40])
                    let name = String(refPart[refPart.index(after: idx40)...])
                    if hex.count == 40 {
                        let oid = OID(hex: hex)
                        refs.append(RemoteRef(oid: oid, name: name))
                    }
                }
            }
        }
    }

    return .success((refs, caps))
}

/// Build a want/have negotiation request for fetch.
public func buildWantHave(wants: [OID], haves: [OID], caps: [String]) -> Data {
    var out = Data()

    for (i, want) in wants.enumerated() {
        let line: String
        if i == 0 && !caps.isEmpty {
            line = "want \(want.hex) \(caps.joined(separator: " "))\n"
        } else {
            line = "want \(want.hex)\n"
        }
        out.append(pktLineEncode(line.data(using: .utf8)!))
    }

    out.append(pktLineFlush())

    for have in haves {
        let line = "have \(have.hex)\n"
        out.append(pktLineEncode(line.data(using: .utf8)!))
    }

    out.append(pktLineEncode("done\n".data(using: .utf8)!))
    out.append(pktLineFlush())

    return out
}

/// Parse a URL into (scheme, host, path).
public func parseGitURL(_ url: String) -> (scheme: String, host: String, path: String)? {
    // Handle SSH shorthand: user@host:path
    if !url.contains("://") {
        if let colonIdx = url.firstIndex(of: ":") {
            let beforeColon = url[url.startIndex..<colonIdx]
            if beforeColon.contains("@") {
                let host = String(beforeColon)
                let path = String(url[url.index(after: colonIdx)...])
                return ("ssh", host, path)
            }
        }
        return nil
    }

    guard let schemeEnd = url.range(of: "://") else { return nil }
    let scheme = String(url[url.startIndex..<schemeEnd.lowerBound])
    let rest = String(url[schemeEnd.upperBound...])

    let pathStart = rest.firstIndex(of: "/") ?? rest.endIndex
    let host = String(rest[rest.startIndex..<pathStart])
    let path: String
    if pathStart < rest.endIndex {
        path = String(rest[pathStart...])
    } else {
        path = "/"
    }

    return (scheme, host, path)
}
