package ai.muonium.muongit

import java.io.File
import java.lang.ProcessBuilder
import kotlin.test.Test
import kotlin.test.assertEquals

class RemoteTransportTest {
    @Test
    fun testHTTPBasicCloneFetchPushRoundTrip() {
        if (!haveTools("git", "python3", "curl")) {
            return
        }

        val root = testDirectory("kotlin_remote_http_basic_round_trip")
        root.deleteRecursively()
        root.mkdirs()
        try {
            val setup = GitFixtureSetup(root)
            val fixture = FixtureProcess.http(
                repo = setup.remoteGitDir,
                auth = "basic",
                username = "alice",
                secret = "s3cret",
            )
            try {
                val cloneDir = File(root, "clone")
                val repo = Repository.clone(
                    fixture.url,
                    cloneDir.path,
                    CloneOptions(
                        transport = TransportOptions(
                            auth = RemoteAuth.Basic("alice", "s3cret")
                        )
                    )
                )

                assertEquals("hello\n", File(cloneDir, "hello.txt").readText())
                assertEquals("ref: refs/heads/main", readReference(repo.gitDir, "HEAD"))
                val initialOid = setup.oidHex("refs/heads/main")
                assertEquals(initialOid, resolveReference(repo.gitDir, "refs/remotes/origin/main").hex)

                setup.commitAndPush("hello.txt", "hello remote\n", "remote update")
                val fetchedOid = setup.oidHex("refs/heads/main")

                repo.fetch(
                    "origin",
                    FetchOptions(
                        transport = TransportOptions(
                            auth = RemoteAuth.Basic("alice", "s3cret")
                        )
                    )
                )

                assertEquals(fetchedOid, resolveReference(repo.gitDir, "refs/remotes/origin/main").hex)
                assertEquals("hello\n", File(cloneDir, "hello.txt").readText())

                configureIdentity(cloneDir)
                runCommand("/usr/bin/git", listOf("checkout", "main"), cwd = cloneDir)
                runCommand("/usr/bin/git", listOf("reset", "--hard", "refs/remotes/origin/main"), cwd = cloneDir)
                File(cloneDir, "local.txt").writeText("local push\n")
                runCommand("/usr/bin/git", listOf("add", "local.txt"), cwd = cloneDir)
                runCommand("/usr/bin/git", listOf("commit", "-m", "local push"), cwd = cloneDir)
                val pushedOid = runCommand("/usr/bin/git", listOf("rev-parse", "HEAD"), cwd = cloneDir).trim()

                repo.push(
                    "origin",
                    PushOptions(
                        transport = TransportOptions(
                            auth = RemoteAuth.Basic("alice", "s3cret")
                        )
                    )
                )

                assertEquals(pushedOid, setup.oidHex("refs/heads/main"))
                assertEquals(pushedOid, resolveReference(repo.gitDir, "refs/remotes/origin/main").hex)
            } finally {
                fixture.stop()
            }
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun testHTTPSBearerCloneSmoke() {
        if (!haveTools("git", "python3", "openssl", "curl")) {
            return
        }

        val root = testDirectory("kotlin_remote_https_bearer_clone")
        root.deleteRecursively()
        root.mkdirs()
        try {
            val setup = GitFixtureSetup(root)
            val cert = File(root, "cert.pem")
            val key = File(root, "key.pem")
            generateSelfSignedCert(cert, key)
            val fixture = FixtureProcess.http(
                repo = setup.remoteGitDir,
                auth = "bearer",
                secret = "top-secret-token",
                tls = true,
                cert = cert,
                key = key,
            )
            try {
                val cloneDir = File(root, "clone")
                val repo = Repository.clone(
                    fixture.url,
                    cloneDir.path,
                    CloneOptions(
                        transport = TransportOptions(
                            auth = RemoteAuth.BearerToken("top-secret-token"),
                            insecureSkipTLSVerify = true,
                        )
                    )
                )

                assertEquals("hello\n", File(cloneDir, "hello.txt").readText())
                assertEquals(setup.oidHex("refs/heads/main"), resolveReference(repo.gitDir, "refs/remotes/origin/main").hex)
            } finally {
                fixture.stop()
            }
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun testSSHKeyCloneAndPushRoundTrip() {
        if (!haveTools("git", "python3", "ssh", "ssh-keygen", "sshd")) {
            return
        }

        val root = testDirectory("kotlin_remote_ssh_clone_push")
        root.deleteRecursively()
        root.mkdirs()
        try {
            val setup = GitFixtureSetup(root)
            val clientKey = File(root, "client_key")
            generateSSHKey(clientKey)
            val username = System.getenv("USER") ?: runCommand("/usr/bin/whoami", emptyList()).trim()
            val fixture = FixtureProcess.ssh(
                repo = setup.remoteGitDir,
                stateDir = File(root, "sshd"),
                authorizedKey = File(root, "client_key.pub"),
                username = username,
            )
            try {
                val cloneDir = File(root, "clone")
                val repo = Repository.clone(
                    fixture.url,
                    cloneDir.path,
                    CloneOptions(
                        transport = TransportOptions(
                            auth = RemoteAuth.SshKey(
                                username = username,
                                privateKey = clientKey.path,
                                strictHostKeyChecking = false,
                            )
                        )
                    )
                )

                assertEquals("hello\n", File(cloneDir, "hello.txt").readText())

                configureIdentity(cloneDir)
                runCommand("/usr/bin/git", listOf("checkout", "main"), cwd = cloneDir)
                File(cloneDir, "ssh.txt").writeText("ssh push\n")
                runCommand("/usr/bin/git", listOf("add", "ssh.txt"), cwd = cloneDir)
                runCommand("/usr/bin/git", listOf("commit", "-m", "ssh push"), cwd = cloneDir)
                val pushedOid = runCommand("/usr/bin/git", listOf("rev-parse", "HEAD"), cwd = cloneDir).trim()

                repo.push(
                    "origin",
                    PushOptions(
                        transport = TransportOptions(
                            auth = RemoteAuth.SshKey(
                                username = username,
                                privateKey = clientKey.path,
                                strictHostKeyChecking = false,
                            )
                        )
                    )
                )

                assertEquals(pushedOid, setup.oidHex("refs/heads/main"))
            } finally {
                fixture.stop()
            }
        } finally {
            root.deleteRecursively()
        }
    }

    private class GitFixtureSetup(root: File) {
        val remoteGitDir = File(root, "remote.git")
        private val seedWorkdir = File(root, "seed")

        init {
            runCommand("/usr/bin/git", listOf("init", "--bare", remoteGitDir.path), cwd = root)
            runCommand("/usr/bin/git", listOf("init", seedWorkdir.path), cwd = root)
            configureIdentity(seedWorkdir)
            File(seedWorkdir, "hello.txt").writeText("hello\n")
            runCommand("/usr/bin/git", listOf("add", "hello.txt"), cwd = seedWorkdir)
            runCommand("/usr/bin/git", listOf("commit", "-m", "initial"), cwd = seedWorkdir)
            runCommand("/usr/bin/git", listOf("branch", "-M", "main"), cwd = seedWorkdir)
            runCommand("/usr/bin/git", listOf("remote", "add", "origin", remoteGitDir.path), cwd = seedWorkdir)
            runCommand("/usr/bin/git", listOf("push", "origin", "main"), cwd = seedWorkdir)
            runCommand("/usr/bin/git", listOf("--git-dir", remoteGitDir.path, "symbolic-ref", "HEAD", "refs/heads/main"), cwd = root)
        }

        fun commitAndPush(fileName: String, contents: String, message: String) {
            File(seedWorkdir, fileName).writeText(contents)
            runCommand("/usr/bin/git", listOf("add", fileName), cwd = seedWorkdir)
            runCommand("/usr/bin/git", listOf("commit", "-m", message), cwd = seedWorkdir)
            runCommand("/usr/bin/git", listOf("push", "origin", "main"), cwd = seedWorkdir)
            runCommand(
                "/usr/bin/git",
                listOf("--git-dir", remoteGitDir.path, "symbolic-ref", "HEAD", "refs/heads/main"),
                cwd = remoteGitDir.parentFile,
            )
        }

        fun oidHex(refName: String): String =
            runCommand(
                "/usr/bin/git",
                listOf("--git-dir", remoteGitDir.path, "rev-parse", refName),
                cwd = remoteGitDir.parentFile,
            ).trim()
    }

    private class FixtureProcess(
        private val process: Process,
        val url: String,
    ) {
        fun stop() {
            process.destroy()
            process.waitFor()
        }

        companion object {
            fun http(
                repo: File,
                auth: String = "none",
                username: String? = null,
                secret: String? = null,
                tls: Boolean = false,
                cert: File? = null,
                key: File? = null,
            ): FixtureProcess {
                val args = mutableListOf(
                    "/usr/bin/python3",
                    fixtureScript().path,
                    "serve-http",
                    "--repo",
                    repo.path,
                )
                if (auth != "none") {
                    args += listOf("--auth", auth)
                }
                if (username != null) {
                    args += listOf("--username", username)
                }
                if (secret != null) {
                    args += listOf("--secret", secret)
                }
                if (tls) {
                    args += listOf("--tls", "--cert", cert!!.path, "--key", key!!.path)
                }
                return start(args)
            }

            fun ssh(repo: File, stateDir: File, authorizedKey: File, username: String): FixtureProcess {
                return start(
                    mutableListOf(
                        "/usr/bin/python3",
                        fixtureScript().path,
                        "serve-ssh",
                        "--repo",
                        repo.path,
                        "--state-dir",
                        stateDir.path,
                        "--authorized-key",
                        authorizedKey.path,
                        "--username",
                        username,
                    )
                )
            }

            private fun start(args: List<String>): FixtureProcess {
                val process = ProcessBuilder(args)
                    .redirectError(ProcessBuilder.Redirect.INHERIT)
                    .start()
                val line = process.inputStream.bufferedReader().readLine()
                    ?: error("fixture did not emit a ready line")
                val url = line.split('"').getOrNull(3)
                    ?: error("unexpected fixture output: $line")
                return FixtureProcess(process, url)
            }
        }
    }

    companion object {
        private fun haveTools(vararg tools: String): Boolean {
            val missing = tools.filterNot(::toolAvailable)
            if (missing.isNotEmpty()) {
                println("Skipping remote transport test; missing tools: ${missing.joinToString(", ")}")
                return false
            }
            return true
        }

        private fun toolAvailable(name: String): Boolean {
            return try {
                val process = ProcessBuilder("/usr/bin/which", name).start()
                process.waitFor() == 0
            } catch (_: Exception) {
                false
            }
        }

        private fun configureIdentity(repoDir: File) {
            runCommand("/usr/bin/git", listOf("config", "user.name", "MuonGit Test"), cwd = repoDir)
            runCommand("/usr/bin/git", listOf("config", "user.email", "muongit@example.com"), cwd = repoDir)
        }

        private fun generateSelfSignedCert(cert: File, key: File) {
            runCommand(
                "/usr/bin/openssl",
                listOf("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key.path, "-out", cert.path, "-days", "1", "-subj", "/CN=127.0.0.1")
            )
        }

        private fun generateSSHKey(path: File) {
            runCommand("/usr/bin/ssh-keygen", listOf("-q", "-t", "ed25519", "-N", "", "-f", path.path))
        }

        private fun runCommand(executable: String, arguments: List<String>, cwd: File? = null): String {
            val process = ProcessBuilder(listOf(executable) + arguments)
                .directory(cwd)
                .start()
            val stdout = process.inputStream.bufferedReader().readText()
            val stderr = process.errorStream.bufferedReader().readText()
            check(process.waitFor() == 0) {
                "$executable ${arguments.joinToString(" ")} failed\n$stderr\n$stdout"
            }
            return stdout
        }

        private fun testDirectory(name: String): File =
            File(System.getProperty("user.dir")).resolve("../tmp/$name")

        private fun fixtureScript(): File =
            File(System.getProperty("user.dir")).resolve("../scripts/git_remote_fixture.py")
    }
}
