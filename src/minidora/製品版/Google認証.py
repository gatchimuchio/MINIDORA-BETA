"""固定版ADCを当回の取得制御へ接続する。秘密値を監査へ返さない。"""
from __future__ import annotations

from dataclasses import dataclass, field
from http.client import HTTPConnection
import json
import math
from pathlib import Path
import re
import socket
from threading import RLock
import time
from urllib.parse import parse_qs, urlsplit

import google.auth
from google.auth import compute_engine
from google.auth.exceptions import TransportError
from google.oauth2.credentials import Credentials as 利用者Credentials

from ..公開本文取得 import 公開本文取得器
from ..取得実行制御 import 共有取得制御, 取得資源不足
from .検索送信予算 import 固定保存先


_範囲 = "https://www.googleapis.com/auth/cloud-platform"
_OAuth = "https://oauth2.googleapis.com/token"
_版 = "2.61.0"


class Google認証失敗(ValueError):
    """通信応答・鍵・自由なSDK例外文を含まない。"""


@dataclass(frozen=True, repr=False)
class _認証応答:
    status: int
    headers: dict[str, str] = field(repr=False)
    data: bytes = field(repr=False)


class _認証HTTPS(公開本文取得器):
    def __init__(self, url, method, headers, body, control, timeout):
        super().__init__(timeout=timeout, 最大バイト数=65536, 最大転送数=0,
                         許可ホスト=(urlsplit(url).hostname,), 取得制御=control)
        self._URL, self._method, self._headers, self._body = url, method, headers, body

    def _要求を構成(self, url):
        if url != self._URL:
            raise Google認証失敗("認証通信先不一致")
        self.取得制御.確認()
        _, headers, _ = super()._要求を構成(url)
        return self._method, {**headers, **self._headers}, self._body


class _当回認証Request:
    """SDKのRequest口。転送・proxy・別セッション・背景処理は設けない。"""

    def __init__(self, control, サービスアカウント=None):
        if not isinstance(control, 共有取得制御):
            raise TypeError("Google認証に当回取得制御が必要")
        self._制御, self._SA = control, サービスアカウント
        self._失敗 = None

    def 失敗を通知(self):
        if self._失敗 is not None:
            raise self._失敗 from None
        self._制御.確認()

    def __deepcopy__(self, memo):
        # 固定版SDKはblockingを指定する。背景用のコピーは許可しない。
        raise Google認証失敗("背景認証未対応")

    def __call__(self, url, method="GET", body=None, headers=None, timeout=None, **kwargs):
        try:
            self.失敗を通知()
            if kwargs or type(url) is not str or method not in {"GET", "POST"}:
                raise Google認証失敗("認証要求形不正")
            if timeout is None:
                timeout = 8.0
            if type(timeout) not in (int, float) or not math.isfinite(timeout) or timeout <= 0:
                raise Google認証失敗("認証時間設定不正")
            timeout = min(float(timeout), 8.0)
            headers = self._ヘッダー(headers)
            if body is not None and (type(body) is not bytes or len(body) > 65536):
                raise Google認証失敗("認証要求本文不正")
            if url.startswith("http://"):
                self._メタデータ先を確認(url, method, headers, body)
                return self._メタデータ取得(url, headers, timeout)
            if url == _OAuth and self._SA is None:
                if (method != "POST" or body is None or "authorization" in headers
                        or headers.get("content-type") != "application/x-www-form-urlencoded"):
                    raise Google認証失敗("OAuth要求不正")
            elif self._SA is not None and url == (
                    "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/"
                    + self._SA + "/allowedLocations"):
                if method != "GET" or body is not None or "authorization" not in headers:
                    raise Google認証失敗("境界照会要求不正")
            else:
                raise Google認証失敗("認証通信先未対応")
            if "metadata-flavor" in headers:
                raise Google認証失敗("公開認証ヘッダー不正")
            transport = _認証HTTPS(url, method, headers, body, self._制御, timeout)
            try:
                status, response_headers, 応答値 = transport._一回取得(url)
                # 非200本文は既存transportが読まない。秘密エラー本文もSDKへ渡さない。
                if status == 200:
                    応答値 = self._応答を限定(応答値, OAuth=url == _OAuth)
                return _認証応答(status, response_headers, 応答値)
            finally:
                transport._headers, transport._body = {}, None
        except 取得資源不足 as exc:
            self._失敗 = 取得資源不足(exc.理由)
            raise self._失敗 from None
        except Exception:
            self._失敗 = Google認証失敗("認証通信失敗")
            # SDKが捕捉する場合も、認証器は後で当回の失敗を再通知する。
            raise TransportError("MINIDORA認証通信失敗") from None

    @staticmethod
    def _応答を限定(raw, *, OAuth):
        応答値 = json.loads(raw)
        if type(応答値) is not dict:
            raise Google認証失敗("認証応答形不正")
        if not OAuth:
            value = 応答値.get("encodedLocations")
            if (type(value) is not str or not value or len(value) > 16384
                    or re.search(r"[^\x20-\x7e]", value)):
                raise Google認証失敗("境界応答不正")
            # SDKは不正な200応答をloggerへ出すため、先に検査し必要fieldへ限定する。
            out = {"encodedLocations": value}
        else:
            トークン, expires = 応答値.get("access_token"), 応答値.get("expires_in")
            if (type(トークン) is not str or len(トークン) > 16384
                    or not re.fullmatch(r"[A-Za-z0-9._~+/=-]+", トークン)
                    or type(expires) not in (int, float) or not math.isfinite(expires)
                    or not 1 <= expires <= 86400 or 応答値.get("token_type") != "Bearer"):
                raise Google認証失敗("OAuth応答不正")
            out = {"access_token": トークン, "expires_in": expires, "token_type": "Bearer"}
            for name in ("refresh_token", "scope"):
                value = 応答値.get(name)
                if value is not None:
                    if (type(value) is not str or not value or len(value) > 16384
                            or re.search(r"[^\x20-\x7e]", value)):
                        raise Google認証失敗("OAuth追加値不正")
                    out[name] = value
        return json.dumps(out, ensure_ascii=True, allow_nan=False).encode("ascii")

    @staticmethod
    def _ヘッダー(values):
        if values is None:
            return {}
        if type(values) is not dict:
            raise Google認証失敗("認証ヘッダー形不正")
        out = {}
        for name, value in values.items():
            if (type(name) is not str or type(value) is not str or not value
                    or len(value) > 16384 or re.search(r"[^\x20-\x7e]", value)):
                raise Google認証失敗("認証ヘッダー値不正")
            name = name.lower()
            if name in out or name not in {"authorization", "content-type", "metadata-flavor",
                                           "x-goog-api-client", "x-goog-user-project"}:
                raise Google認証失敗("認証ヘッダー名不正")
            if name == "authorization" and not re.fullmatch(r"Bearer [A-Za-z0-9._~+/=-]+", value):
                raise Google認証失敗("認証Bearer不正")
            out[name] = value
        return out

    def _メタデータ先を確認(self, url, method, headers, body):
        u = urlsplit(url)
        if (self._SA is None or method != "GET" or body is not None
                or u.netloc != "metadata.google.internal" or u.fragment
                or headers.get("metadata-flavor") != "Google"
                or set(headers) - {"metadata-flavor", "x-goog-api-client"}):
            raise Google認証失敗("メタデータ要求不正")
        root = "/computeMetadata/v1/instance/service-accounts/"
        query = parse_qs(u.query, keep_blank_values=True, strict_parsing=True)
        if u.path == root + self._SA + "/":
            allowed = query == {"recursive": ["true"]}
        elif u.path == root + "default/token":
            allowed = query == {"scopes": [_範囲]}
        else:
            allowed = False
        if not allowed:
            raise Google認証失敗("メタデータ経路未対応")

    def _メタデータ取得(self, url, headers, timeout):
        control = self._制御
        control.通信を開始()
        deadline = time.monotonic() + timeout
        def 猶予():
            return control.残時間(max(0, deadline - time.monotonic()))
        # metadataだけの私有HTTP境界。DNS・公開URLの許可条件は変更しない。
        conn = HTTPConnection("metadata.google.internal", 80, timeout=猶予())
        raw_socket = socket.create_connection(("169.254.169.254", 80), timeout=猶予())
        conn.sock = raw_socket
        try:
            u = urlsplit(url)
            raw_socket.settimeout(猶予())
            conn.request("GET", u.path + "?" + u.query,
                         headers={**headers, "Accept-Encoding": "identity", "Connection": "close"})
            raw_socket.settimeout(猶予())
            with conn.getresponse() as response:
                猶予()
                response_headers = {k.lower(): v for k, v in response.getheaders()}
                if response.status != 200:
                    return _認証応答(response.status, response_headers, b"")
                if response_headers.get("metadata-flavor") != "Google":
                    raise Google認証失敗("メタデータ応答不正")
                length = response_headers.get("content-length")
                if length is not None:
                    if not length.isdecimal() or int(length) > 65536:
                        raise Google認証失敗("認証応答容量上限")
                    if int(length) > control.残量("総バイト数"):
                        raise 取得資源不足("総バイト数上限")
                chunks, size = [], 0
                while not response.isclosed():
                    raw_socket.settimeout(猶予())
                    count = min(65536 + 1 - size, control.残量("総バイト数"))
                    if count <= 0:
                        raise 取得資源不足("総バイト数上限")
                    chunk = response.read1(count)
                    if chunk:
                        control.受信を記録(len(chunk))
                    猶予()
                    if not chunk:
                        break
                    chunks.append(chunk)
                    size += len(chunk)
                    if size > 65536:
                        raise Google認証失敗("認証応答容量上限")
                応答値 = b"".join(chunks)
                if length is not None and len(応答値) != int(length):
                    raise Google認証失敗("認証応答長不一致")
                if u.path.endswith("/" + self._SA + "/"):
                    info = json.loads(応答値)
                    if type(info) is not dict or info.get("email") != self._SA:
                        raise Google認証失敗("実行アカウント不一致")
                    応答値 = json.dumps({"email": self._SA}).encode("ascii")
                else:
                    応答値 = self._応答を限定(応答値, OAuth=True)
                return _認証応答(response.status, response_headers, 応答値)
        finally:
            conn.close()
            raw_socket.close()


class GoogleADC認証器:
    """Cloud Runの明示SA又は明示authorized_userファイルのみ。自動探索しない。"""

    def __init__(self, credentials, プロジェクトID, サービスアカウント=None):
        if google.auth.__version__ != _版:
            raise Google認証失敗("Google認証SDK版不一致")
        if not re.fullmatch(r"[a-z][a-z0-9-]{4,28}[a-z0-9]", プロジェクトID):
            raise Google認証失敗("認証プロジェクト不正")
        if type(credentials) not in {compute_engine.Credentials, 利用者Credentials}:
            raise Google認証失敗("ADC型未対応")
        self._credentials, self._プロジェクト, self._SA = credentials, プロジェクトID, サービスアカウント
        self._ロック = RLock()
        # 固定版SDKの内部API依存。RABを無効化せず当回内で実行させる。
        credentials._set_blocking_regional_access_boundary_lookup()

    @classmethod
    def CloudRun(cls, プロジェクトID, サービスアカウント):
        if (type(サービスアカウント) is not str or not re.fullmatch(
                r"[a-z][a-z0-9-]{4,28}[a-z0-9]@" + re.escape(プロジェクトID)
                + r"\.iam\.gserviceaccount\.com", サービスアカウント)):
            raise Google認証失敗("実行アカウント設定不正")
        credentials = compute_engine.Credentials(service_account_email=サービスアカウント,
            scopes=(_範囲,), universe_domain="googleapis.com", quota_project_id=プロジェクトID)
        return cls(credentials, プロジェクトID, サービスアカウント)

    @classmethod
    def ローカル利用者ファイル(cls, path, プロジェクトID):
        try:
            with Path(path).open("rb") as stream:
                raw = stream.read(65537)
            if len(raw) > 65536:
                raise Google認証失敗("ADCファイル容量上限")
            info = json.loads(raw)
            if (type(info) is not dict or info.get("type") != "authorized_user"
                    or info.get("universe_domain", "googleapis.com") != "googleapis.com"
                    or info.get("quota_project_id", プロジェクトID) != プロジェクトID):
                raise Google認証失敗("ADCファイル型未対応")
            clean = {"quota_project_id": プロジェクトID, "universe_domain": "googleapis.com"}
            for name in ("client_id", "client_secret", "refresh_token"):
                value = info.get(name)
                if type(value) is not str or not value or len(value) > 16384:
                    raise Google認証失敗("ADC必須値不正")
                clean[name] = value
            # 保存access token・trust_boundary・任意token_uriは採用しない。
            credentials = 利用者Credentials.from_authorized_user_info(clean, scopes=(_範囲,))
            return cls(credentials, プロジェクトID)
        except Exception:
            raise Google認証失敗("ADCファイル読取失敗") from None

    def 認証する(self, url, *, 取得制御):
        prefix = "https://discoveryengine.googleapis.com/v1/projects/" + self._プロジェクト + "/"
        if type(url) is not str or not url.startswith(prefix) or not url.endswith("/servingConfigs/default_search:search"):
            raise Google認証失敗("認証用途不一致")
        return self._認証を実行(url, "POST", 取得制御)

    def 固定保存先(self, バケット, オブジェクト):
        return _固定保存認証器(self, 固定保存先(バケット, オブジェクト))

    def _認証を実行(self, url, method, 取得制御):
        request = _当回認証Request(取得制御, self._SA)
        with self._ロック:
            request.失敗を通知()
            headers = {}
            try:
                self._credentials.before_request(request, method, url, headers)
            except Exception:
                request.失敗を通知()
                raise Google認証失敗("ADC更新失敗") from None
            request.失敗を通知()
            return {k.lower(): v for k, v in headers.items()}


class _固定保存認証器:
    """管理者が指定した一つのGCSオブジェクトのGET/CAS POSTだけ。"""

    def __init__(self, 認証器, 保存先):
        self._認証器, self._保存先 = 認証器, 保存先

    def 認証する(self, url, *, method, 取得制御):
        self._保存先.要求を確認(url, method)
        return self._認証器._認証を実行(url, method, 取得制御)
