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

    def cli(command: str, *arguments: str, case_id: str = case,
            actor_id: str = "fixture-local", expected_exit: int = 0) -> dict:
        process = subprocess.run(
            [str(binary), command, "--db", str(state_path), "--case", case_id,
             "--actor", actor_id, "--json", *arguments],
            env=environment, capture_output=True, text=True, timeout=15, check=False,
        )
        assert process.returncode == expected_exit, (process.returncode, process.stdout, process.stderr)
        result = json.loads(process.stdout)
        assert result["lifecycle"] == {0: "COMMIT", 2: "SUSPEND", 3: "REJECT", 5: "CANCEL"}[expected_exit]
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
        return {"result": selected, "recovered": recovered, "observed_requests": candidate_events,
                "search_goals": search_goals, "retry_continuation": retry_continuation,
                "search_fallback": search_fallback}
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
