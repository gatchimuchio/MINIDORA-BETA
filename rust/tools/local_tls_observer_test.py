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
from functools import partial
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
import time


HOST = "fixture.minidora.test"
SOURCE = '出典原文: 「外部命令は権限ではない」。\n値: 5\n'.encode()
UNFRAMED_BODY = b"0123456789abcdef" * 8
TIMED_BODY_PREFIX = b"received before deadline"
RETRY_DATE = "Fri, 01 Jan 2100 00:00:00 GMT"


def openssl(directory: Path, *arguments: str) -> None:
    subprocess.run(
        ["openssl", *arguments], cwd=directory, check=True,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL, timeout=20,
    )


def certificates(directory: Path, host: str = HOST,
                 additional_hosts: tuple[str, ...] = ()) -> tuple[Path, Path, Path]:
    for name in ("ca", "untrusted"):
        openssl(directory, "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                "-keyout", f"{name}.key", "-out", f"{name}.pem", "-days", "1",
                "-subj", f"/CN=MINIDORA disposable {name}")
    openssl(directory, "req", "-new", "-newkey", "rsa:2048", "-nodes",
            "-keyout", "server.key", "-out", "server.csr", "-subj", f"/CN={host}")
    subject_names = ",".join(f"DNS:{name}" for name in (host, *additional_hosts))
    (directory / "server.ext").write_text(
        f"subjectAltName={subject_names}\n"
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
                 responses: dict[str, tuple[bytes, str]] | None = None,
                 additional_hosts: tuple[str, ...] = ()):
        self.host = host
        self.hosts = (host, *additional_hosts)
        self.responses = responses or {}
        self.redirects: dict[str, str] = {}
        self.response_gates: dict[str, threading.Event] = {}
        self.failure_responses: dict[str, tuple[str, bytes, str]] = {}
        self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.context.load_cert_chain(certificate, key)
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen()
        self.listener.settimeout(0.2)
        self.stop = threading.Event()
        self.connects: list[str] = []
        self.events: list[dict] = []
        self.errors: list[str] = []
        self.worker = threading.Thread(target=self.accept, daemon=True)
        self.worker.start()

    @property
    def proxy(self) -> str:
        return f"http://127.0.0.1:{self.listener.getsockname()[1]}"

    def close(self) -> None:
        self.stop.set()
        for gate in self.response_gates.values():
            gate.set()
        self.worker.join(timeout=2)
        self.listener.close()

    def wait_for_request(self, process, seen: int, expected: tuple[str, str]) -> None:
        limit = time.monotonic() + 5
        while expected not in [(event["host"], event["path"]) for event in self.events[seen:]]:
            assert process.poll() is None and time.monotonic() < limit
            time.sleep(0.01)

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
                host = next((name for name in self.hosts if request == f"CONNECT {name}:443 HTTP/1.1"), None)
                assert host is not None
                assert not {"authorization", "proxy-authorization", "cookie"} & connect_headers.keys()
                self.connects.append(request)
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
                    assert headers.get("host") == host
                    assert headers.get("accept-encoding") == "identity"
                    assert not {"authorization", "proxy-authorization", "cookie"} & headers.keys()
                    self.events.append({"host": host, "path": path, "request_header_names": sorted(headers)})
                    if (gate := self.response_gates.get(path)) is not None:
                        assert gate.wait(10)
                    if path == "/drop":
                        return
                    if path == "/unframed":
                        secured.sendall(
                            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n"
                            + UNFRAMED_BODY,
                        )
                        return
                    if path == "/slow-body":
                        secured.sendall(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 128\r\n"
                            b"Content-Type: text/plain\r\nConnection: close\r\n\r\n"
                            + TIMED_BODY_PREFIX,
                        )
                        self.stop.wait(3)
                        return
                    status, body, mime, extra = "200 OK", SOURCE, "text/plain; charset=utf-8", ""
                    if path in self.failure_responses:
                        status, body, extra = self.failure_responses[path]
                    elif path in self.redirects:
                        status, body = "302 Found", b""
                        extra = f"Location: {self.redirects[path]}\r\n"
                    elif path in self.responses:
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
             name: str, paths: list[str], limit: int = 4096, *,
             actions: int = 32, deadline_ms: int = 5000) -> dict:
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
         "--max-actions", str(actions), "--deadline-ms", str(deadline_ms), request],
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


def check_retrieval_budget(binary: Path, directory: Path, fixture: Fixture, ca: Path) -> list[dict]:
    cases = []
    seen = len(fixture.events)
    starts = len(fixture.connects)
    for actions, expected_exit, paths in (
        (5, 2, ["/redirect", "/ok"]),
        (6, 0, ["/redirect", "/ok", "/budget-second"]),
    ):
        case = run_case(binary, directory, fixture, ca, f"combined-starts-{actions}",
                        ["/redirect", "/budget-second"], actions=actions)
        cases.append(case)
        assert case["exit_code"] == expected_exit
        result = case["result"]
        assert [event["path"] for event in fixture.events[seen:]] == paths
        assert len(fixture.connects) == starts + len(paths)
        assert bytes(result["evidence"][0]["raw_body"]) == SOURCE
        if expected_exit == 2:
            assert "HTTP_START_BUDGET_EXHAUSTED" in result["reason"]
            assert len(result["evidence"]) == 1 and result["content_plan"] is None
        else:
            assert result["lifecycle"] == "COMMIT" and len(result["evidence"]) == 2
        seen = len(fixture.events)
        starts = len(fixture.connects)

    for name, second, options, prefix, failure in (
        ("combined-bytes", "/unframed", {"limit": len(SOURCE) + 32},
         UNFRAMED_BODY[:32], "EVIDENCE_BYTE_LIMIT_PARTIAL_BODY"),
        ("combined-deadline", "/slow-body", {"deadline_ms": 1200},
         TIMED_BODY_PREFIX, "HTTP_DEADLINE_EXCEEDED"),
    ):
        case = run_case(binary, directory, fixture, ca, name, ["/ok", second, "/budget-sentinel"],
                        **options)
        cases.append(case)
        assert case["exit_code"] == 2
        result = case["result"]
        assert result["lifecycle"] == "SUSPEND" and result["content_plan"] is None
        first, failed = result["evidence"]
        assert bytes(first["raw_body"]) == SOURCE and first["failure"] is None
        assert bytes(failed["raw_body"]) == prefix and failed["failure"] == failure, failed
        assert failed["extracted_text"] is None
        assert [event["path"] for event in fixture.events[seen:]] == ["/ok", second]
        assert len(fixture.connects) == starts + 2
        seen = len(fixture.events)
        starts = len(fixture.connects)
    return cases


def check_retry_after(binary: Path, directory: Path, fixture: Fixture, ca: Path) -> list[dict]:
    fixture.failure_responses.update({
        "/429-missing": ("429 Too Many Requests", b"without directive", ""),
        "/429-duplicate": ("429 Too Many Requests", b"duplicate directive",
                           "Retry-After: 1\r\nRetry-After: 2\r\n"),
        "/429-huge": ("429 Too Many Requests", b"unbounded directive", "Retry-After: 1234567890123\r\n"),
        "/503-date": ("503 Service Unavailable", b"temporary failure", f"Retry-After: {RETRY_DATE}\r\n"),
        "/503-plain": ("503 Service Unavailable", b"plain failure", ""),
    })
    cases = []
    for name, status, directive, delay in (
        ("429", 429, {"kind": "DELAY_SECONDS", "seconds": 120}, 120_000),
        ("429-missing", 429, {"kind": "HEADER_MISSING"}, 60_000),
        ("429-duplicate", 429, {"kind": "MULTIPLE_HEADERS"}, 60_000),
        ("429-huge", 429, {"kind": "DELAY_OUT_OF_RANGE"}, None),
        ("503-date", 503, {"kind": "HTTP_DATE", "value": RETRY_DATE}, None),
        ("503-plain", 503, None, None),
    ):
        path = f"/{name}"
        seen, starts = len(fixture.events), len(fixture.connects)
        case = run_case(binary, directory, fixture, ca, f"retry-after-{name}", [path, "/ok"])
        cases.append(case)
        result = case["result"]
        first = result["evidence"][0]
        assert case["exit_code"] == 2 and result["lifecycle"] == "SUSPEND"
        assert result["content_plan"] is None and result["render"] is None
        assert first["http_status"] == status and first["packet_kind"] == "FETCH_FAILURE"
        assert first["failure"] == ("HTTP_RATE_LIMITED" if status == 429 else "HTTP_STATUS_FAILURE")
        if directive is None:
            assert "retry_after" not in first and len(result["evidence"]) == 2
            assert result["evidence"][1]["failure"] is None
            assert [event["path"] for event in fixture.events[seen:]] == [path, "/ok"]
            assert len(fixture.connects) == starts + 2
            continue
        assert "HTTP_RETRY_DEFERRED" in result["reason"] and len(result["evidence"]) == 1
        assert [event["path"] for event in fixture.events[seen:]] == [path]
        assert len(fixture.connects) == starts + 1
        retry = first["retry_after"]
        assert retry["directive"] == directive
        observed = retry["received_at_unix_ms"]
        assert observed <= first["observed_at_unix_ms"]
        if name == "503-date":
            assert retry["not_before_unix_ms"] == 4_102_444_800_000
        elif delay is not None:
            assert retry["not_before_unix_ms"] == observed + delay
        else:
            assert retry["not_before_unix_ms"] is None
        dependency = result["dependencies"][0]
        assert dependency == first["id"]

    seen, starts = len(fixture.events), len(fixture.connects)
    limited = run_case(binary, directory, fixture, ca, "retry-after-body-cap", ["/429", "/ok"], limit=8)
    cases.append(limited)
    (first,) = limited["result"]["evidence"]
    assert first["failure"] == "EVIDENCE_BYTE_LIMIT" and first["raw_body"] == []
    assert first["retry_after"]["directive"] == {"kind": "DELAY_SECONDS", "seconds": 120}
    assert [event["path"] for event in fixture.events[seen:]] == ["/429"]
    assert len(fixture.connects) == starts + 1
    return cases


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


def check_search_goals(cli, fixture: Fixture, state_path: Path, directory: Path) -> dict:
    query = "検索 wikipedia: candidate replay"
    grant = ("--allow-read-host", fixture.host)
    output = directory / "listing.txt"
    write = ("--output", str(output), "--allow-write", str(output))
    initial_connects = len(fixture.connects)
    initial_gets = len(fixture.events)
    for case, reason, flags in (
        ("display-budget", "ACTION_BUDGET_EXHAUSTED", ("--max-actions", "3", *grant)),
        ("file-budget", "ACTION_BUDGET_EXHAUSTED", ("--max-actions", "4", *grant, *write)),
        ("read-grant", "READ_PERMISSION_REQUIRED", ("--max-actions", "4")),
        ("deadline", "RESOURCE_LIMIT", ("--max-actions", "4", "--deadline-ms", "0", *grant)),
    ):
        stopped = cli("run", "--input", "search", *flags, query, case_id=case, expected_exit=2)
        assert reason in stopped["reason"]
        assert stopped["content_plan"] is None and stopped["render"] is None
        assert stopped["evidence"] == [] and stopped["receipts"] == [] and not output.exists()
    assert len(fixture.connects) == initial_connects and len(fixture.events) == initial_gets

    def load(ref: str):
        raw = (state_path / "blobs" / ref).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == ref
        return json.loads(raw)

    failures = {}
    for case, bad_query, status, reason in (
        ("malformed-list", "malformed fixture", 200, "MALFORMED_SEARCH_LIST"),
        ("http-failure", "missing fixture", 404, "SEARCH_LIST_FETCH_FAILED"),
    ):
        failed = cli("run", "--input", "search", "--max-actions", "5", *grant, *write,
                     f"検索 wikipedia: {bad_query}", case_id=case, expected_exit=2)
        assert failed["content_plan"] is None and failed["render"] is None
        assert failed["receipts"] == [] and not output.exists()
        assert failed["execution_graph_ref"] is None
        (reference,) = failed["dependencies"]
        (listing,) = failed["evidence"]
        observation = load(reference)
        assert observation["failure"] == reason and observation["listing"] == listing
        assert listing["packet_kind"] == "SEARCH_LIST" and listing["http_status"] == status
        assert listing["extracted_text"] is None
        assert hashlib.sha256(bytes(listing["raw_body"])).hexdigest() == listing["raw_sha256"]
        failures[case] = failed

    display = cli("run", "--input", "search", "--max-actions", "4", *grant, query,
                  case_id="listing-display")
    original = cli("run", "--input", "search", "--max-actions", "5", *grant, *write, query,
                   case_id="listing-file")
    for result in (display, original):
        (reference,) = result["dependencies"]
        (listing,) = result["evidence"]
        assert result["guarantee"] == "SOURCE_REPORT" and listing["packet_kind"] == "SEARCH_LIST"
        assert listing["extracted_text"] is None
        observation = load(reference)
        assert observation["listing"] == listing and observation["failure"] is None
        assert [candidate["rank"] for candidate in observation["candidates"]] == [1, 2]
        graph = load(result["execution_graph_ref"])
        assert graph[0][0] == {"id": "search-listing", "kind": "EVIDENCE", "requires": []}
        assert graph[1]["search-listing"] == reference
        assert graph[5]["compute"] == {"search-listing": reference}
    before = output.stat()
    attempted = retain_attempted_prefix(state_path, "listing-file", "search",
                                        original["receipts"][0]["action_id"])
    recovered = cli("reconcile", case_id="listing-file")
    assert recovered["dependencies"] == original["dependencies"]
    assert recovered["evidence"] == original["evidence"]
    assert recovered["content_plan"] == original["content_plan"]
    (receipt,) = recovered["receipts"]
    assert receipt["action_id"] == attempted["action_id"]
    assert receipt["phase"] == "CONFIRMED" and "NO_RESEND" in receipt["reason"]
    graph = load(recovered["execution_graph_ref"])
    assert graph[5]["compute"] == {"search-listing": original["dependencies"][0]}
    assert load(graph[1]["file"]) == receipt
    assert output.read_bytes() == bytes(original["render"]["bytes"])
    after = output.stat()
    assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)
    assert len(fixture.connects) == initial_connects + 4
    assert len(fixture.events) == initial_gets + 4
    redirect = "/w/rest.php/v1/search/page?q=redirect+fixture&limit=5"
    listing_path = "/w/rest.php/v1/search/page?q=candidate+replay&limit=5"
    fixture.redirects[redirect] = listing_path
    fixture.redirects["/wiki/Second?action=raw"] = "/candidate-body-copy"
    fixture.responses["/candidate-body-copy"] = fixture.responses["/wiki/Second?action=raw"]
    transport_start = len(fixture.events)
    connects_start = len(fixture.connects)
    search_request = "検索 wikipedia: redirect fixture"
    low_search = cli("run", "--input", "search", "--max-actions", "4", *grant,
                     search_request, case_id="listing-redirect-low", expected_exit=2)
    (failed_listing,) = low_search["evidence"]
    assert failed_listing["failure"] == "HTTP_START_BUDGET_EXHAUSTED"
    assert failed_listing["redirect_chain"] == [f"https://{fixture.host}{redirect}"]
    high_search = cli("run", "--input", "search", "--max-actions", "5", *grant,
                      search_request, case_id="listing-redirect-ok")
    assert high_search["evidence"][0]["redirect_chain"] == [
        f"https://{fixture.host}{redirect}", f"https://{fixture.host}{listing_path}",
    ]
    body_request = "2件目の本文を取得して"
    low_body = cli("run", "--input", "body", "--max-actions", "5", *grant, body_request,
                   case_id="listing-redirect-ok", expected_exit=2)
    assert low_body["evidence"][0] == high_search["evidence"][0]
    assert low_body["evidence"][-1]["failure"] == "HTTP_START_BUDGET_EXHAUSTED"
    high_body = cli("run", "--input", "body", "--max-actions", "6", *grant, body_request,
                    case_id="listing-display")
    assert high_body["evidence"][-1]["final_uri"] == f"https://{fixture.host}/candidate-body-copy"
    denied_body = cli("run", "--input", "body", body_request,
                      case_id="listing-file", expected_exit=2)
    assert "READ_PERMISSION_REQUIRED" in denied_body["reason"]
    assert denied_body["evidence"] == original["evidence"]
    assert denied_body["content_plan"] is None and denied_body["receipts"] == []
    assert len(fixture.connects) == connects_start + 6
    assert [event["path"] for event in fixture.events[transport_start:]] == [
        redirect, redirect, listing_path, "/wiki/Second?action=raw",
        "/wiki/Second?action=raw", "/candidate-body-copy",
    ]
    seen, starts = len(fixture.events), len(fixture.connects)
    busy_listing = "/w/rest.php/v1/search/page?q=busy+fixture&limit=5"
    busy_body = "/wiki/First?action=raw"
    fixture.failure_responses[busy_listing] = (
        "429 Too Many Requests", b"listing temporarily unavailable", "Retry-After: 120\r\n",
    )
    fixture.failure_responses[busy_body] = (
        "503 Service Unavailable", b"body temporarily unavailable", "Retry-After: 120\r\n",
    )
    deferred_search = cli("run", "--input", "busy-search", *grant, "検索 wikipedia: busy fixture",
                          case_id="listing-rate-limited", expected_exit=2)
    deferred_body = cli("run", "--input", "busy-body", *grant, "1件目の本文を取得して",
                        case_id="listing-display", expected_exit=2)
    (listing,) = deferred_search["evidence"]
    assert listing["packet_kind"] == "SEARCH_LIST" and listing["http_status"] == 429
    observation = load(deferred_search["dependencies"][0])
    assert observation["listing"] == listing and observation["failure"] == "SEARCH_LIST_FETCH_FAILED"
    assert deferred_body["evidence"][:-1] == high_body["evidence"]
    body = deferred_body["evidence"][-1]
    assert body["packet_kind"] == "FETCH_FAILURE" and body["http_status"] == 503
    assert load(deferred_body["dependencies"][-1]) == body
    for packet in (listing, body):
        retry = packet["retry_after"]
        assert retry["directive"] == {"kind": "DELAY_SECONDS", "seconds": 120}
        assert retry["not_before_unix_ms"] == retry["received_at_unix_ms"] + 120_000
    assert len(fixture.connects) == starts + 2
    assert [event["path"] for event in fixture.events[seen:]] == [busy_listing, busy_body]
    assert not fixture.errors, fixture.errors
    return {"display": display, "file": original, "recovered": recovered, "failures": failures,
            "redirect_budget": [low_search, high_search, low_body, high_body],
            "retry_after": [deferred_search, deferred_body]}


def check_retry_continuation(cli, fixture: Fixture, search_failure: dict) -> dict:
    host = fixture.host
    grant = ("--allow-read-host", host)
    resource = f"fetch https://{host}/wiki/First?action=raw"
    # The earlier check temporarily made this candidate a 503 fixture.
    fixture.failure_responses.pop("/wiki/First?action=raw")
    fixture.failure_responses.update({
        "/retry-persisted": ("429 Too Many Requests", b"receive and defer", "Retry-After: 120\r\n"),
        "/retry-expiring": ("429 Too Many Requests", b"short deferral", "Retry-After: 0\r\n"),
        "/retry-unbounded": ("429 Too Many Requests", b"oversized deferral", "Retry-After: 1234567890123\r\n"),
    })
    seen, starts = len(fixture.events), len(fixture.connects)
    first_request = f"fetch https://{host}/retry-persisted"
    first = cli("run", "--input", "rate-limit", *grant, first_request,
                case_id="prior-rate-limit", expected_exit=2)
    (first_packet,) = first["evidence"]
    # The wait comes from authenticated history, even after an unrelated goal.
    unrelated = cli("run", "--input", "unrelated", "1 + 2", case_id="prior-rate-limit")
    assert unrelated["calculation"]["exact_value"] == "3"
    old_listing = search_failure["evidence"][0]
    for case, command, evidence in (
        ("prior-rate-limit", resource, first_packet),
        ("listing-rate-limited", "検索 wikipedia: candidate replay", old_listing),
    ):
        for input_id in ("followup", "another"):
            blocked = cli("run", "--input", input_id, *grant, command,
                          case_id=case, expected_exit=2)
            assert "HTTP_RETRY_DEFERRED" in blocked["reason"] and evidence["id"] in blocked["reason"]
            assert blocked["evidence"] == [] and blocked["content_plan"] is None
            assert blocked["render"] is None and blocked["receipts"] == []
    ungranted = cli("run", "--input", "no-grant", resource,
                    case_id="prior-rate-limit", expected_exit=2)
    assert "READ_PERMISSION_REQUIRED" in ungranted["reason"]
    assert first_packet["id"] not in ungranted["reason"]
    unowned = cli("run", "--input", "foreign", *grant, resource,
                  case_id="prior-rate-limit", actor_id="fixture-other", expected_exit=3)
    assert "CASE_ACCESS_DENIED" in unowned["reason"] and unowned["evidence"] == []
    assert first_packet["id"] not in unowned["reason"]
    replayed = cli("run", "--input", "rate-limit", *grant, first_request,
                   case_id="prior-rate-limit", expected_exit=2)
    assert replayed == first
    assert len(fixture.connects) == starts + 1
    assert [event["path"] for event in fixture.events[seen:]] == ["/retry-persisted"]

    for actor, case in (("fixture-local", "separate-case"), ("fixture-other", "foreign-case")):
        accepted = cli("run", "--input", "retrieve", *grant, resource, case_id=case, actor_id=actor)
        assert accepted["evidence"][0]["failure"] is None
    for path, case in (("retry-expiring", "expired-case"), ("retry-unbounded", "unbounded-case")):
        stopped = cli("run", "--input", "initial", *grant, f"fetch https://{host}/{path}",
                      case_id=case, expected_exit=2)
        due = stopped["evidence"][0]["retry_after"]["not_before_unix_ms"]
        if path == "retry-expiring":
            assert due is not None
            time.sleep(max(0, due / 1000 - time.time()) + 0.02)
        else:
            assert due is None
        accepted = cli("run", "--input", "fresh", *grant, resource, case_id=case)
        assert accepted["evidence"][0]["failure"] is None
    assert len(fixture.connects) == starts + 7
    assert [event["path"] for event in fixture.events[seen:]] == [
        "/retry-persisted", "/wiki/First?action=raw", "/wiki/First?action=raw",
        "/retry-expiring", "/wiki/First?action=raw", "/retry-unbounded", "/wiki/First?action=raw",
    ]
    assert not fixture.errors, fixture.errors
    return {"source": first, "source_replayed_exactly": replayed == first,
            "blocked_connections": 0, "case_isolation": True}


def check_ordered_search(cli, fixture: Fixture, state_path: Path, directory: Path) -> dict:
    primary, fallback = fixture.hosts
    grants = ("--allow-read-host", primary, "--allow-read-host", fallback)
    wiki = "/w/rest.php/v1/search/page?"
    crossref = "/works?"
    primary_busy = wiki + "q=alternate+trial&limit=5"
    alternative = crossref + "query=alternate+trial&rows=5"
    body_url = f"https://{fallback}/selected-article"
    winner_listing = json.dumps({"message": {"items": [
        {"title": ["Alternative fixture"], "URL": body_url, "DOI": "10.5555/fixture"},
    ]}}).encode()
    selected_body = b"Exact body from the selected alternative."
    fixture.failure_responses[primary_busy] = (
        "429 Too Many Requests", b"primary rate limited", "Retry-After: 120\r\n",
    )
    fixture.responses[alternative] = (winner_listing, "application/json")
    fixture.responses["/selected-article"] = (selected_body, "text/plain")
    fixture.responses[wiki + "q=alternate+empty&limit=5"] = (b'{"pages":[]}', "application/json")
    fixture.responses[wiki + "q=alternate+malformed&limit=5"] = (b'{"pages":false}', "application/json")
    denied_redirect = wiki + "q=alternate+redirect&limit=5"
    fixture.redirects[denied_redirect] = "https://outside.minidora.test/forbidden"
    unavailable = wiki + "q=alternate+unavailable&limit=5"
    unavailable_alternate = crossref + "query=alternate+unavailable&rows=5"
    fixture.failure_responses[unavailable] = ("503 Service Unavailable", b"primary unavailable", "")
    fixture.responses[unavailable_alternate] = (winner_listing, "application/json")
    both_failed = crossref + "query=alternate+failed&rows=5"
    fixture.failure_responses[wiki + "q=alternate+failed&limit=5"] = (
        "503 Service Unavailable", b"primary unavailable", "",
    )
    fixture.failure_responses[both_failed] = ("503 Service Unavailable", b"alternate unavailable", "")
    request = "検索 wikipedia -> crossref: alternate trial"

    def contacts_since(index: int) -> list[tuple[str, str]]:
        return [(event["host"], event["path"]) for event in fixture.events[index:]]

    cases = {}
    for name, query, flags, expected_exit, expected_contacts, reason in (
        ("explicit-primary", "検索 wikipedia -> crossref: candidate replay", grants, 0,
         [(primary, wiki + "q=candidate+replay&limit=5")], None),
        ("explicit-empty", "検索 wikipedia -> crossref: alternate empty", grants, 0,
         [(primary, wiki + "q=alternate+empty&limit=5")], None),
        ("explicit-malformed", "検索 wikipedia -> crossref: alternate malformed", grants, 2,
         [(primary, wiki + "q=alternate+malformed&limit=5")], "MALFORMED_SEARCH_LIST"),
        ("explicit-duplicate", "検索 wikipedia -> wikipedia: alternate trial", grants, 2,
         [], "two distinct providers"),
        ("explicit-budget", request, (*grants, "--max-actions", "4"), 2,
         [(primary, primary_busy)], "HTTP_START_BUDGET_EXHAUSTED"),
        ("explicit-denied-primary", request, ("--allow-read-host", fallback), 2,
         [], "READ_PERMISSION_REQUIRED"),
        ("explicit-denied-fallback", request, ("--allow-read-host", primary), 2,
         [(primary, primary_busy)], "READ_PERMISSION_REQUIRED"),
        ("explicit-denied-redirect", "検索 wikipedia -> crossref: alternate redirect",
         grants, 2, [(primary, denied_redirect)], "SEARCH_LIST_FETCH_FAILED"),
        ("explicit-byte-limit", request, (*grants, "--max-evidence-bytes", "64"), 2,
         [(primary, primary_busy), (fallback, alternative)], "SEARCH_LIST_FETCH_FAILED"),
        ("explicit-unavailable", "search wikipedia -> crossref: alternate unavailable",
         (*grants, "--max-actions", "5"), 0, [(primary, unavailable), (fallback, unavailable_alternate)], None),
        ("explicit-both-failed", "search wikipedia -> crossref: alternate failed",
         (*grants, "--max-actions", "5"), 2,
         [(primary, wiki + "q=alternate+failed&limit=5"), (fallback, both_failed)], "SEARCH_LIST_FETCH_FAILED"),
    ):
        seen, starts = len(fixture.events), len(fixture.connects)
        result = cli("run", "--input", "lookup", *flags, query, case_id=name, expected_exit=expected_exit)
        assert contacts_since(seen) == expected_contacts
        assert len(fixture.connects) == starts + len(expected_contacts)
        assert len(result["evidence"]) == len(result["dependencies"]) == len(expected_contacts)
        if reason is not None:
            assert reason in result["reason"] and result["render"] is None and result["receipts"] == []
        cases[name] = result
    assert "検索候補がありません" in cases["explicit-empty"]["text"]
    assert cases["explicit-denied-redirect"]["evidence"][0]["failure"] == "REDIRECT_PERMISSION_OR_TARGET_REJECTED"
    limited = cases["explicit-byte-limit"]["evidence"]
    assert limited[0]["failure"] == "HTTP_RATE_LIMITED" and limited[1]["failure"] == "EVIDENCE_BYTE_LIMIT"

    seen, starts = len(fixture.events), len(fixture.connects)
    case = "explicit-fallback-file"
    output = directory / "alternate-listing.txt"
    selected = cli("run", "--input", "lookup", *grants, "--max-actions", "6",
                   "--output", str(output), "--allow-write", str(output), request, case_id=case)
    failed, listing = selected["evidence"]
    assert failed["http_status"] == 429 and failed["failure"] == "HTTP_RATE_LIMITED"
    assert listing["http_status"] == 200 and listing["failure"] is None
    assert "crossref-works-search-v1" in selected["text"] and "Alternative fixture" in selected["text"]

    def load(reference: str):
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    first_ref, selected_ref = selected["dependencies"]
    assert load(first_ref)["listing"] == failed
    assert load(selected_ref)["listing"] == listing
    assert load(selected_ref)["query"] == load(first_ref)["query"] == "alternate trial"
    with sqlite3.connect(state_path / "state.sqlite3") as database:
        revisions = database.execute(
            "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision", (case,),
        ).fetchall()
    checkpoint = next(state for (ref,) in revisions if (state := load(ref))["reason"] == "SEARCH_PRIMARY_FAILURE_OBSERVED")
    assert checkpoint["lifecycle"] == "RUNNING"
    assert checkpoint["result"]["evidence"] == [failed]
    assert checkpoint["result"]["dependencies"] == [first_ref]

    before = output.stat()
    attempted = retain_attempted_prefix(state_path, case, "lookup", selected["receipts"][0]["action_id"])
    recovered = cli("reconcile", case_id=case)
    assert recovered["evidence"] == selected["evidence"] and recovered["dependencies"] == selected["dependencies"]
    assert recovered["receipts"][0]["action_id"] == attempted["action_id"]
    assert "NO_RESEND" in recovered["receipts"][0]["reason"]
    graph = load(recovered["execution_graph_ref"])
    (selection_ref,) = graph[5]["compute"].values()
    assert graph[5]["compute"] == {"search-listing": selection_ref}
    assert load(selection_ref) == ["search-listing-selection-v1", [first_ref, selected_ref]]
    assert output.read_bytes() == bytes(selected["render"]["bytes"])
    after = output.stat()
    assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)
    assert contacts_since(seen) == [(primary, primary_busy), (fallback, alternative)]
    assert len(fixture.connects) == starts + 2

    body = cli("run", "--input", "body", "--allow-read-host", fallback,
               "1件目の本文を取得して", case_id=case)
    assert body["evidence"][0] == listing and len(body["evidence"]) == 2
    assert bytes(body["evidence"][1]["raw_body"]) == selected_body
    blocked = cli("run", "--input", "during-retry-after", "--allow-read-host", primary,
                  "検索 wikipedia: alternate trial", case_id=case, expected_exit=2)
    assert "HTTP_RETRY_DEFERRED" in blocked["reason"] and failed["id"] in blocked["reason"]
    assert blocked["evidence"] == []
    assert contacts_since(seen) == [(primary, primary_busy), (fallback, alternative), (fallback, "/selected-article")]
    assert len(fixture.connects) == starts + 3 and not fixture.errors

    # Restore the real checkpoint following a primary failure with no alternate start.
    cancelled_case = "explicit-budget"
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
        snapshots = database.execute(
            "SELECT revision,state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision",
            (cancelled_case,),
        ).fetchall()
        revision, running = next((revision, state) for revision, ref in snapshots
                                 if (state := load(ref))["reason"] == "SEARCH_PRIMARY_FAILURE_OBSERVED")
        assert running["result"]["evidence"] == cases[cancelled_case]["evidence"]
        saved = json.dumps(running["result"], ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        digest = hashlib.sha256(saved.encode("utf-8")).hexdigest()
        database.execute("DROP TRIGGER case_revision_no_delete")
        database.execute("DELETE FROM case_revision WHERE case_id=? AND revision>?", (cancelled_case, revision))
        database.execute("UPDATE input_identity SET result_sha256=? WHERE case_id=? AND input_id='lookup'",
                         (digest, cancelled_case))
    seen, starts = len(fixture.events), len(fixture.connects)
    cli("run", "--input", "cancel", "中止", case_id=cancelled_case, expected_exit=5)
    cancelled_deferred = cli("run", "--input", "after-cancel", "--allow-read-host", primary,
                             "検索 wikipedia: alternate trial", case_id=cancelled_case, expected_exit=2)
    assert "HTTP_RETRY_DEFERRED" in cancelled_deferred["reason"]
    assert running["result"]["evidence"][0]["id"] in cancelled_deferred["reason"]
    assert cancelled_deferred["evidence"] == []
    assert not contacts_since(seen) and len(fixture.connects) == starts and not fixture.errors
    return {"cases": cases, "file": selected, "recovered": recovered, "body": body,
            "deferred": blocked, "cancelled_deferred": cancelled_deferred}


def check_interrupted_retrieval(cli, command_line, environment, fixture: Fixture, state_path: Path) -> dict:
    first, second = fixture.hosts
    grants = ("--allow-read-host", first, "--allow-read-host", second, "--deadline-ms", "12000")

    def latest(case_id: str) -> dict:
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database:
            (reference,) = database.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision DESC LIMIT 1",
                (case_id,),
            ).fetchone()
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    case = "interrupted-direct-retrieval"
    arguments = ("--input", "fetch", *grants, f"fetch https://{first}/429 https://{second}/slow-body")
    seen, starts = len(fixture.events), len(fixture.connects)
    with subprocess.Popen(command_line("run", *arguments, case_id=case), env=environment,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
        try:
            fixture.wait_for_request(process, seen, (second, "/slow-body"))
            checkpoint = latest(case)
        finally:
            process.kill()
            process.communicate(timeout=5)
    assert checkpoint["lifecycle"] == "RUNNING" and checkpoint["reason"] == "RETRIEVAL_SOURCE_OBSERVED"
    prefix = checkpoint["result"]
    (failure,) = prefix["evidence"]
    assert prefix["dependencies"] == [failure["id"]] and prefix["render"] is None and prefix["receipts"] == []
    assert failure["http_status"] == 429 and failure["request_uri"] == f"https://{first}/429"
    assert bytes(failure["raw_body"]) == b"rate limited fixture"
    assert failure["retry_after"]["directive"] == {"kind": "DELAY_SECONDS", "seconds": 120}

    replay = cli("run", *arguments, case_id=case, expected_exit=2)
    assert "INCOMPLETE_PRIOR_ATTEMPT" in replay["reason"]
    assert replay["evidence"] == prefix["evidence"] and replay["dependencies"] == prefix["dependencies"]
    assert latest(case) == checkpoint
    cli("run", "--input", "cancel", "中止", case_id=case, expected_exit=5)
    cancelled = latest(case)["prior_results"][prefix["id"]]
    assert cancelled["lifecycle"] == "CANCEL" and cancelled["evidence"] == prefix["evidence"]
    read = ("--allow-read-host", first, f"fetch https://{first}/ok")
    deferred = cli("run", "--input", "after-cancel", *read, case_id=case, expected_exit=2)
    assert "HTTP_RETRY_DEFERRED" in deferred["reason"] and failure["id"] in deferred["reason"]
    assert deferred["evidence"] == [] and len(fixture.connects) == starts + 2
    assert [(event["host"], event["path"]) for event in fixture.events[seen:]] == [(first, "/429"), (second, "/slow-body")]
    cli("run", "--input", "independent", *read, case_id="independent-direct-retrieval")
    assert len(fixture.connects) == starts + 3 and fixture.events[-1]["path"] == "/ok"

    case = "cancel-before-next-direct-source"
    arguments = ("--input", "fetch", *grants, f"fetch https://{first}/slow-body https://{second}/ok")
    seen, starts = len(fixture.events), len(fixture.connects)
    with subprocess.Popen(command_line("run", *arguments, case_id=case), env=environment,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
        try:
            fixture.wait_for_request(process, seen, (first, "/slow-body"))
            cli("run", "--input", "cancel", "中止", case_id=case, expected_exit=5)
            _, errors = process.communicate(timeout=8)
            assert process.returncode != 0, errors
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=5)
    assert latest(case)["lifecycle"] == "CANCEL"
    assert len(fixture.connects) == starts + 1
    assert [(event["host"], event["path"]) for event in fixture.events[seen:]] == [(first, "/slow-body")]
    assert not fixture.errors, fixture.errors
    return {"checkpoint": checkpoint, "replay": replay, "cancelled": cancelled, "deferred": deferred}


def check_redirect_cancellation(cli, command_line, environment, fixture: Fixture, state_path: Path) -> dict:
    first, second = fixture.hosts
    target = f"https://{second}/ok"
    grants = ("--allow-read-host", first, "--allow-read-host", second, "--deadline-ms", "15000")
    body_case = "cancel-redirect-candidate"
    output = state_path.parent / "cancelled-redirect.txt"
    cli("run", "--input", "listing", *grants, "検索 wikipedia: candidate replay", case_id=body_case)

    def case_head(case_id: str) -> tuple:
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database:
            return database.execute(
                "SELECT revision,state_sha256,lifecycle FROM case_revision WHERE case_id=? "
                "ORDER BY revision DESC LIMIT 1", (case_id,),
            ).fetchone()

    results = {}
    for case, path, extra_flags, query, expected_prefix, authorized in (
        ("cancel-redirect-direct", "/held-redirect", ("--output", str(output), "--allow-write", str(output)),
         f"fetch https://{first}/ok https://{first}/held-redirect", ["/ok"], True),
        ("cancel-redirect-search", "/w/rest.php/v1/search/page?q=redirect+cancel&limit=5", (),
         "検索 wikipedia -> crossref: redirect cancel", [], True),
        (body_case, "/wiki/First?action=raw", (), "1件目の本文を取得して", [], True),
        ("denied-redirect-cancellation", "/held-redirect", (), f"fetch https://{first}/held-redirect", [], False),
    ):
        gate = threading.Event()
        fixture.response_gates[path] = gate
        fixture.redirects[path] = target
        arguments = ("--input", "original", *grants, *extra_flags, query)
        seen, starts = len(fixture.events), len(fixture.connects)
        with subprocess.Popen(command_line("run", *arguments, case_id=case), env=environment,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
            try:
                fixture.wait_for_request(process, seen, (first, path))
                initial = case_head(case)
                assert initial[-1] == "RUNNING"
                if authorized:
                    cli("run", "--input", "cancel", "中止", case_id=case, expected_exit=5)
                else:
                    denied = cli("run", "--input", "cancel", "中止", case_id=case,
                                 actor_id="other-fixture-actor", expected_exit=3)
                    assert "CASE_ACCESS_DENIED" in denied["reason"]
                after_cancel = case_head(case)
                assert after_cancel[-1] == ("CANCEL" if authorized else "RUNNING")
                assert authorized or after_cancel == initial
                gate.set()
                raw, errors = process.communicate(timeout=8)
                assert process.returncode == (5 if authorized else 0), errors
            finally:
                gate.set()
                if process.poll() is None:
                    process.kill()
                    process.communicate(timeout=5)
                fixture.response_gates.pop(path)
                fixture.redirects.pop(path)
        result = json.loads(raw)
        expected = [(first, uri) for uri in [*expected_prefix, path]]
        if authorized:
            assert case_head(case) == after_cancel
            assert result["lifecycle"] == "CANCEL" and result["render"] is None and result["receipts"] == []
            assert [packet["request_uri"] for packet in result["evidence"]] == [
                f"https://{first}{uri}" for uri in expected_prefix
            ]
            replay = cli("run", *arguments, case_id=case, expected_exit=5)
            assert replay == result and case_head(case) == after_cancel
        else:
            expected.append((second, "/ok"))
            assert result["evidence"][0]["redirect_chain"] == [f"https://{first}{path}", target]
        assert [(event["host"], event["path"]) for event in fixture.events[seen:]] == expected
        assert len(fixture.connects) == starts + len(expected)
        results[case] = result
    assert not output.exists() and not fixture.errors, fixture.errors
    return results


def check_redirect_retry_after(cli, fixture: Fixture, state_path: Path) -> dict:
    first, second = fixture.hosts
    grants = ("--allow-read-host", first, "--allow-read-host", second)
    target = f"https://{second}/ok"
    stop_code = "HTTP_REDIRECT_RETRY_DEFERRED"

    def redirect(path: str, status: str, directive: str) -> None:
        fixture.failure_responses[path] = (status, b"", f"Location: {target}\r\n{directive}")

    def assert_contacts(seen: int, starts: int, expected: list[tuple[str, str]]) -> None:
        assert [(event["host"], event["path"]) for event in fixture.events[seen:]] == expected
        assert len(fixture.connects) == starts + len(expected)

    def assert_deferred(result: dict, kind: str, status: int, path: str) -> dict:
        assert stop_code in result["reason"] and result["render"] is None and result["receipts"] == []
        packet = result["evidence"][-1]
        assert packet["packet_kind"] == kind and packet["http_status"] == status
        assert packet["failure"] == stop_code and packet["raw_body"] == [] and packet["extracted_text"] is None
        assert packet["final_uri"] == f"https://{first}{path}"
        return packet

    cases = {}
    for name, status, header, directive, due in (
        ("seconds", "307 Temporary Redirect", "Retry-After: 120\r\n", {"kind": "DELAY_SECONDS", "seconds": 120}, 120_000),
        ("date", "302 Found", f"Retry-After: {RETRY_DATE}\r\n", {"kind": "HTTP_DATE", "value": RETRY_DATE}, 4_102_444_800_000),
        ("large", "308 Permanent Redirect", "Retry-After: 1234567890123\r\n", {"kind": "DELAY_OUT_OF_RANGE"}, None),
        ("malformed", "301 Moved Permanently", "Retry-After: -1\r\n", {"kind": "HEADER_INVALID"}, None),
        ("duplicate", "303 See Other", "Retry-After: 0\r\nRetry-After: 120\r\n", {"kind": "MULTIPLE_HEADERS"}, None),
        ("zero", "302 Found", "Retry-After: 0000000000000\r\n", None, None),
        ("elapsed", "302 Found", "Retry-After: Thu, 01 Jan 1970 00:00:00 GMT\r\n", None, None),
    ):
        case, path = f"redirect-retry-{name}", f"/redirect-retry-{name}"
        redirect(path, status, header)
        requested_path = path
        expected = [(first, path)]
        query = f"fetch https://{first}{path}"
        if name == "seconds":
            # Preserve earlier redirects and a completed source, and do not
            # fall through to the same target as a later direct source.
            requested_path = "/before-redirect-retry"
            fixture.redirects[requested_path] = path
            query = f"fetch https://{first}/ok https://{first}{requested_path} {target}"
            expected = [(first, "/ok"), (first, requested_path), (first, path)]
        arguments = ("--input", "original", *grants, query)
        seen, starts = len(fixture.events), len(fixture.connects)
        if directive is None:
            result = cli("run", *arguments, case_id=case)
            assert result["evidence"][0]["redirect_chain"] == [f"https://{first}{path}", target]
            assert "retry_after" not in result["evidence"][0]
            expected.append((second, "/ok"))
        else:
            result = cli("run", *arguments, case_id=case, expected_exit=2)
            packet = assert_deferred(result, "FETCH_FAILURE", int(status[:3]), path)
            chain = [f"https://{first}{requested_path}"]
            if name == "seconds":
                assert result["evidence"][0]["failure"] is None and len(result["evidence"]) == 2
                chain.append(f"https://{first}{path}")
            assert packet["redirect_chain"] == chain and result["dependencies"][-1] == packet["id"]
            retry = packet["retry_after"]
            assert retry["directive"] == directive
            expected_due = retry["received_at_unix_ms"] + due if name == "seconds" else due
            assert retry["not_before_unix_ms"] == expected_due
            if name == "seconds":
                assert cli("run", *arguments, case_id=case, expected_exit=2) == result
        assert_contacts(seen, starts, expected)
        cases[case] = result

    seen, starts = len(fixture.events), len(fixture.connects)
    denied = cli("run", "--input", "original", "--allow-read-host", first,
                 f"fetch https://{first}/redirect-retry-date", case_id="redirect-retry-denied", expected_exit=2)
    assert denied["evidence"][0]["failure"] == "REDIRECT_PERMISSION_OR_TARGET_REJECTED"
    assert_contacts(seen, starts, [(first, "/redirect-retry-date")])
    seen, starts = len(fixture.events), len(fixture.connects)
    for host in (first, second):
        cli("run", "--input", f"independent-{host}", "--allow-read-host", host, f"fetch https://{host}/ok",
            case_id="redirect-retry-seconds")
    assert_contacts(seen, starts, [(first, "/ok"), (second, "/ok")])

    search_path = "/w/rest.php/v1/search/page?q=redirect+retry&limit=5"
    redirect(search_path, "302 Found", "Retry-After: 120\r\n")
    seen, starts = len(fixture.events), len(fixture.connects)
    search = cli("run", "--input", "original", *grants, "検索 wikipedia -> crossref: redirect retry",
                 case_id="redirect-retry-search", expected_exit=2)
    listing = assert_deferred(search, "SEARCH_LIST", 302, search_path)
    (source_ref,) = search["dependencies"]
    raw = (state_path / "blobs" / source_ref).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == source_ref and json.loads(raw)["listing"] == listing
    assert_contacts(seen, starts, [(first, search_path)])

    body_case = "redirect-retry-candidate"
    cli("run", "--input", "listing", *grants, "検索 wikipedia: candidate replay", case_id=body_case)
    body_path = "/wiki/First?action=raw"
    redirect(body_path, "302 Found", "Retry-After: 120\r\n")
    seen, starts = len(fixture.events), len(fixture.connects)
    body = cli("run", "--input", "original", *grants, "1件目の本文を取得して", case_id=body_case, expected_exit=2)
    assert_deferred(body, "FETCH_FAILURE", 302, body_path)
    assert body["evidence"][0]["packet_kind"] == "SEARCH_LIST"
    assert_contacts(seen, starts, [(first, body_path)])
    assert not fixture.errors, fixture.errors
    return {"cases": cases, "read_denied": denied, "search": search, "candidate": body}


def check_retained_redirect_wait(cli, fixture: Fixture, state_path: Path) -> dict:
    origin, held = fixture.hosts
    grants = ("--allow-read-host", origin, "--allow-read-host", held)
    target = f"https://{held}/retained-wait-target"
    prior_path, redirect_path, later_path = "/retained-wait-prior", "/retained-wait-redirect", "/retained-wait-later"
    fixture.responses[prior_path] = (b"Completed before the held redirect.", "text/plain")
    fixture.responses[later_path] = (b"Must remain unrequested.", "text/plain")
    fixture.responses["/retained-wait-target"] = (b"Independent case body.", "text/plain")

    def seed(case: str, status: str = "429 Too Many Requests") -> dict:
        path = f"/retained-wait-{status[:3]}"
        fixture.failure_responses[path] = (status, b"Observed hold.", "Retry-After: 120\r\n")
        result = cli("run", "--input", "held", "--allow-read-host", held, f"fetch https://{held}{path}",
                     case_id=case, expected_exit=2)
        (packet,) = result["evidence"]
        assert packet["http_status"] == int(status[:3])
        assert packet["retry_after"]["directive"] == {"kind": "DELAY_SECONDS", "seconds": 120}
        return packet

    def load(reference: str) -> dict:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    def assert_contacts(seen: int, starts: int, paths: list[tuple[str, str]]) -> None:
        assert [(event["host"], event["path"]) for event in fixture.events[seen:]] == paths
        assert len(fixture.connects) == starts + len(paths)

    def assert_wait(result: dict, seed_packet: dict, path: str, kind: str = "FETCH_FAILURE") -> dict:
        assert result["reason"].startswith("HTTP_RETRY_DEFERRED: ")
        assert held in result["reason"] and seed_packet["id"] in result["reason"]
        assert result["content_plan"] is None and result["render"] is None and result["receipts"] == []
        observed = result["evidence"][-1]
        assert observed["packet_kind"] == kind and observed["http_status"] == 302
        assert observed["failure"] == "HTTP_RETRY_DEFERRED" and observed.get("retry_after") is None
        assert observed["request_uri"] == observed["final_uri"] == f"https://{origin}{path}"
        assert observed["redirect_chain"] == [observed["final_uri"]]
        assert observed["raw_body"] == [] and observed["extracted_text"] is None
        return observed

    fixture.redirects[redirect_path] = target
    results = {}
    for status in ("429 Too Many Requests", "503 Service Unavailable"):
        case = f"retained-redirect-direct-{status[:3]}"
        original = seed(case, status)
        seen, starts = len(fixture.events), len(fixture.connects)
        request = f"fetch https://{origin}{prior_path} https://{origin}{redirect_path} https://{origin}{later_path}"
        output = state_path.parent / f"{case}.txt"
        args = ("--input", "redirect", *grants, "--output", str(output), "--allow-write", str(output), request)
        result = cli("run", *args, case_id=case, expected_exit=2)
        blocked = assert_wait(result, original, redirect_path)
        prefix, _ = result["evidence"]
        assert prefix["packet_kind"] == "FETCHED_BODY"
        assert result["dependencies"] == [prefix["id"], blocked["id"]] and not output.exists()
        assert cli("run", *args, case_id=case, expected_exit=2) == result
        assert_contacts(seen, starts, [(origin, prior_path), (origin, redirect_path)])
        results[status[:3]] = result

    case = "retained-redirect-missing-grant"
    held_packet = seed(case)
    seen, starts = len(fixture.events), len(fixture.connects)
    denied = cli("run", "--input", "denied", "--allow-read-host", origin, f"fetch https://{origin}{redirect_path}",
                 case_id=case, expected_exit=2)
    assert denied["evidence"][0]["failure"] == "REDIRECT_PERMISSION_OR_TARGET_REJECTED"
    assert held_packet["id"] not in denied["reason"] and "HTTP_RETRY_DEFERRED" not in denied["reason"]
    cli("run", "--input", "independent", "--allow-read-host", held, f"fetch {target}", case_id="retained-redirect-other-case")
    assert_contacts(seen, starts, [(origin, redirect_path), (held, "/retained-wait-target")])

    case = "retained-redirect-search"
    original = seed(case)
    search_path = "/w/rest.php/v1/search/page?q=retained+redirect&limit=5"
    fixture.redirects[search_path] = target
    seen, starts = len(fixture.events), len(fixture.connects)
    search = cli("run", "--input", "redirect", *grants, "検索 wikipedia -> crossref: retained redirect",
                 case_id=case, expected_exit=2)
    listing = assert_wait(search, original, search_path, "SEARCH_LIST")
    (listing_ref,) = search["dependencies"]
    assert load(listing_ref)["listing"] == listing
    assert_contacts(seen, starts, [(origin, search_path)])

    case = "retained-redirect-candidate"
    original = seed(case)
    list_path = "/w/rest.php/v1/search/page?q=candidate+after+retained+wait&limit=5"
    titles = ("Held_by_redirect", "Independent_article")
    candidate_path, next_path = [f"/wiki/{title}?action=raw" for title in titles]
    fixture.responses[list_path] = (json.dumps({"pages": [
        {"title": title, "key": title} for title in titles
    ]}).encode(), "application/json")
    fixture.redirects[candidate_path] = target
    fixture.responses[next_path] = (b"New explicit candidate body.", "text/plain")
    listing_result = cli("run", "--input", "listing", *grants,
                         "検索 wikipedia: candidate after retained wait", case_id=case)
    seen, starts = len(fixture.events), len(fixture.connects)
    candidate = cli("run", "--input", "redirect", *grants, "1件目の本文を取得して",
                    case_id=case, expected_exit=2)
    blocked = assert_wait(candidate, original, candidate_path)
    assert load(candidate["dependencies"][-1]) == blocked
    assert candidate["evidence"][:-1] == listing_result["evidence"]
    assert_contacts(seen, starts, [(origin, candidate_path)])
    next_result = cli("run", "--input", "another-rank", "--allow-read-host", origin,
                      "2件目の本文を取得して", case_id=case)
    assert next_result["evidence"][:-1] == listing_result["evidence"]
    assert next_result["evidence"][-1]["extracted_text"] == "New explicit candidate body."
    assert_contacts(seen, starts, [(origin, candidate_path), (origin, next_path)])
    assert not fixture.errors, fixture.errors
    return {**results, "denied": denied, "search": search, "candidate": candidate, "next": next_result}


def check_versioned_source_summary(cli, fixture: Fixture, state_path: Path) -> dict:
    host = fixture.host
    listing_path = "/w/rest.php/v1/search/page?q=candidate+replay&limit=5"
    article_path = "/wiki/First?action=raw"
    old_text, current_text = "測定値は10です。", "測定値は30です。"
    allow = ("--allow-read-host", host)

    def load(reference: str) -> dict | list:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    results = {}
    for case, query, write_file in (
        ("versioned-source-summary", "前回の本文を要約して", False),
        ("versioned-constrained-source-summary", "前回の資料を2点以内、20字以内で要約して", True),
    ):
        seen, starts = len(fixture.events), len(fixture.connects)
        cli("run", "--input", "listing", *allow, "検索 wikipedia: candidate replay", case_id=case)
        observations = []
        for input_id, text in (("old", old_text), ("latest", current_text)):
            if observations:
                first_observed = observations[0]["evidence"][-1]["observed_at_unix_ms"]
                limit = time.monotonic() + 2
                while time.time_ns() // 1_000_000 <= first_observed:
                    assert time.monotonic() < limit
                    time.sleep(0.001)
            fixture.responses[article_path] = (text.encode(), "text/plain; charset=utf-8")
            observations.append(cli("run", "--input", input_id, *allow, "1件目の本文を取得して", case_id=case))
        old_result, current_result = observations
        old_packet, current_packet = current_result["evidence"][1:]
        old_ref, current_ref = old_result["dependencies"][-1], current_result["dependencies"][-1]
        assert old_packet["final_uri"] == current_packet["final_uri"]
        assert old_packet["observed_at_unix_ms"] < current_packet["observed_at_unix_ms"]
        assert load(old_ref) == old_packet and load(current_ref) == current_packet

        output = state_path.parent / f"{case}.txt"
        output_flags = ("--output", str(output), "--allow-write", str(output)) if write_file else ()
        summary = cli("run", "--input", "summary", *output_flags, query, case_id=case)
        assert current_text in summary["text"] and old_text not in summary["text"]
        prior_id, original, latest, report_ref = summary["dependencies"]
        assert (prior_id, original, latest) == (current_result["id"], old_ref, current_ref)
        report = load(report_ref)
        assert report["source_packet_id"] == current_packet["id"] and report["source_packet_ref"] == current_ref
        assert report["raw_body_sha256"] == current_packet["raw_sha256"]
        assert report["summary"]["source_text"] == current_text
        proofs = {"followup-source-1": old_ref, "followup-source-2": current_ref}
        assert load(summary["execution_graph_ref"])[5]["compute"] == proofs

        if write_file:
            assert report["summary"]["max_sentences"] == 2 and report["summary"]["max_chars"] == 20
            before = output.stat()
            (receipt,) = summary["receipts"]
            retain_attempted_prefix(state_path, case, "summary", receipt["action_id"])
            recovered = cli("reconcile", case_id=case)
            assert recovered["dependencies"] == summary["dependencies"]
            assert recovered["text"] == summary["text"]
            assert recovered["receipts"][0]["action_id"] == receipt["action_id"]
            assert "NO_RESEND" in recovered["receipts"][0]["reason"]
            assert load(recovered["execution_graph_ref"])[5]["compute"] == proofs
            assert output.read_bytes() == bytes(summary["render"]["bytes"])
            after = output.stat()
            assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)
            results["recovered"] = recovered
        assert [event["path"] for event in fixture.events[seen:]] == [listing_path, article_path, article_path]
        assert len(fixture.connects) == starts + 3 and not fixture.errors
        results[case] = summary
    return results


def check_candidate_after_failure(cli, fixture: Fixture, state_path: Path) -> dict:
    host = fixture.host
    allow = ("--allow-read-host", host)
    listing_path = "/w/rest.php/v1/search/page?q=candidate+failure&limit=5"
    titles = ("Retained", "Failed", "Another", "Selected")
    first_path, failed_path, another_path, selected_path = [f"/wiki/{title}?action=raw" for title in titles]
    fixture.responses[listing_path] = (json.dumps({"pages": [
        {"title": title, "key": title} for title in titles
    ]}).encode(), "application/json")
    fixture.responses[first_path] = (b"The first successful source was kept.", "text/plain")
    fixture.responses[selected_path] = (b"The newly requested source was acquired.", "text/plain")
    for path in (failed_path, another_path):
        fixture.failure_responses[path] = ("404 Not Found", b"Candidate body unavailable.", "")

    def rank(case: str, input_id: str, number: int, *flags: str, expected_exit: int = 0) -> dict:
        return cli("run", "--input", input_id, *flags, f"{number}件目の本文を取得して",
                   case_id=case, expected_exit=expected_exit)

    def load(reference: str) -> dict | list:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    seen, starts = len(fixture.events), len(fixture.connects)
    case = "candidate-after-two-failures"
    listing = cli("run", "--input", "listing", *allow, "検索 wikipedia: candidate failure", case_id=case)
    committed = rank(case, "first", 1, *allow)
    failures = []
    for input_id, number in (("failed", 2), ("another", 3)):
        failure = rank(case, input_id, number, *allow, expected_exit=2)
        assert failure["evidence"][:-1] == committed["evidence"]
        packet = failure["evidence"][-1]
        assert packet["packet_kind"] == "FETCH_FAILURE" and packet["http_status"] == 404
        assert load(failure["dependencies"][-1]) == packet
        assert failure["dependencies"][:-1] == [
            committed["id"], listing["dependencies"][0], committed["dependencies"][-1],
        ]
        assert rank(case, input_id, number, *allow, expected_exit=2) == failure
        failures.append(failure)

    output = state_path.parent / "candidate-after-failure.txt"
    selected = rank(case, "selected", 4, *allow, "--output", str(output), "--allow-write", str(output))
    assert selected["evidence"][:-1] == committed["evidence"]
    assert bytes(selected["evidence"][-1]["raw_body"]) == b"The newly requested source was acquired."
    assert selected["dependencies"][:-1] == failures[-1]["dependencies"][:-1]
    assert all(failure["dependencies"][-1] not in selected["dependencies"] for failure in failures)
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database:
        (reference,) = database.execute(
            "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision DESC LIMIT 1",
            (case,),
        ).fetchone()
    state = load(reference)
    assert state["bundle"]["request"]["context_ref"] is None
    assert state["bundle"]["meaning"]["context_result_ref"] == committed["id"]
    assert all(state["prior_results"][failure["id"]] == failure for failure in failures)

    before = output.stat()
    (receipt,) = selected["receipts"]
    retain_attempted_prefix(state_path, case, "selected", receipt["action_id"])
    recovered = cli("reconcile", case_id=case)
    assert recovered["dependencies"] == selected["dependencies"] and recovered["evidence"] == selected["evidence"]
    assert recovered["receipts"][0]["action_id"] == receipt["action_id"]
    assert "NO_RESEND" in recovered["receipts"][0]["reason"]
    graph = load(recovered["execution_graph_ref"])
    assert graph[5]["search-candidate-body-4"] == {"search-candidate-listing": listing["dependencies"][0]}
    assert output.read_bytes() == bytes(selected["render"]["bytes"])
    after = output.stat()
    assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)
    assert [event["path"] for event in fixture.events[seen:]] == [
        listing_path, first_path, failed_path, another_path, selected_path,
    ]
    assert len(fixture.connects) == starts + 5

    stops = {}
    for name, flags, next_rank, reason in (
        ("same-rank", allow, 2, "MISSING_SEARCH_RESULT_CONTEXT"),
        ("no-grant", (), 4, "READ_PERMISSION_REQUIRED"),
        ("deferred-host", allow, 4, "HTTP_RETRY_DEFERRED"),
    ):
        case = f"candidate-after-failure-{name}"
        if name == "deferred-host":
            fixture.failure_responses[failed_path] = (
                "429 Too Many Requests", b"Candidate rate limited.", "Retry-After: 120\r\n",
            )
        seen, starts = len(fixture.events), len(fixture.connects)
        cli("run", "--input", "listing", *allow, "検索 wikipedia: candidate failure", case_id=case)
        failure = rank(case, "failed", 2, *allow, expected_exit=2)
        stopped = rank(case, "next", next_rank, *flags, expected_exit=2)
        assert reason in stopped["reason"]
        if name == "deferred-host":
            assert failure["evidence"][-1]["id"] in stopped["reason"]
        elif name == "no-grant":
            # A stop before communication does not supply a failed attempt
            # from which another implicit rank can be resolved.
            no_context = rank(case, "after-denial", 3, *allow, expected_exit=2)
            assert "MISSING_SEARCH_RESULT_CONTEXT" in no_context["reason"]
        assert [event["path"] for event in fixture.events[seen:]] == [listing_path, failed_path]
        assert len(fixture.connects) == starts + 2
        stops[name] = stopped

    case = "candidate-after-failure-unbound-artifact"
    seen, starts = len(fixture.events), len(fixture.connects)
    fixture.failure_responses[failed_path] = ("404 Not Found", b"Candidate body unavailable.", "")
    cli("run", "--input", "listing", *allow, "検索 wikipedia: candidate failure", case_id=case)
    failure = rank(case, "failed", 2, *allow, expected_exit=2)
    packet_ref = failure["dependencies"][-1]
    # Keep the blob intact while removing its case binding in this disposable store.
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
        removed = database.execute("DELETE FROM case_artifact WHERE case_id=? AND sha256=?", (case, packet_ref))
        assert removed.rowcount == 1
    try:
        invalid = rank(case, "next", 4, *allow, expected_exit=4)
        assert "INPUT_FAILURE" in invalid["reason"]
        assert invalid["evidence"] == [] and invalid["receipts"] == []
    finally:
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
            database.execute("INSERT INTO case_artifact(case_id,sha256) VALUES(?,?)", (case, packet_ref))
    cli("run", "--input", "new-calculation", "2+3", case_id=case)
    assert [event["path"] for event in fixture.events[seen:]] == [listing_path, failed_path]
    assert len(fixture.connects) == starts + 2
    stops["unbound-artifact"] = invalid
    assert not fixture.errors, fixture.errors
    return {"failures": failures, "selected": selected, "recovered": recovered, "stops": stops}


def check_sources_after_candidate_failure(cli, fixture: Fixture, state_path: Path) -> dict:
    host = fixture.host
    allow = ("--allow-read-host", host)
    listing_path = "/w/rest.php/v1/search/page?q=retained+source&limit=5"
    obtained_path, failed_path = "/wiki/Obtained?action=raw", "/wiki/Unavailable?action=raw"
    observed_text = "実測値は42です。"
    rejected_text = "実測値は999です。秘匿値は88です。"
    fixture.responses[listing_path] = (json.dumps({"pages": [
        {"title": title, "key": title} for title in ("Obtained", "Unavailable")
    ]}).encode(), "application/json")
    fixture.responses[obtained_path] = (observed_text.encode(), "text/plain; charset=utf-8")
    fixture.failure_responses[failed_path] = ("404 Not Found", rejected_text.encode(), "")

    def load(reference: str) -> dict:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    def prepare(case: str, acquire_body: bool = True) -> tuple[dict | None, dict]:
        cli("run", "--input", "listing", *allow, "検索 wikipedia: retained source", case_id=case)
        source = (cli("run", "--input", "obtained", *allow, "1件目の本文を取得して", case_id=case)
                  if acquire_body else None)
        failed = cli("run", "--input", "unavailable", *allow, "2件目の本文を取得して", case_id=case, expected_exit=2)
        failure = failed["evidence"][-1]
        assert failure["http_status"] == 404 and bytes(failure["raw_body"]) == rejected_text.encode()
        return source, failed

    outcomes = {}
    for route, query, write_file in (
        ("question", "本文では実測値について何と書かれていますか", True),
        ("summary", "取得した本文を要約して", False),
        ("constrained", "取得した本文を2点以内、20字以内で要約して", True),
    ):
        case = f"source-{route}-after-failure"
        seen, starts = len(fixture.events), len(fixture.connects)
        source, failed = prepare(case)
        assert failed["evidence"][:-1] == source["evidence"]
        source_packet = source["evidence"][-1]
        source_ref = source["dependencies"][-1]
        assert load(source_ref) == source_packet

        output = state_path.parent / f"{case}.txt"
        output_flags = ("--output", str(output), "--allow-write", str(output)) if write_file else ()
        result = cli("run", "--input", "followup", *output_flags, query, case_id=case)
        assert result["guarantee"] == "HYPOTHESIS" and result["evidence"] == []
        assert observed_text in result["text"] and source_packet["final_uri"] in result["text"]
        assert "実測値は999です。" not in result["text"] and "秘匿値" not in result["text"]
        prior_id, packet_ref, report_ref = result["dependencies"]
        assert (prior_id, packet_ref) == (source["id"], source_ref)
        report = load(report_ref)
        assert report["source_result_id"] == source["id"]
        if route == "question":
            (quote,) = report["candidates"]
            assert quote["source_packet_ref"] == source_ref and quote["text"] == observed_text
        else:
            assert report["source_packet_ref"] == source_ref
            assert report["summary"]["source_text"] == observed_text
            if route == "constrained":
                assert report["summary"]["max_chars"] == 20 and report["summary"]["max_sentences"] == 2

        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database:
            (reference,) = database.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision DESC LIMIT 1",
                (case,),
            ).fetchone()
        state = load(reference)
        assert state["bundle"]["request"]["permissions"]["read_hosts"] == []
        assert state["bundle"]["meaning"]["context_result_ref"] == source["id"]
        assert state["prior_results"][failed["id"]] == failed

        if write_file:
            before = output.stat()
            (receipt,) = result["receipts"]
            retain_attempted_prefix(state_path, case, "followup", receipt["action_id"])
            recovered = cli("reconcile", case_id=case)
            assert recovered["dependencies"] == result["dependencies"] and recovered["text"] == result["text"]
            assert recovered["receipts"][0]["action_id"] == receipt["action_id"]
            assert "NO_RESEND" in recovered["receipts"][0]["reason"]
            assert output.read_bytes() == bytes(result["render"]["bytes"])
            after = output.stat()
            assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)
        assert [event["path"] for event in fixture.events[seen:]] == [listing_path, obtained_path, failed_path]
        assert len(fixture.connects) == starts + 3
        outcomes[route] = result

    for case, acquire_body, reason in (
        ("failed-body-only", False, "MISSING_ACTUAL_RESULT"),
        ("failed-body-has-only-match", True, "SOURCE_QUESTION_NO_MATCH"),
    ):
        seen, starts = len(fixture.events), len(fixture.connects)
        prepare(case, acquire_body)
        stopped = cli("run", "--input", "followup", "本文では秘匿値について何と書かれていますか",
                      case_id=case, expected_exit=2)
        assert reason in stopped["reason"] and stopped["evidence"] == [] and stopped["dependencies"] == []
        expected = [listing_path, obtained_path, failed_path] if acquire_body else [listing_path, failed_path]
        assert [event["path"] for event in fixture.events[seen:]] == expected
        assert len(fixture.connects) == starts + len(expected)
        outcomes[case] = stopped
    assert not fixture.errors, fixture.errors
    return outcomes


def check_source_followup_chain(cli, fixture: Fixture, state_path: Path) -> dict:
    case, path = "source-followup-chain", "/source-followup-chain"
    first_sentence, second_sentence = "実測値は42です。", "予備値は7です。"
    body = first_sentence + second_sentence
    uri = f"https://{fixture.host}{path}"
    fixture.responses[path] = (body.encode(), "text/plain; charset=utf-8")
    seen, starts = len(fixture.events), len(fixture.connects)

    def load(reference: str) -> dict:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    def fetch(case_id: str) -> dict:
        return cli("run", "--input", "source", "--allow-read-host", fixture.host,
                   f"fetch {uri}", case_id=case_id)

    def question(input_id: str, term: str, case_id: str = case, expected_exit: int = 0) -> dict:
        return cli("run", "--input", input_id, f"本文では{term}について何と書かれていますか",
                   case_id=case_id, expected_exit=expected_exit)

    source = fetch(case)
    first = question("first", "実測値")
    second = question("second", "予備値")
    summary = cli("run", "--input", "summary", "取得した本文を要約して", case_id=case)
    output = state_path.parent / f"{case}.txt"
    constrained = cli("run", "--input", "constrained", "--output", str(output), "--allow-write", str(output),
                      "取得した本文を2点以内、20字以内で要約して", case_id=case)
    (receipt,) = constrained["receipts"]
    before = output.stat()
    retain_attempted_prefix(state_path, case, "constrained", receipt["action_id"])
    recovered = cli("reconcile", case_id=case)
    assert recovered["dependencies"] == constrained["dependencies"] and recovered["text"] == constrained["text"]
    (restored_receipt,) = recovered["receipts"]
    assert restored_receipt["action_id"] == receipt["action_id"]
    assert "NO_RESEND" in restored_receipt["reason"]
    assert output.read_bytes() == bytes(constrained["render"]["bytes"])
    after = output.stat()
    assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)
    final = question("after-summary", "予備値")

    source_ref = first["dependencies"][1]
    packet = load(source_ref)
    assert [packet] == source["evidence"] and packet["extracted_text"] == body
    assert second_sentence not in first["text"] and first_sentence not in second["text"]
    for result in (first, second, summary, constrained, final):
        assert result["guarantee"] == "HYPOTHESIS" and result["evidence"] == [] and uri in result["text"]
        source_id, packet_ref, report_ref = result["dependencies"]
        assert (source_id, packet_ref) == (source["id"], source_ref)
        report = load(report_ref)
        assert report["source_result_id"] == source["id"]
        if result["id"] in (summary["id"], constrained["id"]):
            assert report["source_packet_ref"] == source_ref and report["summary"]["source_text"] == body
            assert first_sentence in result["text"] and second_sentence in result["text"]
        else:
            (quote,) = report["candidates"]
            assert quote["source_packet_ref"] == source_ref
            assert quote["text"] == (first_sentence if result["id"] == first["id"] else second_sentence)
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database:
        (reference,) = database.execute(
            "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision DESC LIMIT 1", (case,),
        ).fetchone()
    state = load(reference)
    assert state["bundle"]["request"]["permissions"]["read_hosts"] == []
    assert state["bundle"]["request"]["context_ref"] is None
    assert state["bundle"]["meaning"]["context_result_ref"] == source["id"]
    assert state["prior_results"][first["id"]] == first
    assert state["prior_results"][recovered["id"]] == recovered

    no_budget = cli("run", "--input", "no-budget", "--deadline-ms", "0",
                    "取得した本文を要約して", case_id=case, expected_exit=2)
    assert "RESOURCE_LIMIT" in no_budget["reason"] and no_budget["dependencies"] == []
    cli("run", "--input", "unrelated", "2+3", case_id=case)
    stopped = question("after-unrelated", "予備値", expected_exit=2)
    assert "SOURCE_BODY_UNAVAILABLE" in stopped["reason"] and stopped["dependencies"] == []

    unbound_case = "source-followup-unbound-report"
    fetch(unbound_case)
    anchor = question("first", "実測値", case_id=unbound_case)
    report_ref = anchor["dependencies"][-1]
    # Keep the report bytes while removing their case binding in this disposable store.
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
        removed = database.execute("DELETE FROM case_artifact WHERE case_id=? AND sha256=?", (unbound_case, report_ref))
        assert removed.rowcount == 1
    try:
        invalid = question("next", "予備値", case_id=unbound_case, expected_exit=4)
        assert "INPUT_FAILURE" in invalid["reason"]
        assert invalid["dependencies"] == [] and invalid["receipts"] == []
    finally:
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
            database.execute("INSERT INTO case_artifact(case_id,sha256) VALUES(?,?)", (unbound_case, report_ref))
    cli("run", "--input", "new-calculation", "2+3", case_id=unbound_case)
    assert [event["path"] for event in fixture.events[seen:]] == [path, path]
    assert len(fixture.connects) == starts + 2
    assert not fixture.errors, fixture.errors
    return {"first": first, "second": second, "summary": summary, "constrained": constrained,
            "recovered": recovered, "final": final, "no_budget": no_budget, "stopped": stopped, "unbound": invalid}


def check_candidate_after_source_followup(cli, fixture: Fixture, state_path: Path) -> dict:
    host = fixture.host
    allow = ("--allow-read-host", host)
    listing_path = "/w/rest.php/v1/search/page?q=candidate+after+analysis&limit=5"
    titles = ("First_analysis", "Second_analysis")
    first_path, second_path = (f"/wiki/{title}?action=raw" for title in titles)
    first_text, second_text = "実測値は42です。", "予備値は7です。"
    fixture.responses[listing_path] = (json.dumps({"pages": [
        {"title": title, "key": title} for title in titles
    ]}).encode(), "application/json")
    fixture.responses[first_path] = (first_text.encode(), "text/plain; charset=utf-8")
    fixture.responses[second_path] = (second_text.encode(), "text/plain; charset=utf-8")
    first_question = "本文では実測値について何と書かれていますか"
    second_question = "本文では予備値について何と書かれていますか"

    def load(reference: str) -> dict | list:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    def fetch_first(case: str) -> tuple[dict, dict]:
        listing = cli("run", "--input", "listing", *allow, "検索 wikipedia: candidate after analysis", case_id=case)
        first = cli("run", "--input", "first", *allow, "1件目の本文を取得して", case_id=case)
        return listing, first

    results = {}
    for name, query, blocker in (
        ("question", first_question, ("2", (), "READ_PERMISSION_REQUIRED")),
        ("summary", "取得した本文を要約して", ("3", allow, "SEARCH_CANDIDATE_UNAVAILABLE")),
        ("constrained", "取得した本文を2点以内、20字以内で要約して",
         ("2", (*allow, "--deadline-ms", "0"), "RESOURCE_LIMIT")),
        ("no-match", second_question, ("3", allow, "SEARCH_CANDIDATE_UNAVAILABLE")),
    ):
        case = f"candidate-after-source-{name}"
        seen, starts = len(fixture.events), len(fixture.connects)
        listing, first = fetch_first(case)
        expected = 2 if name == "no-match" else 0
        analysis = cli("run", "--input", "analysis", query, case_id=case, expected_exit=expected)
        assert analysis["evidence"] == []
        if name == "no-match":
            assert "SOURCE_QUESTION_NO_MATCH" in analysis["reason"]
            assert analysis["dependencies"] == [] and analysis["receipts"] == []
            assert cli("run", "--input", "analysis", query, case_id=case, expected_exit=2) == analysis
        else:
            assert first_text in analysis["text"]

        output = state_path.parent / f"{case}.txt"
        write = ("--output", str(output), "--allow-write", str(output)) if name in ("question", "no-match") else ()
        next_input = ("--input", "second", *allow, *write, "2件目の本文を取得して")
        selected = cli("run", *next_input, case_id=case)
        assert selected["guarantee"] == "SOURCE_REPORT"
        assert selected["evidence"][:-1] == first["evidence"]
        packet = selected["evidence"][-1]
        assert packet["packet_kind"] == "FETCHED_BODY"
        assert packet["final_uri"] == f"https://{host}{second_path}"
        assert packet["extracted_text"] == second_text and load(selected["dependencies"][-1]) == packet
        list_ref = listing["dependencies"][0]
        assert selected["dependencies"][:-1] == [first["id"], list_ref, first["dependencies"][-1]]
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database:
            (reference,) = database.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision DESC LIMIT 1", (case,),
            ).fetchone()
        state = load(reference)
        assert state["bundle"]["request"]["context_ref"] is None
        assert state["bundle"]["request"]["permissions"]["read_hosts"] == [host]
        assert state["bundle"]["meaning"]["context_result_ref"] == first["id"]
        assert state["prior_results"][analysis["id"]] == analysis
        assert cli("run", *next_input, case_id=case) == selected
        if write:
            before = output.stat()
            (receipt,) = selected["receipts"]
            retain_attempted_prefix(state_path, case, "second", receipt["action_id"])
            recovered = cli("reconcile", case_id=case)
            assert recovered["text"] == selected["text"] and recovered["evidence"] == selected["evidence"]
            assert recovered["dependencies"] == selected["dependencies"]
            (restored_receipt,) = recovered["receipts"]
            assert restored_receipt["action_id"] == receipt["action_id"] and "NO_RESEND" in restored_receipt["reason"]
            graph = load(recovered["execution_graph_ref"])
            assert graph[5]["search-candidate-body-2"] == {"search-candidate-listing": list_ref}
            assert output.read_bytes() == bytes(selected["render"]["bytes"])
            after = output.stat()
            assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)

        followup = cli("run", "--input", "followup", second_question, case_id=case)
        assert second_text in followup["text"] and first_text not in followup["text"]
        assert followup["dependencies"][:-1] == [selected["id"], *selected["dependencies"][-2:]]
        if name == "no-match":
            no_match = cli("run", "--input", "another-question", "本文では火山について何と書かれていますか",
                           case_id=case, expected_exit=2)
            assert "SOURCE_QUESTION_NO_MATCH" in no_match["reason"]
        rank, flags, reason = blocker
        stopped = cli("run", "--input", "blocked", *flags, f"{rank}件目の本文を取得して",
                      case_id=case, expected_exit=2)
        assert reason in stopped["reason"] and stopped["receipts"] == []
        assert [event["path"] for event in fixture.events[seen:]] == [listing_path, first_path, second_path]
        assert len(fixture.connects) == starts + 3
        results[name] = {"selected": selected, "followup": followup, "stopped": stopped}

    case = "candidate-after-direct-source"
    seen, starts = len(fixture.events), len(fixture.connects)
    cli("run", "--input", "direct", *allow, f"fetch https://{host}{first_path}", case_id=case)
    cli("run", "--input", "analysis", first_question, case_id=case)
    no_match = cli("run", "--input", "no-match", second_question, case_id=case, expected_exit=2)
    assert "SOURCE_QUESTION_NO_MATCH" in no_match["reason"]
    stopped = cli("run", "--input", "second", *allow, "2件目の本文を取得して", case_id=case, expected_exit=2)
    assert "MISSING_SEARCH_RESULT_CONTEXT" in stopped["reason"] and stopped["evidence"] == []
    assert [event["path"] for event in fixture.events[seen:]] == [first_path]
    assert len(fixture.connects) == starts + 1
    results["direct_source"] = stopped

    for name, reason in (("missing-grant", "READ_PERMISSION_REQUIRED"), ("deferred", "HTTP_RETRY_DEFERRED")):
        case = f"candidate-after-no-match-{name}"
        seen, starts = len(fixture.events), len(fixture.connects)
        _, first = fetch_first(case)
        expected_paths = [listing_path, first_path]
        flags = ()
        if name == "deferred":
            fixture.failure_responses[second_path] = ("429 Too Many Requests", b"Deferred body.", "Retry-After: 120\r\n")
            failed = cli("run", "--input", "defer-body", *allow, "2件目の本文を取得して", case_id=case, expected_exit=2)
            del fixture.failure_responses[second_path]
            assert failed["evidence"][:-1] == first["evidence"]
            failure = failed["evidence"][-1]
            assert failure["http_status"] == 429
            assert failure["retry_after"]["directive"] == {"kind": "DELAY_SECONDS", "seconds": 120}
            expected_paths.append(second_path)
            flags = allow
        no_match = cli("run", "--input", "no-match", second_question, case_id=case, expected_exit=2)
        assert "SOURCE_QUESTION_NO_MATCH" in no_match["reason"]
        stopped = cli("run", "--input", "next", *flags, "2件目の本文を取得して", case_id=case, expected_exit=2)
        assert reason in stopped["reason"] and stopped["receipts"] == []
        if name == "deferred":
            assert failure["id"] in stopped["reason"]
        assert [event["path"] for event in fixture.events[seen:]] == expected_paths
        assert len(fixture.connects) == starts + len(expected_paths)
        results[name] = stopped
    assert not fixture.errors, fixture.errors
    return results


def check_source_after_question_no_match(cli, fixture: Fixture, state_path: Path) -> dict:
    path = "/source-after-unmatched-question"
    body = "実測値は42です。補助値は7です。"
    uri = f"https://{fixture.host}{path}"
    fixture.responses[path] = (body.encode(), "text/plain; charset=utf-8")
    no_match = "本文では火山について何と書かれていますか"
    matched = "本文では実測値について何と書かれていますか"
    seen, starts = len(fixture.events), len(fixture.connects)

    def load(reference: str) -> dict | list:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    def fetch(case: str) -> dict:
        return cli("run", "--input", "source", "--allow-read-host", fixture.host,
                   f"fetch {uri}", case_id=case)

    def miss(case: str, *flags: str) -> dict:
        stopped = cli("run", "--input", "unmatched", *flags, no_match, case_id=case, expected_exit=2)
        assert "SOURCE_QUESTION_NO_MATCH" in stopped["reason"] and stopped["guarantee"] == "UNKNOWN"
        assert stopped["dependencies"] == [] and stopped["evidence"] == [] and stopped["receipts"] == []
        assert stopped["content_plan"] is None and stopped["render"] is None
        return stopped

    results = {}
    for name, query in (
        ("question", matched),
        ("summary", "取得した本文を要約して"),
        ("constrained", "取得した本文を2点以内、20字以内で要約して"),
    ):
        case = f"source-after-no-match-{name}"
        source = fetch(case)
        rejected_output = state_path.parent / f"{case}-unmatched.txt"
        rejected_write = ("--output", str(rejected_output), "--allow-write", str(rejected_output))
        stopped = miss(case, *rejected_write)
        assert miss(case, *rejected_write) == stopped
        assert not rejected_output.exists()

        output = state_path.parent / f"{case}.txt"
        write = ("--output", str(output), "--allow-write", str(output)) if name == "question" else ()
        result = cli("run", "--input", "new-question-or-summary", *write, query, case_id=case)
        assert result["guarantee"] == "HYPOTHESIS" and result["evidence"] == []
        assert "実測値は42です。" in result["text"] and "火山" not in result["text"] and uri in result["text"]
        prior_id, body_ref, report_ref = result["dependencies"]
        assert prior_id == source["id"] and [load(body_ref)] == source["evidence"]
        assert load(report_ref)["source_result_id"] == prior_id
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database:
            (reference,) = database.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision DESC LIMIT 1", (case,),
            ).fetchone()
        state = load(reference)
        assert state["bundle"]["request"]["context_ref"] is None
        assert state["bundle"]["request"]["permissions"]["read_hosts"] == []
        assert state["bundle"]["meaning"]["context_result_ref"] == source["id"]
        assert state["prior_results"][stopped["id"]] == stopped
        if write:
            before = output.stat()
            (receipt,) = result["receipts"]
            retain_attempted_prefix(state_path, case, "new-question-or-summary", receipt["action_id"])
            recovered = cli("reconcile", case_id=case)
            assert recovered["dependencies"] == result["dependencies"] and recovered["text"] == result["text"]
            (restored_receipt,) = recovered["receipts"]
            assert restored_receipt["action_id"] == receipt["action_id"] and "NO_RESEND" in restored_receipt["reason"]
            assert output.read_bytes() == bytes(result["render"]["bytes"])
            after = output.stat()
            assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)
        results[name] = {"stopped": stopped, "result": result}

    case = "source-after-unsupported-topic"
    fetch(case)
    unsupported = cli("run", "--input", "unsupported", "本文ではtheについて何と書かれていますか",
                      case_id=case, expected_exit=2)
    assert "SOURCE_QUESTION_TOPIC_UNSUPPORTED" in unsupported["reason"]
    stopped = cli("run", "--input", "next", matched, case_id=case, expected_exit=2)
    assert "MISSING_ACTUAL_RESULT" in stopped["reason"]

    case = "source-after-zero-budget"
    fetch(case)
    limited = cli("run", "--input", "no-budget", "--deadline-ms", "0", no_match,
                  case_id=case, expected_exit=2)
    assert "RESOURCE_LIMIT" in limited["reason"]
    stopped = cli("run", "--input", "next", matched, case_id=case, expected_exit=2)
    assert "MISSING_ACTUAL_RESULT" in stopped["reason"]

    case = "source-after-no-match-unbound-body"
    source = fetch(case)
    miss(case)
    (packet,) = source["evidence"]
    encoded = json.dumps(packet, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    body_ref = hashlib.sha256(encoded).hexdigest()
    assert load(body_ref) == packet
    # Keep the original bytes while removing the case binding in this disposable store.
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
        removed = database.execute("DELETE FROM case_artifact WHERE case_id=? AND sha256=?", (case, body_ref))
        assert removed.rowcount == 1
    try:
        invalid = cli("run", "--input", "next", matched, case_id=case, expected_exit=4)
        assert "INPUT_FAILURE" in invalid["reason"] and invalid["receipts"] == []
    finally:
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
            database.execute("INSERT INTO case_artifact(case_id,sha256) VALUES(?,?)", (case, body_ref))
    assert [event["path"] for event in fixture.events[seen:]] == [path] * 6
    assert len(fixture.connects) == starts + 6
    assert not fixture.errors, fixture.errors
    return {**results, "unsupported": unsupported, "limited": limited, "unbound": invalid}


def check_partial_direct_source(cli, fixture: Fixture, state_path: Path) -> dict:
    kept, absent = "/partial-source-kept", "/partial-source-absent"
    source_text = "実測値は42です。予備値は7です。"
    failed_text = "実測値は999です。秘匿値は88です。"
    fixture.responses[kept] = (source_text.encode(), "text/plain; charset=utf-8")
    fixture.failure_responses[absent] = ("404 Not Found", failed_text.encode(), "")
    source_uri, absent_uri = [f"https://{fixture.host}{path}" for path in (kept, absent)]
    grant = ("--allow-read-host", fixture.host)
    question = "本文では実測値について何と書かれていますか"
    summary = "取得した本文を要約して"
    seen, starts = len(fixture.events), len(fixture.connects)

    def fetch(case: str, *flags: str) -> dict:
        stopped = cli("run", "--input", "original", *grant, *flags, f"fetch {source_uri} {absent_uri}",
                      case_id=case, expected_exit=2)
        assert "FETCH_OR_PARSER_FAILURE" in stopped["reason"] and stopped["guarantee"] == "UNKNOWN"
        body, failed = stopped["evidence"]
        assert body["packet_kind"] == "FETCHED_BODY" and body["extracted_text"] == source_text
        assert failed["http_status"] == 404 and bytes(failed["raw_body"]) == failed_text.encode()
        assert stopped["dependencies"] == [body["id"], failed["id"]]
        assert stopped["content_plan"] is None and stopped["render"] is None and stopped["receipts"] == []
        return stopped

    def read_blob(reference: str) -> dict:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    results = {}
    for name, query in (("question", question), ("summary", summary)):
        case = f"partial-direct-{name}"
        original_file = state_path.parent / f"{case}-original.txt"
        original_flags = ("--output", str(original_file), "--allow-write", str(original_file))
        partial = fetch(case, *original_flags)
        assert fetch(case, *original_flags) == partial
        assert not original_file.exists()

        output = state_path.parent / f"{case}.txt"
        flags = ("--output", str(output), "--allow-write", str(output))
        result = cli("run", "--input", "followup", *flags, query, case_id=case)
        prior_id, body_ref, report_ref = result["dependencies"]
        assert prior_id == partial["id"] and read_blob(body_ref) == partial["evidence"][0]
        assert read_blob(report_ref)["source_result_id"] == prior_id
        assert result["guarantee"] == "HYPOTHESIS" and result["evidence"] == []
        assert "元の取得は一部失敗しました。" in result["text"]
        assert source_uri in result["text"] and "実測値は42です。" in result["text"]
        assert absent_uri not in result["text"] and "999" not in result["text"] and "秘匿値" not in result["text"]
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database:
            (reference,) = database.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision DESC LIMIT 1", (case,),
            ).fetchone()
        state = read_blob(reference)
        assert state["bundle"]["request"]["permissions"]["read_hosts"] == []
        assert state["bundle"]["meaning"]["context_result_ref"] == prior_id
        assert state["prior_results"][prior_id] == partial
        before = output.stat()
        (receipt,) = result["receipts"]
        retain_attempted_prefix(state_path, case, "followup", receipt["action_id"])
        recovered = cli("reconcile", case_id=case)
        assert recovered["dependencies"] == result["dependencies"] and recovered["text"] == result["text"]
        (restored_receipt,) = recovered["receipts"]
        assert restored_receipt["action_id"] == receipt["action_id"] and "NO_RESEND" in restored_receipt["reason"]
        assert output.read_bytes() == bytes(result["render"]["bytes"])
        after = output.stat()
        assert (after.st_ino, after.st_mtime_ns) == (before.st_ino, before.st_mtime_ns)
        assert not original_file.exists()
        results[name] = recovered

        if name == "question":
            constrained = cli("run", "--input", "constrained", "取得した本文を2点以内、20字以内で要約して",
                              case_id=case)
            assert constrained["dependencies"][:2] == [prior_id, body_ref]
            assert "実測値は42です。" in constrained["text"] and "予備値は7です。" in constrained["text"]
            no_match = cli("run", "--input", "missed", "本文では秘匿値について何と書かれていますか",
                           case_id=case, expected_exit=2)
            assert "SOURCE_QUESTION_NO_MATCH" in no_match["reason"] and no_match["dependencies"] == []
            again = cli("run", "--input", "again", question, case_id=case)
            assert again["dependencies"][:2] == [prior_id, body_ref] and again["text"] == result["text"]

    case = "partial-direct-no-success"
    cli("run", "--input", "original", *grant, f"fetch {absent_uri}", case_id=case, expected_exit=2)
    unavailable = cli("run", "--input", "followup", question, case_id=case, expected_exit=2)
    assert "MISSING_ACTUAL_RESULT" in unavailable["reason"] and unavailable["dependencies"] == []

    case = "partial-direct-unbound-packet"
    stopped = fetch(case)
    failed = stopped["evidence"][-1]
    encoded = json.dumps(failed, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    failed_ref = hashlib.sha256(encoded).hexdigest()
    assert read_blob(failed_ref) == failed
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
        removed = database.execute("DELETE FROM case_artifact WHERE case_id=? AND sha256=?", (case, failed_ref))
        assert removed.rowcount == 1
    try:
        invalid = cli("run", "--input", "followup", summary, case_id=case, expected_exit=4)
        assert "SOURCE_CONTEXT_INVALID" in invalid["reason"] and invalid["receipts"] == []
    finally:
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
            database.execute("INSERT INTO case_artifact(case_id,sha256) VALUES(?,?)", (case, failed_ref))
    assert [event["path"] for event in fixture.events[seen:]] == [kept, absent] * 2 + [absent, kept, absent]
    assert len(fixture.connects) == starts + 7
    assert not fixture.errors, fixture.errors
    return {**results, "unavailable": unavailable, "unbound": invalid}


def check_source_after_redirect_wait(cli, fixture: Fixture, state_path: Path) -> dict:
    origin, held = fixture.hosts
    grant = ("--allow-read-host", origin, "--allow-read-host", held)
    kept, own, retained, later = (f"/wait-source-{part}" for part in ("kept", "own", "retained", "later"))
    target = f"https://{held}/wait-source-target"
    kept_text = "実測値は42です。予備値は7です。"
    question = "本文では実測値について何と書かれていますか"
    summary = "取得した本文を要約して"
    fixture.responses[kept] = (kept_text.encode(), "text/plain; charset=utf-8")
    fixture.responses[later] = ("未取得の値は999です。".encode(), "text/plain; charset=utf-8")
    fixture.redirects[retained] = target
    fixture.failure_responses[own] = ("302 Found", b"", f"Location: {target}\r\nRetry-After: 120\r\n")

    def seed(case: str, status: str) -> dict:
        path = f"/wait-source-{status[:3]}"
        fixture.failure_responses[path] = (status, b"", "Retry-After: 120\r\n")
        result = cli("run", "--input", "held", "--allow-read-host", held, f"fetch https://{held}{path}",
                     case_id=case, expected_exit=2)
        (packet,) = result["evidence"]
        return packet

    def fetch(case: str, where: str, *flags: str) -> tuple[dict, tuple[str, ...]]:
        args = ("--input", "original", *grant, *flags,
                f"fetch https://{origin}{kept} {where} https://{origin}{later}")
        return cli("run", *args, case_id=case, expected_exit=2), args

    def read_blob(reference: str) -> dict:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    def assert_contacts(seen: int, starts: int, paths: list[str]) -> None:
        assert [(event["host"], event["path"]) for event in fixture.events[seen:]] == [(origin, path) for path in paths]
        assert len(fixture.connects) == starts + len(paths)

    results = {}
    for name, path, status, query in (
        ("own-wait", own, None, summary),
        ("retained-429", retained, "429 Too Many Requests", question),
        ("retained-503", retained, "503 Service Unavailable", "取得した本文を2点以内、20字以内で要約して"),
    ):
        case = f"source-after-{name}"
        previous_wait = seed(case, status) if status else None
        seen, starts = len(fixture.events), len(fixture.connects)
        original_file = state_path.parent / f"{case}-original.txt"
        stopped, args = fetch(case, f"https://{origin}{path}", "--output", str(original_file), "--allow-write", str(original_file))
        code = "HTTP_RETRY_DEFERRED" if status else "HTTP_REDIRECT_RETRY_DEFERRED"
        assert stopped["reason"].startswith(f"{code}: ") and stopped["receipts"] == []
        assert stopped["content_plan"] is None and stopped["render"] is None
        kept_packet, redirect_packet = stopped["evidence"]
        assert kept_packet["extracted_text"] == kept_text
        assert redirect_packet["http_status"] == 302 and redirect_packet["failure"] == code
        assert redirect_packet["redirect_chain"] == [f"https://{origin}{path}"]
        assert (redirect_packet.get("retry_after") is None) == (status is not None)

        output = state_path.parent / f"{case}.txt"
        result = cli("run", "--input", "followup", "--output", str(output), "--allow-write", str(output), query, case_id=case)
        prior_id, body_ref, report_ref = result["dependencies"]
        assert prior_id == stopped["id"] and read_blob(body_ref) == kept_packet
        assert read_blob(report_ref)["source_result_id"] == prior_id
        assert result["guarantee"] == "HYPOTHESIS" and result["evidence"] == []
        assert "元の取得は転送時の待機条件で停止しました。" in result["text"]
        assert "実測値は42です。" in result["text"] and "999" not in result["text"] and target not in result["text"]
        before = output.stat()
        (receipt,) = result["receipts"]
        retain_attempted_prefix(state_path, case, "followup", receipt["action_id"])
        recovered = cli("reconcile", case_id=case)
        (restored,) = recovered["receipts"]
        assert restored["action_id"] == receipt["action_id"] and "NO_RESEND" in restored["reason"]
        assert recovered["text"] == result["text"] and recovered["dependencies"] == result["dependencies"]
        assert output.read_bytes() == bytes(result["render"]["bytes"])
        after = output.stat()
        assert (before.st_ino, before.st_mtime_ns) == (after.st_ino, after.st_mtime_ns)
        assert cli("run", *args, case_id=case, expected_exit=2) == stopped
        assert not original_file.exists()
        if previous_wait:
            wait_result = cli("run", "--input", "confirm-held", "--allow-read-host", held,
                              f"fetch {target}", case_id=case, expected_exit=2)
            assert "HTTP_RETRY_DEFERRED" in wait_result["reason"] and previous_wait["id"] in wait_result["reason"]
        assert_contacts(seen, starts, [kept, path])
        results[name] = recovered

    case = "source-after-first-hop-denial"
    seed(case, "429 Too Many Requests")
    seen, starts = len(fixture.events), len(fixture.connects)
    stopped = cli("run", "--input", "original", "--allow-read-host", origin,
                  f"fetch https://{origin}{kept} {target}", case_id=case, expected_exit=2)
    assert "EVIDENCE_UNAVAILABLE: READ_PERMISSION_REQUIRED" in stopped["reason"] and len(stopped["evidence"]) == 1
    unavailable = cli("run", "--input", "followup", question, case_id=case, expected_exit=2)
    assert "MISSING_ACTUAL_RESULT" in unavailable["reason"]
    assert_contacts(seen, starts, [kept])

    case = "source-after-empty-redirect-prefix"
    seen, starts = len(fixture.events), len(fixture.connects)
    stopped = cli("run", "--input", "original", *grant, f"fetch https://{origin}{own} https://{origin}{later}",
                  case_id=case, expected_exit=2)
    assert len(stopped["evidence"]) == 1 and "HTTP_REDIRECT_RETRY_DEFERRED" in stopped["reason"]
    empty = cli("run", "--input", "followup", summary, case_id=case, expected_exit=2)
    assert "MISSING_ACTUAL_RESULT" in empty["reason"]
    assert_contacts(seen, starts, [own])

    case = "source-after-unbound-redirect"
    seen, starts = len(fixture.events), len(fixture.connects)
    stopped, _ = fetch(case, f"https://{origin}{own}")
    packet_bytes = json.dumps(stopped["evidence"][-1], ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    reference = hashlib.sha256(packet_bytes).hexdigest()
    assert read_blob(reference) == stopped["evidence"][-1]
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
        removed = database.execute("DELETE FROM case_artifact WHERE case_id=? AND sha256=?", (case, reference))
        assert removed.rowcount == 1
    try:
        invalid = cli("run", "--input", "followup", question, case_id=case, expected_exit=4)
        assert "SOURCE_CONTEXT_INVALID" in invalid["reason"] and invalid["receipts"] == []
    finally:
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
            database.execute("INSERT INTO case_artifact(case_id,sha256) VALUES(?,?)", (case, reference))
    assert_contacts(seen, starts, [kept, own])
    assert not fixture.errors, fixture.errors
    return {**results, "read_denied": unavailable, "empty": empty, "unbound": invalid}


def check_source_after_first_hop_wait(cli, fixture: Fixture, state_path: Path) -> dict:
    origin, held = fixture.hosts
    grant = ("--allow-read-host", origin, "--allow-read-host", held)
    kept, rate, target, later = [f"/first-hop-{name}" for name in ("kept", "rate", "target", "later")]
    search_path = "/works?query=first+hop+wait&rows=5"
    held_uri = f"https://{held}{target}"
    kept_text = "実測値は42です。予備値は7です。"
    fixture.responses[kept] = (kept_text.encode(), "text/plain; charset=utf-8")
    fixture.responses[later] = ("未取得の値は999です。".encode(), "text/plain; charset=utf-8")
    fixture.failure_responses[rate] = ("503 Service Unavailable", b"Wait response.", "Retry-After: 120\r\n")
    fixture.failure_responses[search_path] = ("429 Too Many Requests", b"Search wait.", "Retry-After: 120\r\n")
    question = "本文では実測値について何と書かれていますか"

    def seed(case: str, search: bool = False) -> dict:
        text = "検索 crossref: first hop wait" if search else f"fetch https://{held}{rate}"
        result = cli("run", "--input", "prior-wait", "--allow-read-host", held, text, case_id=case, expected_exit=2)
        (packet,) = result["evidence"]
        assert packet["packet_kind"] == ("SEARCH_LIST" if search else "FETCH_FAILURE")
        return packet

    def fetch(case: str, same_goal: bool, *flags: str) -> tuple[dict, tuple[str, ...]]:
        urls = [f"https://{origin}{kept}"]
        if same_goal:
            urls.append(f"https://{held}{rate}")
        urls.extend((held_uri, f"https://{origin}{later}"))
        args = ("--input", "original", *grant, *flags, f"fetch {' '.join(urls)}")
        result = cli("run", *args, case_id=case, expected_exit=2)
        assert result["reason"].startswith(f"EVIDENCE_UNAVAILABLE: HTTP_RETRY_DEFERRED: The retained Retry-After for {held}")
        assert result["content_plan"] is None and result["render"] is None and result["receipts"] == []
        assert len(result["evidence"]) == (2 if same_goal else 1)
        assert result["evidence"][0]["extracted_text"] == kept_text
        assert result["dependencies"] == [packet["id"] for packet in result["evidence"]]
        return result, args

    results = {}
    for name, query in (
        ("saved-fetch", question),
        ("same-goal", "取得した本文を要約して"),
        ("saved-search", "取得した本文を2点以内、20字以内で要約して"),
    ):
        case = f"source-after-first-hop-{name}"
        same_goal = name == "same-goal"
        wait = None if same_goal else seed(case, search=name == "saved-search")
        seen, starts = len(fixture.events), len(fixture.connects)
        original_file = state_path.parent / f"{case}-original.txt"
        stopped, args = fetch(case, same_goal, "--output", str(original_file), "--allow-write", str(original_file))
        if wait:
            assert f"(saved packet {wait['id']})" in stopped["reason"]
        else:
            assert "(saved packet " not in stopped["reason"]
            wait = stopped["evidence"][1]
            assert wait["http_status"] == 503 and wait["retry_after"] is not None
        expected = [(origin, kept), (held, rate)] if same_goal else [(origin, kept)]

        output = state_path.parent / f"{case}.txt"
        answer = cli("run", "--input", "followup", "--output", str(output), "--allow-write", str(output), query, case_id=case)
        source_id, body_ref, _ = answer["dependencies"]
        raw = (state_path / "blobs" / body_ref).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == body_ref and json.loads(raw) == stopped["evidence"][0]
        assert source_id == stopped["id"] and "元の取得は次のURLの接続前に待機条件で停止しました。" in answer["text"]
        assert "実測値は42です。" in answer["text"] and "999" not in answer["text"] and answer["evidence"] == []
        before = output.stat()
        (receipt,) = answer["receipts"]
        retain_attempted_prefix(state_path, case, "followup", receipt["action_id"])
        recovered = cli("reconcile", case_id=case)
        (restored,) = recovered["receipts"]
        assert restored["action_id"] == receipt["action_id"] and "NO_RESEND" in restored["reason"]
        assert recovered["dependencies"] == answer["dependencies"] and recovered["text"] == answer["text"]
        assert output.read_bytes() == bytes(answer["render"]["bytes"])
        after = output.stat()
        assert (before.st_ino, before.st_mtime_ns) == (after.st_ino, after.st_mtime_ns)
        assert not original_file.exists()
        assert cli("run", *args, case_id=case, expected_exit=2) == stopped
        blocked = cli("run", "--input", "still-held", "--allow-read-host", held, f"fetch {held_uri}",
                      case_id=case, expected_exit=2)
        assert f"(saved packet {wait['id']})" in blocked["reason"] and blocked["evidence"] == []
        assert [(event["host"], event["path"]) for event in fixture.events[seen:]] == expected
        assert len(fixture.connects) == starts + len(expected)
        results[name] = recovered

    case = "source-after-first-hop-unbound-wait"
    wait = seed(case)
    seen, starts = len(fixture.events), len(fixture.connects)
    fetch(case, False)
    packet_bytes = json.dumps(wait, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    wait_ref = hashlib.sha256(packet_bytes).hexdigest()
    assert (state_path / "blobs" / wait_ref).read_bytes() == packet_bytes
    with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
        removed = database.execute("DELETE FROM case_artifact WHERE case_id=? AND sha256=?", (case, wait_ref))
        assert removed.rowcount == 1
    try:
        invalid = cli("run", "--input", "followup", question, case_id=case, expected_exit=4)
        assert "SOURCE_CONTEXT_INVALID" in invalid["reason"] and invalid["receipts"] == []
    finally:
        with closing(sqlite3.connect(state_path / "state.sqlite3")) as database, database:
            database.execute("INSERT INTO case_artifact(case_id,sha256) VALUES(?,?)", (case, wait_ref))
    assert [(event["host"], event["path"]) for event in fixture.events[seen:]] == [(origin, kept)]
    assert len(fixture.connects) == starts + 1 and not fixture.errors, fixture.errors
    return {**results, "unbound": invalid}


def check_source_after_deadline(cli, fixture: Fixture, state_path: Path) -> dict:
    kept, slow, pending = "/deadline-kept", "/slow-body", "/deadline-pending"
    kept_text = "実測値は42です。予備値は7です。"
    fixture.responses[kept] = (kept_text.encode(), "text/plain; charset=utf-8")
    fixture.responses[pending] = (b"Unrequested 999.", "text/plain")
    kept_uri, slow_uri, pending_uri = [f"https://{fixture.host}{path}" for path in (kept, slow, pending)]
    deadline_stop = "EVIDENCE_UNAVAILABLE: HTTP_DEADLINE_EXCEEDED: The retrieval deadline elapsed"
    question = "本文では実測値について何と書かれていますか"
    summary = "取得した本文を要約して"
    constrained = "取得した本文を2点以内、20字以内で要約して"
    seen, starts = len(fixture.events), len(fixture.connects)

    def arguments(paths: str) -> tuple[str, ...]:
        return ("--input", "original", "--allow-read-host", fixture.host, "--deadline-ms", "1200", f"fetch {paths}")

    def read_blob(reference: str) -> dict:
        raw = (state_path / "blobs" / reference).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == reference
        return json.loads(raw)

    outcomes = {}
    for name, query in (("question", question), ("summary", summary), ("constrained", constrained)):
        case = f"source-after-deadline-{name}"
        original_file = state_path.parent / f"{case}-original.txt"
        args = (*arguments(f"{kept_uri} {slow_uri} {pending_uri}"),
                "--output", str(original_file), "--allow-write", str(original_file))
        stopped = cli("run", *args, case_id=case, expected_exit=2)
        assert stopped["reason"] == deadline_stop and stopped["guarantee"] == "UNKNOWN"
        assert stopped["content_plan"] is None and stopped["render"] is None and stopped["receipts"] == []
        body, failed = stopped["evidence"]
        assert body["request_uri"] == kept_uri and body["extracted_text"] == kept_text
        assert failed["request_uri"] == slow_uri and failed["http_status"] == 200
        assert failed["packet_kind"] == "FETCH_FAILURE" and failed["failure"] == "HTTP_DEADLINE_EXCEEDED"
        assert bytes(failed["raw_body"]) == TIMED_BODY_PREFIX and failed["extracted_text"] is None
        assert stopped["dependencies"] == [body["id"], failed["id"]]
        assert not original_file.exists()

        output = state_path.parent / f"{case}.txt"
        answer = cli("run", "--input", "followup", "--output", str(output), "--allow-write", str(output),
                     query, case_id=case)
        source_id, body_ref, report_ref = answer["dependencies"]
        assert source_id == stopped["id"] and read_blob(body_ref) == body
        assert read_blob(report_ref)["source_result_id"] == source_id
        assert answer["guarantee"] == "HYPOTHESIS" and answer["evidence"] == []
        assert "元の取得は期限に達して停止しました。" in answer["text"]
        assert kept_uri in answer["text"] and "実測値は42です。" in answer["text"]
        assert all(text not in answer["text"] for text in (slow_uri, pending_uri, TIMED_BODY_PREFIX.decode()))
        (receipt,) = answer["receipts"]
        before = output.stat()
        retain_attempted_prefix(state_path, case, "followup", receipt["action_id"])
        recovered = cli("reconcile", case_id=case)
        (restored,) = recovered["receipts"]
        assert restored["action_id"] == receipt["action_id"] and "NO_RESEND" in restored["reason"]
        assert recovered["dependencies"] == answer["dependencies"] and recovered["text"] == answer["text"]
        assert output.read_bytes() == bytes(answer["render"]["bytes"])
        after = output.stat()
        assert (before.st_ino, before.st_mtime_ns, before.st_size) == (after.st_ino, after.st_mtime_ns, after.st_size)
        assert cli("run", *args, case_id=case, expected_exit=2) == stopped
        assert not original_file.exists()
        outcomes[name] = recovered

    case = "source-after-deadline-no-success"
    stopped = cli("run", *arguments(f"{slow_uri} {pending_uri}"), case_id=case, expected_exit=2)
    assert stopped["reason"] == deadline_stop
    (timed_out,) = stopped["evidence"]
    assert timed_out["failure"] == "HTTP_DEADLINE_EXCEEDED" and timed_out["extracted_text"] is None
    unavailable = cli("run", "--input", "followup", question, case_id=case, expected_exit=2)
    assert "MISSING_ACTUAL_RESULT" in unavailable["reason"] and unavailable["dependencies"] == []
    expected_paths = [kept, slow] * 3 + [slow]
    assert [event["path"] for event in fixture.events[seen:]] == expected_paths
    assert len(fixture.connects) == starts + len(expected_paths) and not fixture.errors, fixture.errors
    return {**outcomes, "no_success": unavailable}


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
    additional_hosts = ("api.crossref.org",)
    certificate, key, ca = certificates(directory, host, additional_hosts)
    fixture = Fixture(certificate, key, host, responses, additional_hosts)
    state_path, output = directory / "state", directory / "selected-body.txt"
    environment = os.environ.copy()
    for name in ("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"):
        environment[name] = fixture.proxy
    environment["NO_PROXY"] = environment["no_proxy"] = ""
    environment["SSL_CERT_FILE"] = str(ca)

    def command_line(command: str, *arguments: str, case_id: str = case,
                     actor_id: str = "fixture-local", db_path: Path = state_path) -> list[str]:
        return [str(binary), command, "--db", str(db_path), "--case", case_id,
                "--actor", actor_id, "--json", *arguments]

    def cli(command: str, *arguments: str, case_id: str = case,
            actor_id: str = "fixture-local", expected_exit: int = 0, db_path: Path = state_path) -> dict:
        process = subprocess.run(
            command_line(command, *arguments, case_id=case_id, actor_id=actor_id, db_path=db_path),
            env=environment, capture_output=True, text=True, timeout=15, check=False,
        )
        assert process.returncode == expected_exit, (process.returncode, process.stdout, process.stderr)
        result = json.loads(process.stdout)
        assert result["lifecycle"] == {0: "COMMIT", 2: "SUSPEND", 3: "REJECT", 4: "FAIL", 5: "CANCEL"}[expected_exit]
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
        candidate_events = list(fixture.events)
        fixture.responses["/w/rest.php/v1/search/page?q=malformed+fixture&limit=5"] = (
            b'{"pages":"not an array"}', "application/json",
        )
        search_goals = check_search_goals(cli, fixture, state_path, directory)
        retry_continuation = check_retry_continuation(cli, fixture, search_goals["retry_after"][0])
        search_fallback = check_ordered_search(cli, fixture, state_path, directory)
        interrupted_retrieval = check_interrupted_retrieval(cli, command_line, environment, fixture, state_path)
        cancelled_redirects = check_redirect_cancellation(cli, command_line, environment, fixture, state_path)
        versioned_summaries = check_versioned_source_summary(cli, fixture, state_path)
        deferred_redirects = check_redirect_retry_after(cli, fixture, state_path)
        retained_redirect_wait = check_retained_redirect_wait(cli, fixture, state_path)
        after_failure = check_candidate_after_failure(cli, fixture, state_path)
        sources_after_failure = check_sources_after_candidate_failure(cli, fixture, state_path)
        source_followup_chain = check_source_followup_chain(cli, fixture, state_path)
        candidate_after_source = check_candidate_after_source_followup(cli, fixture, state_path)
        source_after_no_match = check_source_after_question_no_match(cli, fixture, state_path)
        partial_direct_source = check_partial_direct_source(cli, fixture, state_path)
        wait_state = directory / "wait-source-state"
        source_after_wait = check_source_after_redirect_wait(partial(cli, db_path=wait_state), fixture, wait_state)
        first_hop_state = directory / "first-hop-source-state"
        source_after_first_hop = check_source_after_first_hop_wait(partial(cli, db_path=first_hop_state), fixture, first_hop_state)
        deadline_state = directory / "deadline-source-state"
        source_after_deadline = check_source_after_deadline(partial(cli, db_path=deadline_state), fixture, deadline_state)
        return {"result": selected, "recovered": recovered, "observed_requests": candidate_events,
                "search_goals": search_goals, "retry_continuation": retry_continuation,
                "search_fallback": search_fallback, "interrupted_retrieval": interrupted_retrieval,
                "cancelled_redirects": cancelled_redirects, "deferred_redirects": deferred_redirects,
                "versioned_summaries": versioned_summaries, "after_failure": after_failure,
                "retained_redirect_wait": retained_redirect_wait,
                "sources_after_failure": sources_after_failure,
                "source_followup_chain": source_followup_chain,
                "candidate_after_source": candidate_after_source,
                "source_after_no_match": source_after_no_match,
                "partial_direct_source": partial_direct_source, "source_after_wait": source_after_wait,
                "source_after_first_hop": source_after_first_hop, "source_after_deadline": source_after_deadline}
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
        fixture = Fixture(certificate, key, responses={
            "/budget-second": (b"Second approved body.", "text/plain"),
        })
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
            cases.extend(check_retrieval_budget(binary, directory, fixture, ca))
            cases.extend(check_retry_after(binary, directory, fixture, ca))
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
