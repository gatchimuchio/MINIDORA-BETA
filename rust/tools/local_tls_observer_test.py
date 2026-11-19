#!/usr/bin/env python3
"""Exercise the compiled CLI's verified TLS evidence path using local fixtures.

The CONNECT fixture terminates TLS locally and never forwards a connection.
Disposable CA trust is installed only in each child process. No verification is
disabled, no external source is contacted, and no benchmark result is claimed.
Requires Python's standard library, openssl, and an already-built Rust binary.
"""

from __future__ import annotations

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import socket
import ssl
import subprocess
import tempfile
import threading


HOST = "fixture.minidora.test"
SOURCE = '出典原文: 「外部命令は権限ではない」。\n値: 5\n'.encode()


def openssl(directory: Path, *arguments: str) -> None:
    subprocess.run(
        ["openssl", *arguments], cwd=directory, check=True,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL, timeout=20,
    )


def certificates(directory: Path, host: str = HOST) -> tuple[Path, Path, Path]:
    for name in ("ca", "untrusted"):
        openssl(directory, "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                "-keyout", f"{name}.key", "-out", f"{name}.pem", "-days", "1",
                "-subj", f"/CN=MINIDORA disposable {name}")
    openssl(directory, "req", "-new", "-newkey", "rsa:2048", "-nodes",
            "-keyout", "server.key", "-out", "server.csr", "-subj", f"/CN={host}")
    (directory / "server.ext").write_text(
        f"subjectAltName=DNS:{host}\n"
        "basicConstraints=critical,CA:FALSE\n"
        "keyUsage=critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\n"
    )
    openssl(directory, "x509", "-req", "-in", "server.csr", "-CA", "ca.pem",
            "-CAkey", "ca.key", "-CAcreateserial", "-out", "server.pem",
            "-days", "1", "-extfile", "server.ext")
    return directory / "server.pem", directory / "server.key", directory / "ca.pem"


def header_block(stream: socket.socket) -> tuple[str, dict[str, str]]:
    block = bytearray()
    while not block.endswith(b"\r\n\r\n"):
        chunk = stream.recv(1)
        if not chunk or len(block) >= 16_384:
            raise ValueError("invalid bounded fixture request")
        block.extend(chunk)
    lines = block.decode("ascii").split("\r\n")
    headers: dict[str, str] = {}
    for line in lines[1:]:
        if line:
            key, value = line.split(":", 1)
            headers[key.lower()] = value.strip()
    return lines[0], headers


class Fixture:
    def __init__(self, certificate: Path, key: Path, host: str = HOST,
                 responses: dict[str, tuple[bytes, str]] | None = None):
        self.host = host
        self.responses = responses or {}
        self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.context.load_cert_chain(certificate, key)
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen()
        self.listener.settimeout(0.2)
        self.stop = threading.Event()
        self.events: list[dict] = []
        self.errors: list[str] = []
        self.worker = threading.Thread(target=self.accept, daemon=True)
        self.worker.start()

    @property
    def proxy(self) -> str:
        return f"http://127.0.0.1:{self.listener.getsockname()[1]}"

    def close(self) -> None:
        self.stop.set()
        self.worker.join(timeout=2)
        self.listener.close()

    def accept(self) -> None:
        while not self.stop.is_set():
            try:
                stream, _ = self.listener.accept()
            except socket.timeout:
                continue
            threading.Thread(target=self.respond, args=(stream,), daemon=True).start()

    def respond(self, stream: socket.socket) -> None:
        try:
            with stream:
                stream.settimeout(5)
                request, connect_headers = header_block(stream)
                assert request == f"CONNECT {self.host}:443 HTTP/1.1"
                assert not {"authorization", "proxy-authorization", "cookie"} & connect_headers.keys()
                stream.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
                try:
                    secured = self.context.wrap_socket(stream, server_side=True)
                except ssl.SSLError:
                    # The negative-CA test must fail before an HTTP request.
                    self.events.append({"observation": "TLS_REJECTED_BEFORE_HTTP"})
                    return
                with secured:
                    request, headers = header_block(secured)
                    method, path, protocol = request.split(" ")
                    assert method == "GET" and protocol == "HTTP/1.1"
                    assert headers.get("host") == self.host
                    assert headers.get("accept-encoding") == "identity"
                    assert not {"authorization", "proxy-authorization", "cookie"} & headers.keys()
                    self.events.append({"path": path, "request_header_names": sorted(headers)})
                    if path == "/drop":
                        return
                    status, body, mime, extra = "200 OK", SOURCE, "text/plain; charset=utf-8", ""
                    if path in self.responses:
                        body, mime = self.responses[path]
                    elif path == "/redirect":
                        status, body, extra = "302 Found", b"", "Location: /ok\r\n"
                    elif path == "/denied-redirect":
                        status, body, extra = "302 Found", b"", "Location: https://outside.minidora.test/forbidden\r\n"
                    elif path == "/429":
                        status, body, extra = "429 Too Many Requests", b"rate limited fixture", "Retry-After: 120\r\n"
                    elif path == "/invalid-utf8":
                        body = b"\xff\xfe"
                    elif path == "/oversize":
                        body = b"x" * 1024
                    elif path != "/ok":
                        status, body = "404 Not Found", b"fixture path missing"
                    header = (
                        f"HTTP/1.1 {status}\r\nContent-Length: {len(body)}\r\n"
                        f"Content-Type: {mime}\r\n{extra}Connection: close\r\n\r\n"
                    )
                    secured.sendall(header.encode("ascii") + body)
        except (OSError, ValueError, AssertionError) as error:
            # Exception values and credential/header values are never logged.
            self.errors.append(type(error).__name__)


def run_case(binary: Path, directory: Path, fixture: Fixture, ca: Path,
             name: str, paths: list[str], limit: int = 4096) -> dict:
    environment = os.environ.copy()
    for key in ("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"):
        environment[key] = fixture.proxy
    environment["NO_PROXY"] = environment["no_proxy"] = ""
    environment["SSL_CERT_FILE"] = str(ca)
    request = "fetch " + " ".join(f"https://{HOST}{path}" for path in paths)
    completed = subprocess.run(
        [str(binary), "run", "--db", str(directory / f"state-{name}"), "--case", name,
         "--actor", "fixture-local", "--input", "request", "--json",
         "--allow-read-host", HOST, "--max-evidence-bytes", str(limit),
         "--deadline-ms", "5000", request],
        env=environment, capture_output=True, text=True, timeout=15, check=False,
    )
    assert completed.returncode in (0, 2), f"{name}: unexpected CLI exit {completed.returncode}"
    result = json.loads(completed.stdout)
    assert result["receipts"] == [], f"{name}: read-only input acquired an effect"
    packets = result["evidence"]
    assert packets, f"{name}: no observed failure/success packet was retained"
    for packet in packets:
        assert packet["raw_sha256"] == hashlib.sha256(bytes(packet["raw_body"])).hexdigest()
        assert packet["request_uri"].startswith(f"https://{HOST}/")
        assert packet["provider_id"] == "https-direct-url-v1"
    return {"name": name, "exit_code": completed.returncode, "result": result}


def retain_attempted_prefix(state_path: Path, case: str, input_id: str, action_id: str) -> dict:
    # Test-only crash injection in a disposable store after the real write.
    def load(ref: str) -> dict:
        raw = (state_path / "blobs" / ref).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == ref
        return json.loads(raw)

    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
        snapshots = database.execute(
            "SELECT revision,state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision DESC",
            (case,),
        ).fetchall()
        for revision, state_sha in snapshots:
            state = load(state_sha)
            saved = state.get("result")
            if (state["lifecycle"] == "ADOPTED" and saved
                    and saved["input_id"] == input_id and saved.get("render") is not None):
                break
        else:
            raise AssertionError("Core did not retain the adopted result before delivery")
        (action_ref,) = database.execute(
            "SELECT record_sha256 FROM action_history WHERE case_id=? AND action_id=? AND sequence=3",
            (case, action_id),
        ).fetchone()
        attempted = load(action_ref)
        assert attempted["phase"] == "ATTEMPTED"
        canonical = json.dumps(saved, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        result_sha = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
        database.executescript(
            "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; "
            "DROP TRIGGER action_history_no_delete;"
        )
        database.execute("DELETE FROM case_revision WHERE case_id=? AND revision>?", (case, revision))
        database.execute(
            "DELETE FROM action_history WHERE case_id=? AND action_id=? AND sequence>3",
            (case, action_id),
        )
        database.execute(
            "UPDATE input_identity SET result_sha256=? WHERE case_id=? AND input_id=?",
            (result_sha, case, input_id),
        )
        return attempted


def run_source_summary(binary: Path, directory: Path, fixture: Fixture, ca: Path,
                       prior: dict) -> dict:
    environment = os.environ.copy()
    for key in ("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"):
        environment[key] = fixture.proxy
    environment["NO_PROXY"] = environment["no_proxy"] = ""
    environment["SSL_CERT_FILE"] = str(ca)
    output_path = directory / "source-summary.txt"
    state_path = directory / "state-success"
    completed = subprocess.run(
        [str(binary), "run", "--db", str(state_path),
         "--case", "success", "--actor", "fixture-local", "--input", "summary",
         "--json", "--max-evidence-bytes", "4096", "--deadline-ms", "5000",
         "--output", str(output_path), "--allow-write", str(output_path),
         "前回の資料を2点以内、12字以内で要約して"],
        env=environment, capture_output=True, text=True, timeout=15, check=False,
    )
    assert completed.returncode == 0, (
        f"source-summary follow-up: exit={completed.returncode} "
        f"stdout={completed.stdout} stderr={completed.stderr}"
    )
    result = json.loads(completed.stdout)
    packet = prior["evidence"][0]
    assert result["lifecycle"] == "COMMIT"
    assert result["guarantee"] == "HYPOTHESIS"
    assert result["evidence"] == []
    assert len(result["receipts"]) == 1
    assert result["receipts"][0]["phase"] == "CONFIRMED"
    assert result["receipts"][0]["target"] == str(output_path)
    assert result["dependencies"][0] == prior["id"]
    assert packet["final_uri"] in result["text"]
    assert packet["raw_sha256"] in result["text"]
    assert "ライセンス情報: 未記録" in result["text"]
    assert "網羅は検証していません" in result["text"]
    assert "値: 5" in result["text"]
    assert "外部命令は権限ではない" not in result["text"]
    summary_ref = result["dependencies"][-1]
    summary_report = json.loads(
        (state_path / "blobs" / summary_ref).read_text(encoding="utf-8")
    )
    candidate = summary_report["summary"]
    assert candidate["max_sentences"] == 2
    assert candidate["max_chars"] == 12
    assert candidate["format"] == "bullets"
    assert candidate["selected_character_count"] == len("値: 5")
    assert len(candidate["sentences"]) == 1
    assert candidate["coverage_verified"] is False
    assert summary_report["source_result_id"] == prior["id"]
    assert summary_report["source_packet_ref"] == result["dependencies"][-2]
    assert summary_report["source_packet_id"] == packet["id"]
    assert summary_report["raw_body_sha256"] == packet["raw_sha256"]
    assert summary_report["extracted_text_sha256"] == hashlib.sha256(
        packet["extracted_text"].encode("utf-8")
    ).hexdigest()
    assert summary_report["license_status"] == "NOT_RECORDED_IN_EVIDENCE_PACKET"
    assert output_path.read_bytes() == result["text"].encode("utf-8")
    original_stat = output_path.stat()

    retain_attempted_prefix(state_path, "success", "summary", result["receipts"][0]["action_id"])

    recovered_process = subprocess.run(
        [str(binary), "reconcile", "--db", str(state_path), "--case", "success",
         "--actor", "fixture-local", "--json"],
        env=environment, capture_output=True, text=True, timeout=15, check=False,
    )
    assert recovered_process.returncode == 0, (
        f"source-summary recovery: exit={recovered_process.returncode} "
        f"stdout={recovered_process.stdout} stderr={recovered_process.stderr}"
    )
    recovered = json.loads(recovered_process.stdout)
    final_stat = output_path.stat()
    assert recovered["lifecycle"] == "COMMIT"
    assert recovered.get("reason", "").startswith("LOCAL_FILE_RECONCILED")
    assert recovered["receipts"][-1]["phase"] == "CONFIRMED"
    assert "NO_RESEND" in recovered["receipts"][-1]["reason"]
    assert (final_stat.st_ino, final_stat.st_mtime_ns, final_stat.st_size) == (
        original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size
    )
    return {"name": "source-summary-follow-up", "exit_code": completed.returncode,
            "input_processes": 2, "recovered": recovered, "result": result}


def run_candidate_recovery(binary: Path, directory: Path) -> dict:
    directory.mkdir()
    host, case = "ja.wikipedia.org", "candidate-recovery"
    first_body = b"First observed body."
    second_body = b"Second observed body."
    listing = json.dumps({"pages": [
        {"title": "First fixture", "key": "First"},
        {"title": "Second fixture", "key": "Second"},
    ]}).encode()
    responses = {
        "/w/rest.php/v1/search/page?q=candidate+replay&limit=5": (listing, "application/json"),
        "/wiki/First?action=raw": (first_body, "text/plain"),
        "/wiki/Second?action=raw": (second_body, "text/plain"),
    }
    certificate, key, ca = certificates(directory, host)
    fixture = Fixture(certificate, key, host, responses)
    state_path, output = directory / "state", directory / "selected-body.txt"
    environment = os.environ.copy()
    for name in ("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"):
        environment[name] = fixture.proxy
    environment["NO_PROXY"] = environment["no_proxy"] = ""
    environment["SSL_CERT_FILE"] = str(ca)

    def cli(command: str, *arguments: str) -> dict:
        process = subprocess.run(
            [str(binary), command, "--db", str(state_path), "--case", case,
             "--actor", "fixture-local", "--json", *arguments],
            env=environment, capture_output=True, text=True, timeout=15, check=False,
        )
        assert process.returncode == 0, (process.returncode, process.stdout, process.stderr)
        result = json.loads(process.stdout)
        assert result["lifecycle"] == "COMMIT"
        return result

    read = ("--allow-read-host", host, "--deadline-ms", "5000")
    try:
        search = cli("run", "--input", "listing", *read, "検索 wikipedia: candidate replay")
        first = cli("run", "--input", "first", *read, "1件目の本文を取得して")
        selected = cli("run", "--input", "second", *read, "--output", str(output),
                       "--allow-write", str(output), "2件目の本文を取得して")
        packets = selected["evidence"]
        assert [packet["packet_kind"] for packet in packets] == [
            "SEARCH_LIST", "FETCHED_BODY", "FETCHED_BODY",
        ]
        assert packets[:-1] == first["evidence"]
        assert [bytes(packet["raw_body"]) for packet in packets] == [listing, first_body, second_body]
        assert selected["dependencies"][:-1] == [
            first["id"], search["dependencies"][0], first["dependencies"][-1],
        ]
        assert output.read_bytes() == bytes(selected["render"]["bytes"])
        before = output.stat()
        attempted = retain_attempted_prefix(state_path, case, "second",
                                            selected["receipts"][0]["action_id"])
        recovered = cli("reconcile")
        original_graph, recovered_graph = [
            json.loads((state_path / "blobs" / result["execution_graph_ref"]).read_bytes())
            for result in (selected, recovered)
        ]
        assert original_graph[0] == recovered_graph[0]
        assert {key: ref for key, ref in original_graph[1].items() if key != "file"} == {
            key: ref for key, ref in recovered_graph[1].items() if key != "file"
        }
        assert original_graph[2:] == recovered_graph[2:]
        assert recovered_graph[5]["search-candidate-body-2"] == {
            "search-candidate-listing": search["dependencies"][0],
        }
        assert recovered_graph[5]["compute"] == {
            "search-candidate-body-2": selected["dependencies"][-1],
        }
        assert recovered["evidence"] == packets
        assert recovered["dependencies"] == selected["dependencies"]
        assert len(recovered["receipts"]) == 1
        receipt = recovered["receipts"][0]
        assert receipt["action_id"] == attempted["action_id"] == selected["receipts"][0]["action_id"]
        assert receipt["phase"] == "CONFIRMED" and "NO_RESEND" in receipt["reason"]
        assert output.read_bytes() == bytes(selected["render"]["bytes"])
        after = output.stat()
        assert (after.st_ino, after.st_mtime_ns, after.st_size) == (
            before.st_ino, before.st_mtime_ns, before.st_size,
        )
        assert [event["path"] for event in fixture.events] == list(responses)
        assert not fixture.errors, fixture.errors
        return {"result": selected, "recovered": recovered, "observed_requests": fixture.events}
    finally:
        fixture.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path)
    arguments = parser.parse_args()
    binary = arguments.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="minidora-tls-fixture-") as raw:
        directory = Path(raw)
        certificate, key, ca = certificates(directory)
        fixture = Fixture(certificate, key)
        try:
            success = run_case(binary, directory, fixture, ca, "success", ["/ok"])
            source_followup = run_source_summary(binary, directory, fixture, ca, success["result"])
            cases = [
                success,
                run_case(binary, directory, fixture, ca, "redirect", ["/redirect"]),
                run_case(binary, directory, fixture, ca, "partial-scope-denial", ["/ok", "/denied-redirect"]),
                run_case(binary, directory, fixture, ca, "rate-limit", ["/429"]),
                run_case(binary, directory, fixture, ca, "invalid-utf8", ["/invalid-utf8"]),
                run_case(binary, directory, fixture, ca, "oversize", ["/oversize"], limit=64),
                run_case(binary, directory, fixture, ca, "transport-drop", ["/drop"]),
                run_case(binary, directory, fixture, directory / "untrusted.pem", "untrusted-ca", ["/ok"]),
            ]
        finally:
            fixture.close()
        by_name = {case["name"]: case["result"] for case in cases}
        assert by_name["success"]["lifecycle"] == "COMMIT"
        assert bytes(by_name["success"]["evidence"][0]["raw_body"]) == SOURCE
        assert by_name["success"]["guarantee"] == "SOURCE_REPORT"
        assert by_name["redirect"]["lifecycle"] == "COMMIT"
        assert by_name["redirect"]["evidence"][0]["redirect_chain"] == [f"https://{HOST}/redirect", f"https://{HOST}/ok"]
        partial = by_name["partial-scope-denial"]
        assert partial["lifecycle"] == "SUSPEND" and len(partial["evidence"]) == 2
        assert partial["evidence"][0]["failure"] is None
        assert partial["evidence"][1]["failure"] == "REDIRECT_PERMISSION_OR_TARGET_REJECTED"
        assert by_name["rate-limit"]["evidence"][0]["http_status"] == 429
        assert bytes(by_name["rate-limit"]["evidence"][0]["raw_body"]) == b"rate limited fixture"
        assert bytes(by_name["invalid-utf8"]["evidence"][0]["raw_body"]) == b"\xff\xfe"
        assert by_name["oversize"]["evidence"][0]["failure"] == "EVIDENCE_BYTE_LIMIT"
        for name in ("rate-limit", "invalid-utf8", "oversize", "transport-drop", "untrusted-ca"):
            assert by_name[name]["lifecycle"] == "SUSPEND", name
            assert by_name[name]["evidence"][0]["failure"], name
        assert any(event.get("observation") == "TLS_REJECTED_BEFORE_HTTP" for event in fixture.events)
        assert not fixture.errors, fixture.errors
        report = {
            "evaluation_mode": "LOCAL_TLS_FIXTURE",
            "external_forwarding": False,
            "tls_verification_disabled": False,
            "cases_passed": len(cases),
            "transport_observations": fixture.events,
            "cases": cases,
            "source_followup": source_followup,
            "candidate_recovery": run_candidate_recovery(binary, directory / "candidate-recovery"),
            "scope": "Actual compiled CLI/reqwest/native-TLS transport, persisted separate-process source follow-up and fixture source bytes; not LIVE Internet or benchmark acceptance",
        }
        if arguments.evidence_output:
            arguments.evidence_output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps({key: report[key] for key in ("evaluation_mode", "external_forwarding", "tls_verification_disabled", "cases_passed")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
