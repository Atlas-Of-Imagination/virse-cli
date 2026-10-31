#!/usr/bin/env python3
"""Check CLI request binding against local aesthetic fixtures, without backend calls."""

import argparse
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading


EXPECTED_ACTIONS = {
    "create", "details", "save-resource", "references", "test-canvas",
    "delete-test-canvas", "start-draft", "draft", "context", "revise",
    "save-draft", "restore", "snapshot", "train",
}


class VerificationError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise VerificationError(message)


def load_fixtures():
    source = Path(__file__).with_name("aesthetic-contract-source.json")
    fixtures = json.loads(source.read_text(encoding="utf-8"))["contracts"]
    require(len(fixtures) == 14, "Expected exactly 14 request fixtures")
    require({item["action"] for item in fixtures} == EXPECTED_ACTIONS,
            "Fixture actions are missing, duplicated, or unexpected")
    return fixtures


def handler_class(records):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def respond(self):
            length = int(self.headers.get("Content-Length", "0"))
            if length > 1024 * 1024:
                self.send_error(413)
                return
            raw = self.rfile.read(length)
            records.append({
                "method": self.command,
                "path": self.path,
                "body": json.loads(raw) if raw else None,
                "key": self.headers.get("Idempotency-Key"),
            })
            response = b'{"status":1,"fixture":true}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

        do_GET = respond
        do_POST = respond

    return Handler


def command_for(fixture, org, canvas):
    if fixture["action"] == "train":
        return [
            "aesthetic", "train", "--org", fixture["org"],
            "--canvas", fixture["canvas"], "--idempotency-key", fixture["key"],
            "--data", json.dumps(fixture["body"], ensure_ascii=False), "--json",
        ]
    # Supplying global scope to every call proves it is not injected into its body.
    body = fixture["body"]
    if fixture["method"] == "GET":
        # The CLI accepts JSON here, but the SDK must drop it for GET operations.
        body = {"ignored_fixture_field": "GET must send no body"}
    command = [
        "aesthetic", "call", fixture["action"], "--org", org, "--canvas", canvas,
        "--data", json.dumps(body, ensure_ascii=False), "--json",
    ]
    if "id" in fixture:
        command.extend(["--id", fixture["id"]])
    return command


def verify(binary, fixtures):
    train = next(item for item in fixtures if item["action"] == "train")
    records = []
    with tempfile.TemporaryDirectory(prefix="virse-aesthetic-loopback-") as raw_dir:
        workdir = Path(raw_dir).resolve()
        config = workdir / "config"
        config.mkdir()
        # Remove inherited Virse configuration and use a synthetic token only.
        env = {key: value for key, value in os.environ.items()
               if not key.startswith("VIRSE_")}
        env.update({
            "VIRSE_API_TOKEN": "virse_sk_loopback_fixture",
            "VIRSE_CONFIG_DIR": str(config),
            "NO_PROXY": "localhost,127.0.0.1,::1",
            "no_proxy": "localhost,127.0.0.1,::1",
        })
        server = HTTPServer(("127.0.0.1", 0), handler_class(records))
        env["VIRSE_BACKEND_URL"] = f"http://127.0.0.1:{server.server_port}"
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            for fixture in fixtures:
                action = fixture["action"]
                before = len(records)
                command = [str(binary), *command_for(fixture, train["org"], train["canvas"])]
                result = subprocess.run(command, env=env, cwd=workdir,
                                        capture_output=True, text=True, timeout=15)
                require(result.returncode == 0,
                        f"{action}: CLI exit {result.returncode}: {result.stderr.strip()}")
                require(len(records) == before + 1,
                        f"{action}: expected one request, got {len(records) - before}")
                observed = records[-1]
                expected = {
                    "method": fixture["method"],
                    "path": fixture["path"],
                    "body": fixture.get("wire_body", fixture["body"]),
                    "key": fixture.get("key"),
                }
                for field in expected:
                    require(observed[field] == expected[field],
                            f"{action}: {field} mismatch; expected {expected[field]!r}, "
                            f"got {observed[field]!r}")
                print(f"PASS {action}: method/path/body/key")
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
    print("PASS 14/14: call scope stays outside JSON; GET has no body; train wraps org/request")
    print("Loopback request binding only; no authenticated backend or model execution verified.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path,
                        help="Explicit path to the Virse CLI executable")
    args = parser.parse_args()
    binary = args.binary.expanduser().resolve()
    try:
        require(binary.is_file() and os.access(binary, os.X_OK),
                f"CLI binary is not an executable file: {binary}")
        verify(binary, load_fixtures())
    except (VerificationError, OSError, ValueError, KeyError,
            subprocess.TimeoutExpired) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
