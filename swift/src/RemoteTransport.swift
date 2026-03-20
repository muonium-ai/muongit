import Foundation

public enum RemoteAuth: Sendable, Equatable {
    case none
    case basic(username: String, password: String)
    case bearerToken(String)
    case sshKey(username: String, privateKey: String, port: Int? = nil, strictHostKeyChecking: Bool = true)
    case sshAgent(username: String, port: Int? = nil, strictHostKeyChecking: Bool = true)
}

public struct TransportOptions: Sendable, Equatable {
    public var auth: RemoteAuth?
    public var insecureSkipTLSVerify: Bool

    public init(auth: RemoteAuth? = nil, insecureSkipTLSVerify: Bool = false) {
        self.auth = auth
        self.insecureSkipTLSVerify = insecureSkipTLSVerify
    }
}

struct ServiceAdvertisement {
    let refs: [RemoteRef]
    let capabilities: ServerCapabilities
}

private enum RemoteService {
    case uploadPack
    case receivePack

    var infoRefsService: String {
        switch self {
        case .uploadPack: return "git-upload-pack"
        case .receivePack: return "git-receive-pack"
        }
    }

    var requestContentType: String {
        switch self {
        case .uploadPack: return "application/x-git-upload-pack-request"
        case .receivePack: return "application/x-git-receive-pack-request"
        }
    }

    var resultContentType: String {
        switch self {
        case .uploadPack: return "application/x-git-upload-pack-result"
        case .receivePack: return "application/x-git-receive-pack-result"
        }
    }

    var commandName: String {
        switch self {
        case .uploadPack: return "git-upload-pack"
        case .receivePack: return "git-receive-pack"
        }
    }
}

private struct ParsedRemoteURL {
    let scheme: String
    let user: String?
    let host: String
    let port: Int?
    let path: String
}

func advertiseUploadPack(url: String, options: TransportOptions) throws -> ServiceAdvertisement {
    try advertiseRefs(url: url, service: .uploadPack, options: options)
}

func uploadPack(url: String, request: Data, options: TransportOptions) throws -> Data {
    try statelessRPC(url: url, service: .uploadPack, request: request, options: options)
}

func advertiseReceivePack(url: String, options: TransportOptions) throws -> ServiceAdvertisement {
    try advertiseRefs(url: url, service: .receivePack, options: options)
}

func receivePack(url: String, request: Data, options: TransportOptions) throws -> Data {
    try statelessRPC(url: url, service: .receivePack, request: request, options: options)
}

private func advertiseRefs(url: String, service: RemoteService, options: TransportOptions) throws -> ServiceAdvertisement {
    let parsed = try parseRemoteURL(url)
    let response: Data
    switch parsed.scheme {
    case "http", "https":
        response = try advertiseRefsHTTP(url: url, service: service, options: options)
    case "ssh":
        response = try advertiseRefsSSH(parsed: parsed, service: service, options: options)
    default:
        throw MuonGitError.invalidSpec("unsupported remote scheme '\(parsed.scheme)'")
    }

    let (decodedLines, _) = try unwrap(pktLineDecode(response))
    let lines = stripServicePreamble(decodedLines)
    let (refs, capabilities) = try unwrap(parseRefAdvertisement(lines))
    return ServiceAdvertisement(refs: refs, capabilities: capabilities)
}

private func stripServicePreamble(_ lines: [PktLine]) -> [PktLine] {
    guard lines.count >= 2 else { return lines }
    if case let .data(data) = lines[0],
       data.starts(with: Data("# service=".utf8)),
       case .flush = lines[1] {
        return Array(lines.dropFirst(2))
    }
    return lines
}

private func statelessRPC(url: String, service: RemoteService, request: Data, options: TransportOptions) throws -> Data {
    let parsed = try parseRemoteURL(url)
    switch parsed.scheme {
    case "http", "https":
        return try statelessRPCHTTP(url: url, service: service, request: request, options: options)
    case "ssh":
        return try statelessRPCSSH(parsed: parsed, service: service, request: request, options: options)
    default:
        throw MuonGitError.invalidSpec("unsupported remote scheme '\(parsed.scheme)'")
    }
}

private func advertiseRefsHTTP(url: String, service: RemoteService, options: TransportOptions) throws -> Data {
    var args = [
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--header",
        "Accept: application/x-\(service.infoRefsService)-advertisement",
    ]
    applyHTTPAuthArgs(&args, options: options)
    args.append("\(trimTrailingSlash(url))/info/refs?service=\(service.infoRefsService)")
    return try runCommand(executable: "/usr/bin/curl", arguments: args, input: nil)
}

private func statelessRPCHTTP(url: String, service: RemoteService, request: Data, options: TransportOptions) throws -> Data {
    var args = [
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--request",
        "POST",
        "--header",
        "Content-Type: \(service.requestContentType)",
        "--header",
        "Accept: \(service.resultContentType)",
        "--data-binary",
        "@-",
    ]
    applyHTTPAuthArgs(&args, options: options)
    args.append("\(trimTrailingSlash(url))\(serviceHTTPPath(service))")
    return try runCommand(executable: "/usr/bin/curl", arguments: args, input: request)
}

private func advertiseRefsSSH(parsed: ParsedRemoteURL, service: RemoteService, options: TransportOptions) throws -> Data {
    let remoteCommand = "\(service.commandName) --stateless-rpc --advertise-refs \(shellQuote(parsed.path))"
    return try runSSHCommand(parsed: parsed, options: options, remoteCommand: remoteCommand, input: nil)
}

private func statelessRPCSSH(parsed: ParsedRemoteURL, service: RemoteService, request: Data, options: TransportOptions) throws -> Data {
    let remoteCommand = "\(service.commandName) --stateless-rpc \(shellQuote(parsed.path))"
    return try runSSHCommand(parsed: parsed, options: options, remoteCommand: remoteCommand, input: request)
}

private func runSSHCommand(parsed: ParsedRemoteURL, options: TransportOptions, remoteCommand: String, input: Data?) throws -> Data {
    let sshConfig = try resolveSSHConfig(parsed: parsed, options: options)
    var args: [String] = ["-o", "BatchMode=yes"]
    if !sshConfig.strictHostKeyChecking {
        args.append(contentsOf: ["-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null"])
    }
    if let port = sshConfig.port {
        args.append(contentsOf: ["-p", String(port)])
    }
    if let privateKey = sshConfig.privateKey {
        args.append(contentsOf: ["-i", privateKey])
    }
    args.append(sshConfig.target)
    args.append(remoteCommand)
    return try runCommand(executable: "/usr/bin/ssh", arguments: args, input: input)
}

private func resolveSSHConfig(parsed: ParsedRemoteURL, options: TransportOptions) throws -> (target: String, port: Int?, privateKey: String?, strictHostKeyChecking: Bool) {
    switch options.auth {
    case nil, .some(.none):
        let target = parsed.user.map { "\($0)@\(parsed.host)" } ?? parsed.host
        return (target, parsed.port, nil, true)
    case let .some(.sshKey(username, privateKey, port, strictHostKeyChecking)):
        return ("\(username)@\(parsed.host)", port ?? parsed.port, privateKey, strictHostKeyChecking)
    case let .some(.sshAgent(username, port, strictHostKeyChecking)):
        return ("\(username)@\(parsed.host)", port ?? parsed.port, nil, strictHostKeyChecking)
    case .some(.basic), .some(.bearerToken):
        throw MuonGitError.auth("HTTP credentials cannot be used with ssh remotes")
    }
}

private func applyHTTPAuthArgs(_ args: inout [String], options: TransportOptions) {
    if options.insecureSkipTLSVerify {
        args.append("--insecure")
    }
    switch options.auth {
    case let .some(.basic(username, password)):
        args.append(contentsOf: ["--user", "\(username):\(password)"])
    case let .some(.bearerToken(token)):
        args.append(contentsOf: ["--header", "Authorization: Bearer \(token)"])
    default:
        break
    }
}

private func parseRemoteURL(_ url: String) throws -> ParsedRemoteURL {
    if !url.contains("://") {
        guard let colon = url.firstIndex(of: ":") else {
            throw MuonGitError.invalidSpec("invalid remote url '\(url)'")
        }
        let beforeColon = String(url[..<colon])
        let afterColon = String(url[url.index(after: colon)...])
        let (user, host) = parseUserHost(beforeColon)
        return ParsedRemoteURL(scheme: "ssh", user: user, host: host, port: nil, path: afterColon)
    }

    guard let schemeRange = url.range(of: "://") else {
        throw MuonGitError.invalidSpec("invalid remote url '\(url)'")
    }
    let scheme = String(url[..<schemeRange.lowerBound])
    let rest = String(url[schemeRange.upperBound...])
    let pathStart = rest.firstIndex(of: "/") ?? rest.endIndex
    let authority = String(rest[..<pathStart])
    let path = pathStart < rest.endIndex ? String(rest[pathStart...]) : "/"
    let userHost: (String?, String)
    if let at = authority.lastIndex(of: "@") {
        userHost = (String(authority[..<at]), String(authority[authority.index(after: at)...]))
    } else {
        userHost = (nil, authority)
    }
    let (host, port) = splitHostPort(userHost.1)
    return ParsedRemoteURL(scheme: scheme, user: userHost.0, host: host, port: port, path: path)
}

private func parseUserHost(_ text: String) -> (String?, String) {
    if let at = text.firstIndex(of: "@") {
        return (String(text[..<at]), String(text[text.index(after: at)...]))
    }
    return (nil, text)
}

private func splitHostPort(_ authority: String) -> (String, Int?) {
    if let colon = authority.lastIndex(of: ":") {
        let host = String(authority[..<colon])
        let portText = String(authority[authority.index(after: colon)...])
        if let port = Int(portText), portText.allSatisfy(\.isNumber) {
            return (host, port)
        }
    }
    return (authority, nil)
}

private func trimTrailingSlash(_ url: String) -> String {
    var value = url
    while value.hasSuffix("/") {
        value.removeLast()
    }
    return value
}

private func serviceHTTPPath(_ service: RemoteService) -> String {
    switch service {
    case .uploadPack: return "/git-upload-pack"
    case .receivePack: return "/git-receive-pack"
    }
}

private func shellQuote(_ value: String) -> String {
    "'\(value.replacingOccurrences(of: "'", with: "'\\''"))'"
}

private func unwrap<T>(_ result: Result<T, MuonGitError>) throws -> T {
    switch result {
    case let .success(value): return value
    case let .failure(error): throw error
    }
}

#if os(macOS)
private func runCommand(executable: String, arguments: [String], input: Data?) throws -> Data {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: executable)
    process.arguments = arguments

    let stdoutPipe = Pipe()
    let stderrPipe = Pipe()
    process.standardOutput = stdoutPipe
    process.standardError = stderrPipe

    let stdinPipe: Pipe?
    if input != nil {
        let pipe = Pipe()
        process.standardInput = pipe
        stdinPipe = pipe
    } else {
        stdinPipe = nil
    }

    try process.run()

    if let input, let stdinPipe {
        stdinPipe.fileHandleForWriting.write(input)
        stdinPipe.fileHandleForWriting.closeFile()
    }

    let output = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
    let errorData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()

    guard process.terminationStatus == 0 else {
        let message = String(data: errorData, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "command failed"
        let lower = message.lowercased()
        if lower.contains("certificate") {
            throw MuonGitError.certificate(message)
        }
        if lower.contains("auth") || lower.contains("permission denied") || lower.contains("unauthorized") {
            throw MuonGitError.auth(message)
        }
        throw MuonGitError.invalid(message)
    }

    return output
}
#else
private func runCommand(executable: String, arguments: [String], input: Data?) throws -> Data {
    _ = executable
    _ = arguments
    _ = input
    throw MuonGitError.invalid("remote transport requires process support on this platform")
}
#endif
