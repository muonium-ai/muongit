#!/usr/bin/env python3

import argparse
import base64
import http.server
import json
import os
import signal
import socket
import ssl
import subprocess
import sys
import threading
import time
import urllib.parse
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    http_parser = subparsers.add_parser("serve-http")
    http_parser.add_argument("--repo", required=True)
    http_parser.add_argument("--host", default="127.0.0.1")
    http_parser.add_argument("--port", type=int, default=0)
    http_parser.add_argument("--auth", choices=["none", "basic", "bearer"], default="none")
    http_parser.add_argument("--username")
    http_parser.add_argument("--secret")
    http_parser.add_argument("--tls", action="store_true")
    http_parser.add_argument("--cert")
    http_parser.add_argument("--key")

    ssh_parser = subparsers.add_parser("serve-ssh")
    ssh_parser.add_argument("--repo", required=True)
    ssh_parser.add_argument("--state-dir", required=True)
    ssh_parser.add_argument("--authorized-key", required=True)
    ssh_parser.add_argument("--username", required=True)
    ssh_parser.add_argument("--host", default="127.0.0.1")
    ssh_parser.add_argument("--port", type=int, default=0)

    args = parser.parse_args()
    if args.command == "serve-http":
        return serve_http(args)
    if args.command == "serve-ssh":
        return serve_ssh(args)
    parser.error(f"unsupported command: {args.command}")
    return 2


def serve_http(args: argparse.Namespace) -> int:
    repo = Path(args.repo).resolve()
    if args.auth != "none" and not args.secret:
        raise SystemExit("--secret is required for authenticated HTTP fixtures")
    if args.auth == "basic" and not args.username:
        raise SystemExit("--username is required for basic auth fixtures")
    if args.tls and (not args.cert or not args.key):
        raise SystemExit("--cert and --key are required for TLS fixtures")

    expected_basic = None
    if args.auth == "basic":
        token = f"{args.username}:{args.secret}".encode("utf-8")
        expected_basic = "Basic " + base64.b64encode(token).decode("ascii")

    class GitHTTPRequestHandler(http.server.BaseHTTPRequestHandler):
        server_version = "MuonGitFixture/1.0"

        def do_GET(self) -> None:
            self.handle_backend()

        def do_POST(self) -> None:
            self.handle_backend()

        def log_message(self, fmt: str, *values) -> None:
            return

        def handle_backend(self) -> None:
            if not authorize_request(self):
                return

            request = urllib.parse.urlsplit(self.path)
            content_length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(content_length) if content_length > 0 else b""

            env = os.environ.copy()
            env.update(
                {
                    "GIT_PROJECT_ROOT": str(repo.parent),
                    "GIT_HTTP_EXPORT_ALL": "1",
                    "PATH_INFO": request.path,
                    "PATH_TRANSLATED": str(repo.parent / request.path.lstrip("/")),
                    "REQUEST_METHOD": self.command,
                    "QUERY_STRING": request.query,
                    "CONTENT_TYPE": self.headers.get("Content-Type", ""),
                    "CONTENT_LENGTH": str(content_length),
                    "REMOTE_ADDR": self.client_address[0],
                    "REMOTE_USER": args.username or "",
                    "REQUEST_URI": self.path,
                    "SERVER_PROTOCOL": self.request_version,
                }
            )

            result = subprocess.run(
                ["git", "http-backend"],
                input=body,
                capture_output=True,
                env=env,
                check=False,
            )
            if result.returncode != 0 and not result.stdout:
                message = result.stderr.decode("utf-8", errors="replace").strip() or "git http-backend failed"
                self.send_error(500, message)
                return

            status_code, headers, response_body = parse_cgi_response(result.stdout)
            self.send_response(status_code)
            sent_length = False
            for name, value in headers:
                lower = name.lower()
                if lower == "status":
                    continue
                if lower == "content-length":
                    sent_length = True
                self.send_header(name, value)
            if not sent_length:
                self.send_header("Content-Length", str(len(response_body)))
            self.end_headers()
            self.wfile.write(response_body)

    def authorize_request(handler: GitHTTPRequestHandler) -> bool:
        if args.auth == "none":
            return True

        header = handler.headers.get("Authorization")
        if args.auth == "basic" and header == expected_basic:
            return True
        if args.auth == "bearer" and header == f"Bearer {args.secret}":
            return True

        handler.send_response(401)
        if args.auth == "basic":
            handler.send_header("WWW-Authenticate", 'Basic realm="muongit"')
        else:
            handler.send_header("WWW-Authenticate", 'Bearer realm="muongit"')
        handler.send_header("Content-Length", "0")
        handler.end_headers()
        return False

    server = http.server.ThreadingHTTPServer((args.host, args.port), GitHTTPRequestHandler)
    if args.tls:
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(certfile=args.cert, keyfile=args.key)
        server.socket = context.wrap_socket(server.socket, server_side=True)

    scheme = "https" if args.tls else "http"
    url = f"{scheme}://{args.host}:{server.server_port}/{repo.name}"
    print(json.dumps({"url": url}), flush=True)

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


def serve_ssh(args: argparse.Namespace) -> int:
    repo = Path(args.repo).resolve()
    state_dir = Path(args.state_dir).resolve()
    state_dir.mkdir(parents=True, exist_ok=True)

    port = args.port or choose_port(args.host)
    host_key = state_dir / "ssh_host_ed25519_key"
    authorized_keys = state_dir / "authorized_keys"
    pid_file = state_dir / "sshd.pid"
    config_file = state_dir / "sshd_config"

    if not host_key.exists():
        subprocess.run(
            ["/usr/bin/ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(host_key)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    authorized_keys.write_text(Path(args.authorized_key).read_text(encoding="utf-8"), encoding="utf-8")

    config_file.write_text(
        "\n".join(
            [
                f"Port {port}",
                f"ListenAddress {args.host}",
                f"HostKey {host_key}",
                f"PidFile {pid_file}",
                f"AuthorizedKeysFile {authorized_keys}",
                "PasswordAuthentication no",
                "KbdInteractiveAuthentication no",
                "ChallengeResponseAuthentication no",
                "UsePAM no",
                "PubkeyAuthentication yes",
                "PermitRootLogin no",
                "StrictModes no",
                f"AllowUsers {args.username}",
                "LogLevel VERBOSE",
                "",
            ]
        ),
        encoding="utf-8",
    )

    child = subprocess.Popen(
        ["/usr/sbin/sshd", "-D", "-e", "-f", str(config_file)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    stop_event = threading.Event()

    def forward_stream(stream, target):
        try:
            for line in stream:
                if stop_event.is_set():
                    break
                target.write(line)
                target.flush()
        finally:
            stream.close()

    stderr_thread = threading.Thread(target=forward_stream, args=(child.stderr, sys.stderr), daemon=True)
    stderr_thread.start()

    if not wait_for_port(args.host, port, child):
        stop_event.set()
        stdout, stderr = child.communicate(timeout=1)
        message = stderr.strip() or stdout.strip() or "sshd failed to start"
        raise SystemExit(message)

    url = f"ssh://{args.username}@{args.host}:{port}{repo}"
    print(json.dumps({"url": url}), flush=True)

    def shutdown(*_args):
        stop_event.set()
        if child.poll() is None:
            child.terminate()

    signal.signal(signal.SIGTERM, shutdown)
    signal.signal(signal.SIGINT, shutdown)

    try:
        return child.wait()
    finally:
        stop_event.set()
        if child.poll() is None:
            child.kill()
        stderr_thread.join(timeout=1)


def parse_cgi_response(output: bytes):
    if b"\r\n\r\n" in output:
        header_block, body = output.split(b"\r\n\r\n", 1)
    elif b"\n\n" in output:
        header_block, body = output.split(b"\n\n", 1)
    else:
        raise SystemExit("invalid CGI response from git http-backend")

    status = 200
    headers = []
    for line in header_block.decode("utf-8", errors="replace").splitlines():
        if not line:
            continue
        name, _, value = line.partition(":")
        value = value.strip()
        if name.lower() == "status":
            status = int(value.split(" ", 1)[0])
        headers.append((name, value))
    return status, headers, body


def choose_port(host: str) -> int:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        sock.bind((host, 0))
        return sock.getsockname()[1]
    finally:
        sock.close()


def wait_for_port(host: str, port: int, child: subprocess.Popen) -> bool:
    deadline = time.time() + 5.0
    while time.time() < deadline:
        if child.poll() is not None:
            return False
        try:
            with socket.create_connection((host, port), timeout=0.2):
                return True
        except OSError:
            time.sleep(0.1)
    return False


if __name__ == "__main__":
    raise SystemExit(main())
