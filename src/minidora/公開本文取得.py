"""公開HTTPSから静的な本文を読む。検索・知識の採否・ブラウザ操作とは別責任。"""
from __future__ import annotations

from copy import copy
from dataclasses import dataclass
from datetime import datetime, timezone
from email.message import Message
from email.utils import parsedate_to_datetime
from hashlib import sha256
from html.parser import HTMLParser
from http.client import HTTPSConnection
import ipaddress
import json
import math
import re
import socket
import ssl
import time
from threading import Lock
from urllib.parse import quote, urljoin, urlsplit, urlunsplit
from .取得実行制御 import 共有取得制御, 取得資源不足

本文取得版 = "MINIDORA-公開本文取得-v0.2"


class 本文取得失敗(ValueError):
    """例外本文にサーバ応答や認証情報を混ぜず、理由コードだけを持つ。"""


def 公開URL(値: str) -> str:
    if type(値) is not str or not 0 < len(値) <= 4096:
        raise 本文取得失敗("URL型・長さ不正")
    if re.search(r"[\s\x00-\x1f\x7f\\]", 値):
        raise 本文取得失敗("URL制御文字")
    try:
        u = urlsplit(値)
        if u.scheme != "https" or not u.hostname or u.username is not None or u.password is not None:
            raise ValueError()
        if u.port not in (None, 443) or "%" in u.hostname:
            raise ValueError()
        host = u.hostname.encode("idna").decode("ascii").lower().rstrip(".")
        try:
            address = ipaddress.ip_address(host)
        except ValueError:
            if len(host) > 253 or not all(re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", x)
                                          for x in host.split(".")):
                raise ValueError()
        else:
            if not address.is_global or address.is_multicast:
                raise ValueError()
        authority = f"[{host}]" if ":" in host else host
        return urlunsplit(("https", authority, quote(u.path or "/", safe="/%:@!$&'()*+,;=-._~"),
                           quote(u.query, safe="%=&?/:@!$'()*+,;+-._~"), ""))
    except (ValueError, UnicodeError) as exc:
        raise 本文取得失敗("公開HTTPSの標準ポートURLが必要") from exc


def _公開アドレス(host: str) -> str:
    """DNS結果をすべて検査し、接続先をその検査済みIPへ固定する。"""
    rows = socket.getaddrinfo(host, 443, type=socket.SOCK_STREAM)
    addresses = sorted({row[4][0] for row in rows})
    if not addresses:
        raise 本文取得失敗("DNS結果なし")
    for value in addresses:
        ip = ipaddress.ip_address(value)
        if not ip.is_global or ip.is_multicast:
            raise 本文取得失敗("非公開アドレスへの接続禁止")
    return addresses[0]


class _本文抽出(HTMLParser):
    _除外 = {"head", "script", "style", "noscript", "template", "svg", "canvas", "table", "rp"}
    _非文字 = {"img", "iframe", "object", "embed", "audio", "video"}
    _区切 = {"p", "div", "section", "article", "main", "h1", "h2", "h3", "h4", "li", "ul", "ol", "pre", "blockquote", "br", "hr"}
    _空要素 = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"}
    _見出し = {"h1", "h2", "h3", "h4", "h5", "h6"}
    _見出し行内 = {"a", "span", "b", "strong", "em", "i", "u", "s", "small", "abbr",
                   "code", "kbd", "samp", "var", "sub", "sup", "mark", "dfn", "time", "cite"}

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.抑止: list[str] = []
        self.除外: set[str] = set()
        self.断片: list[str] = []
        self.題名: list[str] = []
        self._題名内 = False
        self._ルビ深さ = 0
        self._注記深さ = 0
        self._零ルビ開始: int | None = None
        self._見出し候補 = None

    def _見出し開始(self, tag):
        候補 = self._見出し候補
        if 候補 is not None:
            if tag not in self._見出し行内:
                候補["証明可能"] = False
            if tag not in self._空要素:
                候補["積み"].append(tag)
        elif tag in self._見出し and not self.抑止 and not self._題名内:
            self._見出し候補 = {"タグ": tag, "開始": len(self.断片), "積み": [tag], "証明可能": True}

    def _見出し終了(self, tag):
        候補 = self._見出し候補
        if 候補 is None:
            return
        if 候補["積み"][-1] == tag:
            候補["積み"].pop()
            if 候補["積み"]:
                return
        else:
            候補["証明可能"] = False
            if tag != 候補["タグ"]:
                return
        if 候補["証明可能"]:
            原文字 = "".join(self.断片[候補["開始"]:]).strip()
            if 原文字 and not any(字 in 原文字 for 字 in "\r\n\v\f\x85\u2028\u2029"):
                # 閉じた原hタグと単一文字行の型だけを、既存ATX節境界へ渡す。
                # 普通行・未閉鎖・不整合・非文字/抑止域を見出しへ補完しない。
                見出し = "#" * int(候補["タグ"][1]) + " " + " ".join(原文字.split())
                self.断片[候補["開始"]:] = ["\n", 見出し, "\n"]
        self._見出し候補 = None

    @staticmethod
    def _零サイズルビ(属性):
        # CSS全体を推定しない。単一の明示font-sizeだけを扱い、上書きは保全する。
        形式 = [値 for 鍵, 値 in 属性 if 鍵 == "style"]
        if len(形式) != 1 or 形式[0] is None:
            return False
        宣言 = [項.split(":", 1) for 項 in 形式[0].split(";") if ":" in 項]
        字体 = [値.strip().lower() for 鍵, 値 in 宣言 if 鍵.strip().lower() == "font-size"]
        return (len(字体) == 1 and re.fullmatch(r"0px(?:\s*!important)?", 字体[0]) is not None
                and not any(鍵.strip().lower() in ("font", "all") for 鍵, _ in 宣言))

    def handle_starttag(self, tag, attrs):
        self._見出し開始(tag)
        # 子構造は文字サイズ等を上書きし得る。注記全体を通常の原文字として保全する。
        self._零ルビ開始 = None
        if tag == "ruby":
            self._ルビ深さ += 1
        零注記 = (tag == "rt" and self._ルビ深さ > 0 and self._注記深さ == 0
                  and not self.抑止 and self._零サイズルビ(attrs))
        if tag == "rt":
            self._注記深さ += 1
        if 零注記:
            # 単純な文字注記の閉鎖を観測するまで保存し、未閉鎖なら原文を残す。
            self._零ルビ開始 = len(self.断片)
        if tag in self._非文字:
            # 明示した空alt画像以外を、意味的に無関係とは決め付けない。
            if tag != "img" or dict(attrs).get("alt") != "":
                self.除外.add(tag)
        if tag == "title":
            self._題名内 = True
        if tag in self._除外:
            self.除外.add(tag)
            self.抑止.append(tag)
        elif self.抑止 and tag not in self._空要素:
            self.抑止.append(tag)
        elif not self.抑止 and tag in self._区切:
            self.断片.append("\n")

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        self.handle_endtag(tag)

    def handle_endtag(self, tag):
        if self._零ルビ開始 is not None:
            if tag == "rt":
                del self.断片[self._零ルビ開始:]
                self.除外.add("rt")
            self._零ルビ開始 = None
        if tag == "rt" and self._注記深さ:
            self._注記深さ -= 1
        if tag == "ruby" and self._ルビ深さ:
            self._ルビ深さ -= 1
        if tag == "title":
            self._題名内 = False
        if self.抑止:
            if tag in self.抑止:
                # 対応する最後の開始タグまで閉じる。未閉鎖の除外域は保守的に残す。
                i = len(self.抑止) - 1 - self.抑止[::-1].index(tag)
                del self.抑止[i:]
        elif tag in self._区切:
            self.断片.append("\n")
        self._見出し終了(tag)

    def handle_data(self, 資料):
        if self._題名内:
            self.題名.append(資料)
        elif not self.抑止:
            self.断片.append(資料)

    def 本文(self) -> str:
        return "\n".join(x for line in "".join(self.断片).splitlines() if (x := " ".join(line.split())))


@dataclass(frozen=True, slots=True)
class 取得本文:
    要求URL: str
    最終URL: str
    題名: str
    本文: str
    取得時刻: str
    内容種別: str
    文字コード: str
    転送バイト数: int
    転送SHA256: str
    本文SHA256: str
    経路: tuple[str, ...]
    除外要素: tuple[str, ...] = ()
    抽出頁範囲: tuple[tuple[int, int, int], ...] = ()


def 本文を復号(要求URL: str, 経路: tuple[str, ...], headers: dict[str, str], raw: bytes) -> 取得本文:
    msg = Message()
    msg["content-type"] = headers.get("content-type", "application/octet-stream")
    mime = msg.get_content_type()
    if mime not in ("text/html", "text/plain", "text/markdown"):
        raise 本文取得失敗("未対応内容種別")
    if headers.get("content-encoding", "identity").lower() not in ("", "identity"):
        raise 本文取得失敗("圧縮応答は未対応")
    encoding = msg.get_content_charset()
    if encoding is None and mime == "text/html":
        meta = re.search(br'<meta\b[^>]*\bcharset\s*=\s*[\x22\x27]?([a-zA-Z0-9_-]+)', raw[:4096], re.I)
        encoding = meta[1].decode("ascii") if meta else None
    encoding = encoding or "utf-8-sig"
    if encoding.lower().replace("_", "-") not in (
        "utf-8", "utf-8-sig", "ascii", "us-ascii", "shift-jis", "sjis", "cp932", "euc-jp", "iso-8859-1"):
        raise 本文取得失敗("未対応文字コード")
    if encoding.lower().replace("_", "-") == "utf-8" and raw.startswith(b"\xef\xbb\xbf"):
        # 先頭の符号化標識だけを復号器で扱い、本文内部の同字は保持する。
        # 復号方式と元転送印は取得本文へ記録する。旧保存本文は変換しない。
        encoding = "utf-8-sig"
    try:
        text = raw.decode(encoding, errors="strict")
    except (LookupError, UnicodeError) as exc:
        raise 本文取得失敗("本文復号失敗") from exc
    title, excluded = "", ()
    if mime == "text/html":
        parser = _本文抽出()
        parser.feed(text)
        parser.close()
        text = parser.本文()
        title = " ".join("".join(parser.題名).split())
        if parser.抑止:
            parser.除外.add("未閉鎖除外域")
        excluded = tuple(sorted(parser.除外))
    else:
        text = text.replace("\r\n", "\n").replace("\r", "\n").strip()
    if not text.strip():
        raise 本文取得失敗("取得本文が空")
    return 取得本文(要求URL, 経路[-1], title, text, datetime.now(timezone.utc).isoformat(),
                    mime, encoding, len(raw), sha256(raw).hexdigest(),
                    sha256(text.encode("utf-8")).hexdigest(), 経路, excluded)


class 公開本文取得器:
    """公開HTTPS専用・読取専用。DNS待ちの強制中断やブラウザ描画は提供しない。"""

    _受入形式 = "text/html, text/plain, text/markdown"
    _利用者識別 = "MINIDORA/knowledge-retrieval-0.1"

    def __init__(self, *, timeout: float = 8.0, 最大バイト数: int = 500_000,
                 最大転送数: int = 3, 許可ホスト: tuple[str, ...] = (), 取得制御=None):
        if type(timeout) not in (int, float) or not math.isfinite(timeout) or not 0 < timeout <= 60:
            raise ValueError("timeout範囲不正")
        if type(最大バイト数) is not int or not 1 <= 最大バイト数 <= 2_000_000:
            raise ValueError("最大バイト数範囲不正")
        if type(最大転送数) is not int or not 0 <= 最大転送数 <= 5:
            raise ValueError("最大転送数範囲不正")
        if type(許可ホスト) is not tuple:
            raise ValueError("許可ホストはtuple")
        hosts = []
        for host in 許可ホスト:
            if type(host) is not str or not host or any(c in host for c in "/?#@:"):
                raise ValueError("許可ホスト不正")
            hosts.append(urlsplit(公開URL("https://" + host)).hostname)
        self.timeout, self.最大バイト数, self.最大転送数 = float(timeout), 最大バイト数, 最大転送数
        self.許可ホスト = tuple(hosts)
        if 取得制御 is not None and not isinstance(取得制御, 共有取得制御):
            raise TypeError("取得制御の型不正")
        self.取得制御 = 取得制御

    def 制御を束縛(self, 制御):
        if not isinstance(制御, 共有取得制御):
            raise TypeError("取得制御の型不正")
        obj = copy(self)
        obj.取得制御 = 制御
        return obj

    def _猶予(self, 期限):
        if 期限 is None:
            return self.timeout
        残り = 期限 - time.monotonic()
        if 残り <= 0:
            raise 本文取得失敗("HTTP取得時間上限")
        return self.取得制御.残時間(残り) if self.取得制御 is not None else 残り

    def _要求を構成(self, url):
        return "GET", {"User-Agent": self._利用者識別, "Accept": self._受入形式,
                       "Accept-Encoding": "identity", "Connection": "close"}, None

    def _一回取得(self, url: str) -> tuple[int, dict[str, str], bytes]:
        deadline = time.monotonic() + self.timeout if self.取得制御 is not None else None
        if self.取得制御 is not None:
            self.取得制御.通信を開始()
        host = urlsplit(url).hostname
        ip = _公開アドレス(host)
        文脈 = ssl.create_default_context()
        conn = HTTPSConnection(host, 443, timeout=self._猶予(deadline), context=文脈)
        raw_socket = socket.create_connection((ip, 443), timeout=self._猶予(deadline))
        try:
            raw_socket.settimeout(self._猶予(deadline))
            tls_socket = 文脈.wrap_socket(raw_socket, server_hostname=host)
            conn.sock = tls_socket
            u = urlsplit(url)
            method, headers, body = self._要求を構成(url)
            tls_socket.settimeout(self._猶予(deadline))
            conn.request(method, u.path + ("?" + u.query if u.query else ""), body=body, headers=headers)
            tls_socket.settimeout(self._猶予(deadline))
            with conn.getresponse() as response:
                self._猶予(deadline)
                headers = {k.lower(): v for k, v in response.getheaders()}
                if response.status != 200:
                    return response.status, headers, b""
                length = headers.get("content-length")
                if length is not None and (not length.isdecimal() or int(length) > self.最大バイト数):
                    raise 本文取得失敗("本文サイズ上限")
                if (length is not None and self.取得制御 is not None
                        and int(length) > self.取得制御.残量("総バイト数")):
                    raise 取得資源不足("総バイト数上限")
                chunks, size = [], 0
                if deadline is None:
                    # 制御未指定の従来入口は、既存の読取段階の期限を保つ。
                    deadline = time.monotonic() + self.timeout
                while not response.isclosed():
                    tls_socket.settimeout(self._猶予(deadline))
                    count = min(65536, self.最大バイト数 + 1 - size)
                    if self.取得制御 is not None:
                        count = min(count, self.取得制御.残量("総バイト数"))
                        if count <= 0:
                            raise 取得資源不足("総バイト数上限")
                    chunk = response.read1(count)
                    if chunk and self.取得制御 is not None:
                        self.取得制御.受信を記録(len(chunk))
                    self._猶予(deadline)
                    if not chunk:
                        break
                    chunks.append(chunk)
                    size += len(chunk)
                    if size > self.最大バイト数:
                        raise 本文取得失敗("本文サイズ上限")
                資料 = b"".join(chunks)
                if length is not None and len(資料) != int(length):
                    raise 本文取得失敗("本文受信長不一致")
                return response.status, headers, 資料
        finally:
            conn.close()
            raw_socket.close()

    def _転送を取得(self, url: str, *, 同一ホスト限定: bool = False,
                  許可経路: tuple[str, ...] = ()) -> tuple[str, tuple[str, ...], dict[str, str], bytes]:
        first = current = 公開URL(url)
        初期ホスト = urlsplit(first).hostname
        経路 = []
        for _ in range(self.最大転送数 + 1):
            current = 公開URL(current)
            host = urlsplit(current).hostname
            if self.許可ホスト and host not in self.許可ホスト:
                raise 本文取得失敗("許可ホスト外")
            if 同一ホスト限定 and host != 初期ホスト:
                raise 本文取得失敗("別ホストへの転送禁止")
            if 許可経路 and urlsplit(current).path not in 許可経路:
                raise 本文取得失敗("許可経路外")
            if current in 経路:
                raise 本文取得失敗("転送循環")
            経路.append(current)
            status, headers, 資料 = self._一回取得(current)
            if status in (301, 302, 303, 307, 308):
                target = headers.get("location")
                if not target:
                    raise 本文取得失敗("転送先なし")
                current = urljoin(current, target)
                continue
            if status != 200:
                self._状態失敗(status, headers)
            if len(資料) > self.最大バイト数:
                raise 本文取得失敗("本文サイズ上限")
            return first, tuple(経路), headers, 資料
        raise 本文取得失敗("転送回数上限")

    def _状態失敗(self, status: int, headers: dict[str, str]) -> None:
        raise 本文取得失敗(f"HTTP状態:{status}")

    def 取得(self, url: str) -> 取得本文:
        first, 経路, headers, 資料 = self._転送を取得(url)
        return 本文を復号(first, 経路, headers, 資料)


class 公開PDF本文取得器(公開本文取得器):
    """公開本文の安全取得を共有し、固定依存でPDF文字と頁対応を追加する。"""

    _受入形式 = 公開本文取得器._受入形式 + ", application/pdf"

    def 取得(self, url: str) -> 取得本文:
        first, 経路, headers, raw = self._転送を取得(url)
        msg = Message()
        msg["content-type"] = headers.get("content-type", "application/octet-stream")
        if msg.get_content_type() != "application/pdf":
            return 本文を復号(first, 経路, headers, raw)
        if headers.get("content-encoding", "identity").lower() not in ("", "identity"):
            raise 本文取得失敗("圧縮応答は未対応")
        from .公開PDF抽出 import PDF文字を抽出, PDF抽出版
        秒 = min(8.0, self.timeout)
        if self.取得制御 is not None:
            秒 = self.取得制御.残時間(秒)
        try:
            text, pages = PDF文字を抽出(raw, 秒上限=秒)
        except Exception as exc:
            raise 本文取得失敗("PDF文字抽出不成立") from exc
        if self.取得制御 is not None:
            self.取得制御.確認()
        return 取得本文(first, 経路[-1], "", text, datetime.now(timezone.utc).isoformat(),
                        "application/pdf", PDF抽出版, len(raw), sha256(raw).hexdigest(),
                        sha256(text.encode("utf-8")).hexdigest(), 経路,
                        ("PDF文字以外は未解釈",), pages)


@dataclass(frozen=True, slots=True)
class 取得JSON:
    要求URL: str
    最終URL: str
    値: dict
    取得時刻: str
    内容種別: str
    転送バイト数: int
    転送SHA256: str
    経路: tuple[str, ...]


class JSON待機要求(本文取得失敗):
    def __init__(self, 待機秒: float):
        super().__init__("JSON取得の再試行待機中")
        self.待機秒 = 待機秒


class 公開JSON取得器(公開本文取得器):
    """許可した公開ホスト・経路のJSONだけ読む。記事本文の取得成功とは別契約。"""

    _受入形式 = "application/json"
    _利用者識別 = "MINIDORA/knowledge-retrieval-0.1 (+https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA)"

    def __init__(self, *, 許可ホスト: tuple[str, ...], 許可経路: tuple[str, ...],
                 timeout: float = 8.0, 最大バイト数: int = 2_000_000, 最大転送数: int = 3, 取得制御=None):
        super().__init__(timeout=timeout, 最大バイト数=最大バイト数,
                         最大転送数=最大転送数, 許可ホスト=許可ホスト, 取得制御=取得制御)
        if not self.許可ホスト or type(許可経路) is not tuple or not 許可経路:
            raise ValueError("JSONには許可ホストと経路が必要")
        if any(type(path) is not str or not path.startswith("/")
               or urlsplit(path).path != path or re.search(r"[\s\\]", path) for path in 許可経路):
            raise ValueError("JSON許可経路不正")
        self.許可経路 = 許可経路
        # 束縛した当回コピーも同じ供給器のRetry-Afterを守る。
        self._待機期限 = [0.0]
        self._待機ロック = Lock()

    def _状態失敗(self, status: int, headers: dict[str, str]) -> None:
        if status == 429 or status == 503 and "retry-after" in headers:
            指示 = headers.get("retry-after", "")
            待機秒 = 60.0
            if 指示.isdecimal():
                # 桁数を限定して変換する。過大な待機指示は自動再試行を停止する。
                待機秒 = max(1.0, float(指示)) if len(指示) <= 12 else math.inf
            elif 指示:
                try:
                    日時 = parsedate_to_datetime(指示)
                    if 日時.tzinfo is not None:
                        待機秒 = max(1.0, (日時 - datetime.now(timezone.utc)).total_seconds())
                except (ValueError, OverflowError):
                    pass
            with self._待機ロック:
                self._待機期限[0] = max(self._待機期限[0], time.monotonic() + 待機秒)
            raise JSON待機要求(待機秒)
        super()._状態失敗(status, headers)

    def 待機を確認(self):
        with self._待機ロック:
            待機 = self._待機期限[0] - time.monotonic()
        if 待機 > 0:
            raise JSON待機要求(待機)

    def 取得(self, url: str) -> 取得JSON:
        self.待機を確認()
        first, 経路, headers, raw = self._転送を取得(
            url, 同一ホスト限定=True, 許可経路=self.許可経路)
        msg = Message()
        msg["content-type"] = headers.get("content-type", "application/octet-stream")
        if msg.get_content_type() != "application/json":
            raise 本文取得失敗("JSON内容種別不正")
        if headers.get("content-encoding", "identity").lower() not in ("", "identity"):
            raise 本文取得失敗("圧縮応答は未対応")
        if (msg.get_content_charset() or "utf-8").lower().replace("_", "-") != "utf-8":
            raise 本文取得失敗("JSON文字コード不正")

        def 重複を拒否(rows):
            out = {}
            for key, value in rows:
                if key in out:
                    raise ValueError("JSON重複キー")
                out[key] = value
            return out

        def 非有限を拒否(value):
            raise ValueError("JSON非有限数")

        def 実数を検査(value):
            number = float(value)
            if not math.isfinite(number):
                raise ValueError("JSON非有限数")
            return number

        try:
            value = json.loads(raw.decode("utf-8", errors="strict"),
                               object_pairs_hook=重複を拒否, parse_constant=非有限を拒否,
                               parse_float=実数を検査)
            if type(value) is not dict:
                raise ValueError("JSON最上位objectが必要")
        except (ValueError, UnicodeError, RecursionError) as exc:
            raise 本文取得失敗("JSON復号失敗") from exc
        return 取得JSON(first, 経路[-1], value, datetime.now(timezone.utc).isoformat(),
                        "application/json", len(raw), sha256(raw).hexdigest(), 経路)
