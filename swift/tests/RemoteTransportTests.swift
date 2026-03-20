import Foundation
import XCTest
@testable import MuonGit

final class RemoteTransportTests: XCTestCase {
    func testHTTPBasicCloneFetchPushRoundTrip() throws {
        try Self.requireTools(["git", "python3", "curl"])

        let root = Self.testDirectory("swift_remote_http_basic_round_trip")
        try? FileManager.default.removeItem(atPath: root)
        defer { try? FileManager.default.removeItem(atPath: root) }
        try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)

        let setup = try GitFixtureSetup(root: root)
        let fixture = try FixtureProcess.http(
            repo: setup.remoteGitDir,
            auth: "basic",
            username: "alice",
            secret: "s3cret"
        )
        defer { fixture.stop() }

        let clonePath = (root as NSString).appendingPathComponent("clone")
        let repo = try Repository.clone(
            from: fixture.url,
            to: clonePath,
            options: CloneOptions(
                transport: TransportOptions(auth: .basic(username: "alice", password: "s3cret"))
            )
        )

        XCTAssertEqual(try String(contentsOfFile: (clonePath as NSString).appendingPathComponent("hello.txt"), encoding: .utf8), "hello\n")
        XCTAssertEqual(try readReference(gitDir: repo.gitDir, name: "HEAD"), "ref: refs/heads/main")
        let initialOID = try setup.oidHex("refs/heads/main")
        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "refs/remotes/origin/main").hex, initialOID)

        try setup.commitAndPush(fileName: "hello.txt", contents: "hello remote\n", message: "remote update")
        let fetchedOID = try setup.oidHex("refs/heads/main")

        _ = try repo.fetch(
            remoteName: "origin",
            options: FetchOptions(transport: TransportOptions(auth: .basic(username: "alice", password: "s3cret")))
        )

        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "refs/remotes/origin/main").hex, fetchedOID)
        XCTAssertEqual(try String(contentsOfFile: (clonePath as NSString).appendingPathComponent("hello.txt"), encoding: .utf8), "hello\n")

        try Self.configureIdentity(at: clonePath)
        _ = try Self.runCommand("/usr/bin/git", ["checkout", "main"], cwd: clonePath)
        _ = try Self.runCommand("/usr/bin/git", ["reset", "--hard", "refs/remotes/origin/main"], cwd: clonePath)
        try "local push\n".write(toFile: (clonePath as NSString).appendingPathComponent("local.txt"), atomically: true, encoding: .utf8)
        _ = try Self.runCommand("/usr/bin/git", ["add", "local.txt"], cwd: clonePath)
        _ = try Self.runCommand("/usr/bin/git", ["commit", "-m", "local push"], cwd: clonePath)
        let pushedOID = try Self.runCommand("/usr/bin/git", ["rev-parse", "HEAD"], cwd: clonePath).trimmingCharacters(in: .whitespacesAndNewlines)

        _ = try repo.push(
            remoteName: "origin",
            options: PushOptions(transport: TransportOptions(auth: .basic(username: "alice", password: "s3cret")))
        )

        XCTAssertEqual(try setup.oidHex("refs/heads/main"), pushedOID)
        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "refs/remotes/origin/main").hex, pushedOID)
    }

    func testHTTPSBearerCloneSmoke() throws {
        try Self.requireTools(["git", "python3", "openssl", "curl"])

        let root = Self.testDirectory("swift_remote_https_bearer_clone")
        try? FileManager.default.removeItem(atPath: root)
        defer { try? FileManager.default.removeItem(atPath: root) }
        try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)

        let setup = try GitFixtureSetup(root: root)
        let cert = (root as NSString).appendingPathComponent("cert.pem")
        let key = (root as NSString).appendingPathComponent("key.pem")
        try Self.generateSelfSignedCert(certPath: cert, keyPath: key)
        let fixture = try FixtureProcess.http(
            repo: setup.remoteGitDir,
            auth: "bearer",
            secret: "top-secret-token",
            tls: true,
            cert: cert,
            key: key
        )
        defer { fixture.stop() }

        let clonePath = (root as NSString).appendingPathComponent("clone")
        let repo = try Repository.clone(
            from: fixture.url,
            to: clonePath,
            options: CloneOptions(
                transport: TransportOptions(
                    auth: .bearerToken("top-secret-token"),
                    insecureSkipTLSVerify: true
                )
            )
        )

        XCTAssertEqual(try String(contentsOfFile: (clonePath as NSString).appendingPathComponent("hello.txt"), encoding: .utf8), "hello\n")
        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "refs/remotes/origin/main").hex, try setup.oidHex("refs/heads/main"))
    }

    func testSSHKeyCloneAndPushRoundTrip() throws {
        try Self.requireTools(["git", "python3", "ssh", "ssh-keygen", "sshd"])

        let root = Self.testDirectory("swift_remote_ssh_clone_push")
        try? FileManager.default.removeItem(atPath: root)
        defer { try? FileManager.default.removeItem(atPath: root) }
        try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)

        let setup = try GitFixtureSetup(root: root)
        let clientKey = (root as NSString).appendingPathComponent("client_key")
        try Self.generateSSHKey(path: clientKey)
        let username: String
        if let user = ProcessInfo.processInfo.environment["USER"] {
            username = user
        } else {
            username = try Self.runCommand("/usr/bin/whoami", []).trimmingCharacters(in: .whitespacesAndNewlines)
        }

        let fixture = try FixtureProcess.ssh(
            repo: setup.remoteGitDir,
            stateDir: (root as NSString).appendingPathComponent("sshd"),
            authorizedKey: "\(clientKey).pub",
            username: username
        )
        defer { fixture.stop() }

        let clonePath = (root as NSString).appendingPathComponent("clone")
        let repo = try Repository.clone(
            from: fixture.url,
            to: clonePath,
            options: CloneOptions(
                transport: TransportOptions(
                    auth: .sshKey(username: username, privateKey: clientKey, strictHostKeyChecking: false)
                )
            )
        )

        XCTAssertEqual(try String(contentsOfFile: (clonePath as NSString).appendingPathComponent("hello.txt"), encoding: .utf8), "hello\n")

        try Self.configureIdentity(at: clonePath)
        _ = try Self.runCommand("/usr/bin/git", ["checkout", "main"], cwd: clonePath)
        try "ssh push\n".write(toFile: (clonePath as NSString).appendingPathComponent("ssh.txt"), atomically: true, encoding: .utf8)
        _ = try Self.runCommand("/usr/bin/git", ["add", "ssh.txt"], cwd: clonePath)
        _ = try Self.runCommand("/usr/bin/git", ["commit", "-m", "ssh push"], cwd: clonePath)
        let pushedOID = try Self.runCommand("/usr/bin/git", ["rev-parse", "HEAD"], cwd: clonePath).trimmingCharacters(in: .whitespacesAndNewlines)

        _ = try repo.push(
            remoteName: "origin",
            options: PushOptions(
                transport: TransportOptions(
                    auth: .sshKey(username: username, privateKey: clientKey, strictHostKeyChecking: false)
                )
            )
        )

        XCTAssertEqual(try setup.oidHex("refs/heads/main"), pushedOID)
    }

    private struct GitFixtureSetup {
        let remoteGitDir: String
        let seedWorkdir: String

        init(root: String) throws {
            remoteGitDir = (root as NSString).appendingPathComponent("remote.git")
            seedWorkdir = (root as NSString).appendingPathComponent("seed")

            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["init", "--bare", remoteGitDir], cwd: root)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["init", seedWorkdir], cwd: root)
            try RemoteTransportTests.configureIdentity(at: seedWorkdir)
            try "hello\n".write(toFile: (seedWorkdir as NSString).appendingPathComponent("hello.txt"), atomically: true, encoding: .utf8)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["add", "hello.txt"], cwd: seedWorkdir)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["commit", "-m", "initial"], cwd: seedWorkdir)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["branch", "-M", "main"], cwd: seedWorkdir)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["remote", "add", "origin", remoteGitDir], cwd: seedWorkdir)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["push", "origin", "main"], cwd: seedWorkdir)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["--git-dir", remoteGitDir, "symbolic-ref", "HEAD", "refs/heads/main"], cwd: root)
        }

        func commitAndPush(fileName: String, contents: String, message: String) throws {
            try contents.write(toFile: (seedWorkdir as NSString).appendingPathComponent(fileName), atomically: true, encoding: .utf8)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["add", fileName], cwd: seedWorkdir)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["commit", "-m", message], cwd: seedWorkdir)
            _ = try RemoteTransportTests.runCommand("/usr/bin/git", ["push", "origin", "main"], cwd: seedWorkdir)
            _ = try RemoteTransportTests.runCommand(
                "/usr/bin/git",
                ["--git-dir", remoteGitDir, "symbolic-ref", "HEAD", "refs/heads/main"],
                cwd: (remoteGitDir as NSString).deletingLastPathComponent
            )
        }

        func oidHex(_ refName: String) throws -> String {
            try RemoteTransportTests.runCommand(
                "/usr/bin/git",
                ["--git-dir", remoteGitDir, "rev-parse", refName],
                cwd: (remoteGitDir as NSString).deletingLastPathComponent
            ).trimmingCharacters(in: .whitespacesAndNewlines)
        }
    }

    private final class FixtureProcess {
        let process: Process
        let url: String

        private init(process: Process, url: String) {
            self.process = process
            self.url = url
        }

        static func http(
            repo: String,
            auth: String = "none",
            username: String? = nil,
            secret: String? = nil,
            tls: Bool = false,
            cert: String? = nil,
            key: String? = nil
        ) throws -> FixtureProcess {
            var args = [RemoteTransportTests.fixtureScript(), "serve-http", "--repo", repo]
            if auth != "none" {
                args += ["--auth", auth]
            }
            if let username {
                args += ["--username", username]
            }
            if let secret {
                args += ["--secret", secret]
            }
            if tls {
                args += ["--tls", "--cert", cert!, "--key", key!]
            }
            return try startFixture(args: args)
        }

        static func ssh(repo: String, stateDir: String, authorizedKey: String, username: String) throws -> FixtureProcess {
            try startFixture(args: [
                RemoteTransportTests.fixtureScript(),
                "serve-ssh",
                "--repo", repo,
                "--state-dir", stateDir,
                "--authorized-key", authorizedKey,
                "--username", username,
            ])
        }

        func stop() {
            if process.isRunning {
                process.terminate()
                process.waitUntilExit()
            }
        }

        private static func startFixture(args: [String]) throws -> FixtureProcess {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
            process.arguments = args
            let stdout = Pipe()
            process.standardOutput = stdout
            process.standardError = FileHandle.standardError
            try process.run()
            let line = try RemoteTransportTests.readLine(from: stdout.fileHandleForReading)
            guard let url = line.split(separator: "\"").dropFirst(3).first.map(String.init) else {
                throw NSError(
                    domain: "RemoteTransportTests",
                    code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "unexpected fixture output: \(line)"]
                )
            }
            return FixtureProcess(process: process, url: url)
        }
    }

    private static func requireTools(_ tools: [String]) throws {
        let missing = tools.filter { !toolAvailable($0) }
        if !missing.isEmpty {
            throw XCTSkip("missing tools: \(missing.joined(separator: ", "))")
        }
    }

    private static func toolAvailable(_ name: String) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/which")
        process.arguments = [name]
        process.standardOutput = Pipe()
        process.standardError = Pipe()
        do {
            try process.run()
            process.waitUntilExit()
            return process.terminationStatus == 0
        } catch {
            return false
        }
    }

    private static func configureIdentity(at repo: String) throws {
        _ = try runCommand("/usr/bin/git", ["config", "user.name", "MuonGit Test"], cwd: repo)
        _ = try runCommand("/usr/bin/git", ["config", "user.email", "muongit@example.com"], cwd: repo)
    }

    private static func generateSelfSignedCert(certPath: String, keyPath: String) throws {
        _ = try runCommand(
            "/usr/bin/openssl",
            ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", keyPath, "-out", certPath, "-days", "1", "-subj", "/CN=127.0.0.1"],
            allowFailure: false
        )
    }

    private static func generateSSHKey(path: String) throws {
        _ = try runCommand("/usr/bin/ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", path], allowFailure: false)
    }

    private static func runCommand(_ executable: String, _ arguments: [String], cwd: String? = nil, allowFailure: Bool = false) throws -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        if let cwd {
            process.currentDirectoryURL = URL(fileURLWithPath: cwd)
        }

        let stdout = Pipe()
        let stderr = Pipe()
        process.standardOutput = stdout
        process.standardError = stderr
        try process.run()
        process.waitUntilExit()

        let output = String(data: stdout.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let error = String(data: stderr.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        if !allowFailure && process.terminationStatus != 0 {
            throw NSError(
                domain: "RemoteTransportTests",
                code: Int(process.terminationStatus),
                userInfo: [NSLocalizedDescriptionKey: "\(executable) \(arguments.joined(separator: " ")) failed\n\(error)\n\(output)"]
            )
        }
        return process.terminationStatus == 0 ? output : ""
    }

    private static func readLine(from handle: FileHandle) throws -> String {
        var data = Data()
        while true {
            let chunk = try handle.read(upToCount: 1) ?? Data()
            if chunk.isEmpty {
                break
            }
            data.append(chunk)
            if chunk == Data([0x0a]) {
                break
            }
        }
        guard let line = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines), !line.isEmpty else {
            throw NSError(domain: "RemoteTransportTests", code: 2, userInfo: [NSLocalizedDescriptionKey: "fixture did not emit a ready line"])
        }
        return line
    }

    private static func testDirectory(_ name: String) -> String {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("tmp/\(name)")
            .path
    }

    private static func fixtureScript() -> String {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("scripts/git_remote_fixture.py")
            .path
    }
}
