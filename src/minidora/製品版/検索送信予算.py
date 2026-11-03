"""固定GCSオブジェクトで累積検索送信枠を予約する。総請求額の上限ではない。"""
from __future__ import annotations

from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from hashlib import sha256
import json
import re
from threading import RLock
from urllib.parse import quote

from ..公開本文取得 import 公開本文取得器
from ..取得実行制御 import 共有取得制御, 取得資源不足


class 検索送信予算停止(ValueError):
    """秘密ヘッダー・検索本文・保存応答の自由文を含まない停止理由。"""


def _JSONを読む(raw):
    def 重複を拒否(pairs):
        out = {}
        for key, value in pairs:
            if key in out:
                raise 検索送信予算停止("予算JSONキー重複")
            out[key] = value
        return out
    return json.loads(raw, object_pairs_hook=重複を拒否)


def _JSONにする(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _世代を確認(value):
    if type(value) is not str or not re.fullmatch(r"[1-9][0-9]{0,19}", value):
        raise 検索送信予算停止("保存世代不正")
    return value


@dataclass(frozen=True)
class 固定保存先:
    バケット: str
    オブジェクト: str

    def __post_init__(self):
        if (type(self.バケット) is not str
                or not re.fullmatch(r"[a-z0-9][a-z0-9-]{1,61}[a-z0-9]", self.バケット)):
            raise 検索送信予算停止("予算バケット不正")
        if (type(self.オブジェクト) is not str or len(self.オブジェクト) > 128
                or not re.fullmatch(r"[A-Za-z0-9_-]+(?:/[A-Za-z0-9_.-]+)*", self.オブジェクト)
                or any(p in {".", ".."} for p in self.オブジェクト.split("/"))):
            raise 検索送信予算停止("予算オブジェクト不正")

    @property
    def _基底(self):
        return ("https://storage.googleapis.com/storage/v1/b/" + self.バケット
                + "/o/" + quote(self.オブジェクト, safe=""))

    def メタデータURL(self):
        return self._基底 + "?fields=bucket%2Cname%2Cgeneration"

    def 本文URL(self, 世代):
        return self._基底 + "?alt=media&ifGenerationMatch=" + _世代を確認(世代)

    def 更新URL(self, 世代):
        return ("https://storage.googleapis.com/upload/storage/v1/b/" + self.バケット
                + "/o?uploadType=media&name=" + quote(self.オブジェクト, safe="")
                + "&ifGenerationMatch=" + _世代を確認(世代)
                + "&fields=bucket%2Cname%2Cgeneration")

    def 要求を確認(self, url, method):
        # 再構成した完全URLのみ。別object・履歴世代・初期作成・任意queryは拒否。
        if type(url) is not str:
            raise 検索送信予算停止("予算保存通信先不正")
        if method == "GET" and url == self.メタデータURL():
            return
        match = re.search(r"[?&]ifGenerationMatch=([1-9][0-9]{0,19})(?:&|$)", url)
        if match:
            generation = match.group(1)
            if ((method == "GET" and url == self.本文URL(generation))
                    or (method == "POST" and url == self.更新URL(generation))):
                return
        raise 検索送信予算停止("予算保存用途不一致")


@dataclass(frozen=True)
class 検索送信予算設定:
    予算ID: str
    プロジェクトID: str
    アプリID: str
    保存先: 固定保存先
    開始UTC: datetime
    終了UTC: datetime
    累積上限: int = 1000

    def __post_init__(self):
        if type(self.予算ID) is not str or not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", self.予算ID):
            raise 検索送信予算停止("予算ID不正")
        if (type(self.プロジェクトID) is not str
                or not re.fullmatch(r"[a-z][a-z0-9-]{4,28}[a-z0-9]", self.プロジェクトID)
                or type(self.アプリID) is not str
                or not re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,62}", self.アプリID)
                or type(self.保存先) is not 固定保存先):
            raise 検索送信予算停止("予算対象不正")
        for value in (self.開始UTC, self.終了UTC):
            if (type(value) is not datetime or value.tzinfo != timezone.utc or value.microsecond):
                raise 検索送信予算停止("予算期間は秒単位のUTC")
        if self.開始UTC >= self.終了UTC:
            raise 検索送信予算停止("予算期間不正")
        if type(self.累積上限) is not int or not 1 <= self.累積上限 <= 1000:
            raise 検索送信予算停止("累積検索上限不正")

    @property
    def 検索URL(self):
        return ("https://discoveryengine.googleapis.com/v1/projects/" + self.プロジェクトID
                + "/locations/global/collections/default_collection/engines/" + self.アプリID
                + "/servingConfigs/default_search:search")

    @property
    def 設定SHA256(self):
        設定値 = asdict(self)
        for name in ("開始UTC", "終了UTC"):
            設定値[name] = 設定値[name].isoformat()
        return sha256(_JSONにする(設定値)).hexdigest()

    def 期間を確認(self):
        if not self.開始UTC <= datetime.now(timezone.utc) < self.終了UTC:
            raise 検索送信予算停止("検索送信期間外")

    def 保存内容(self, 予約数):
        if type(予約数) is not int or not 0 <= 予約数 <= self.累積上限:
            raise 検索送信予算停止("累積検索予約数不正")
        return _JSONにする({"版": "minidora-google-search-budget-v1",
                         "設定SHA256": self.設定SHA256, "予約数": 予約数})


class _検索送信許可:
    """CAS成功の当回だけ。一回消費をコピー間でも共有し、保存・復活しない。"""

    def __init__(self, 設定, url, body, control, count):
        self._設定, self._URL, self._SHA = 設定, url, sha256(body).digest()
        self._制御, self._予約数 = control, count
        self._ロック, self._消費済み = RLock(), False

    def __copy__(self):
        return self

    def __deepcopy__(self, memo):
        return self

    def __reduce__(self):
        raise TypeError("検索送信許可は保存不可")

    def 消費(self, url, body, control):
        with self._ロック:
            if (self._消費済み or url != self._URL or type(body) is not bytes
                    or sha256(body).digest() != self._SHA or control is not self._制御):
                raise 検索送信予算停止("検索送信許可不一致又は消費済み")
            control.確認()
            self._設定.期間を確認()
            self._消費済み = True


class _保存取得器(公開本文取得器):
    def __init__(self, 設定, url, method, headers, body, control, timeout):
        super().__init__(timeout=timeout, 最大バイト数=16384, 最大転送数=0,
                         許可ホスト=("storage.googleapis.com",), 取得制御=control)
        self._設定, self._URL, self._method = 設定, url, method
        self._headers, self._body = headers, body

    def _要求を構成(self, url):
        if url != self._URL:
            raise 検索送信予算停止("予算保存通信先不一致")
        self._設定.保存先.要求を確認(url, self._method)
        self.取得制御.確認()
        self._設定.期間を確認()
        _, headers, _ = super()._要求を構成(url)
        return self._method, {**headers, "Cache-Control": "no-cache",
            "Content-Type": "application/json; charset=utf-8", **self._headers}, self._body


class GCS検索送信予算:
    """欠損時の初期化、返却、未知更新の再送、日付や配備によるresetは行わない。"""

    def __init__(self, 設定, 保存認証器, *, timeout=8.0):
        if type(設定) is not 検索送信予算設定:
            raise TypeError("検索送信予算設定が必要")
        if not callable(getattr(保存認証器, "認証する", None)):
            raise TypeError("固定保存先の明示認証器が必要")
        # 通信前に既存取得器と同じ時間設定を検査。
        公開本文取得器(timeout=timeout)
        self.設定, self._認証器, self.timeout = 設定, 保存認証器, timeout

    def _取得(self, url, method, body, control):
        self.設定.保存先.要求を確認(url, method)
        control.確認()
        self.設定.期間を確認()
        headers = self._認証器.認証する(url, method=method, 取得制御=control)
        names = {"authorization", "x-goog-user-project", "x-goog-api-client", "x-allowed-locations"}
        if (type(headers) is not dict or not set(headers) <= names
                or any(type(v) is not str or not v or len(v) > 16384
                       or re.search(r"[^\x20-\x7e]", v) for v in headers.values())
                or not re.fullmatch(r"Bearer [A-Za-z0-9._~+/=-]+", headers.get("authorization", ""))):
            raise 検索送信予算停止("予算保存認証ヘッダー不正")
        transport = _保存取得器(self.設定, url, method, dict(headers), body,
                              control, self.timeout)
        try:
            status, _, raw = transport._一回取得(url)
            return status, raw
        finally:
            transport._headers, transport._body = {}, None

    def _メタデータ(self, raw):
        value = _JSONを読む(raw)
        target = self.設定.保存先
        if (type(value) is not dict or set(value) != {"bucket", "name", "generation"}
                or value["bucket"] != target.バケット or value["name"] != target.オブジェクト):
            raise 検索送信予算停止("予算保存対象不一致")
        return _世代を確認(value["generation"])

    def 予約する(self, url, body, control):
        if (url != self.設定.検索URL or type(body) is not bytes or not body or len(body) > 8192
                or not isinstance(control, 共有取得制御)):
            raise 検索送信予算停止("検索送信予約の当回対象不正")
        try:
            self.設定.期間を確認()
            control.確認()
            target = self.設定.保存先
            for _ in range(2):
                status, raw = self._取得(target.メタデータURL(), "GET", None, control)
                if status != 200:
                    raise 検索送信予算停止("予算保存の読取停止")
                generation = self._メタデータ(raw)
                status, raw = self._取得(target.本文URL(generation), "GET", None, control)
                if status == 412:
                    continue
                if status != 200:
                    raise 検索送信予算停止("予算保存の本文読取停止")
                value = _JSONを読む(raw)
                if (type(value) is not dict or set(value) != {"版", "設定SHA256", "予約数"}
                        or value["版"] != "minidora-google-search-budget-v1"
                        or value["設定SHA256"] != self.設定.設定SHA256):
                    raise 検索送信予算停止("検索送信予算の版又は設定不一致")
                count = value["予約数"]
                self.設定.保存内容(count)  # bool・負数・上限超過も拒否。
                if count >= self.設定.累積上限:
                    raise 検索送信予算停止("累積検索送信上限")
                status, raw = self._取得(target.更新URL(generation), "POST",
                                        self.設定.保存内容(count + 1), control)
                if status == 412:
                    continue
                if status != 200 or self._メタデータ(raw) == generation:
                    raise 検索送信予算停止("検索送信予約の更新結果未確認")
                control.確認()
                self.設定.期間を確認()
                return _検索送信許可(self.設定, url, body, control, count + 1)
            raise 検索送信予算停止("検索送信予約の競合上限")
        except (検索送信予算停止, 取得資源不足):
            raise
        except Exception:
            raise 検索送信予算停止("検索送信予約の結果未確認") from None
