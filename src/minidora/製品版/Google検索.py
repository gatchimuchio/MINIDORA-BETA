"""Google Cloud通常Searchの候補だけ読む。認証・公開範囲は管理者が設定する。"""
from __future__ import annotations

from copy import copy
from dataclasses import asdict
from hashlib import sha256
import json
import re
from urllib.parse import urlsplit

from ..公開本文取得 import 公開JSON取得器, 公開本文取得器, 公開URL, JSON待機要求
from ..取得実行制御 import 共有取得制御, 取得資源不足
from ..公開送信条件 import 公開語を検査, 公開送信停止
from .型 import 参照資料
from .検索送信予算 import GCS検索送信予算, _検索送信許可


class _検索POST取得器(公開JSON取得器):
    def __init__(self, url, 認証器, 送信予算, *, timeout, 公開条件必須=False):
        super().__init__(許可ホスト=("discoveryengine.googleapis.com",),
                         許可経路=(urlsplit(url).path,), 最大転送数=0, timeout=timeout)
        self._URL = url
        self._認証器 = 認証器
        self._送信予算 = 送信予算
        self._公開条件必須 = 公開条件必須

    def 検索JSONを取得(self, query, limit):
        条件 = self.取得制御.公開条件 if self.取得制御 is not None else None
        if self._公開条件必須 and 条件 is None:
            raise 公開送信停止(("文脈未束縛",))
        公開語を検査(query, 条件=条件)
        # 当回のJSON本文だけをコピーへ束縛し、次の質問へ持ち越さない。
        当回 = copy(self)
        当回._要求本文 = json.dumps({"query": query, "pageSize": limit},
                                    ensure_ascii=False, allow_nan=False).encode("utf-8")
        try:
            if 当回.取得制御 is not None:
                当回.取得制御.確認()
            当回.待機を確認()
            # ADC更新を検索TLS接続前に済ませ、当回だけへ束縛する。
            当回._認証ヘッダー = 当回._認証を準備()
            当回._送信許可 = 当回._送信予算.予約する(
                当回._URL, 当回._要求本文, 当回.取得制御)
            return 当回.取得(当回._URL)
        finally:
            当回._要求本文 = b""
            当回._認証ヘッダー = {}
            当回._送信許可 = None

    def _認証を準備(self):
        値 = self._認証器.認証する(self._URL, 取得制御=self.取得制御)
        if type(値) is not dict or "authorization" not in 値:
            raise ValueError("Google検索認証ヘッダー不正")
        許可名 = {"authorization", "x-goog-user-project", "x-goog-api-client", "x-allowed-locations"}
        if (not set(値) <= 許可名 or any(type(v) is not str or not v
                or len(v) > 16384 or re.search(r"[^\x20-\x7e]", v) for v in 値.values())):
            raise ValueError("Google検索認証ヘッダー不正")
        if not re.fullmatch(r"Bearer [A-Za-z0-9._~+/=-]+", 値["authorization"]):
            raise ValueError("Google検索Bearer不正")
        return dict(値)

    def _要求を構成(self, url):
        if url != self._URL:
            raise ValueError("Google検索通信先不一致")
        if self.取得制御 is not None:
            self.取得制御.確認()
        _, headers, _ = super()._要求を構成(url)
        if type(self._送信許可) is not _検索送信許可:
            raise ValueError("Google検索の累積送信予約が必要")
        self._送信許可.消費(url, self._要求本文, self.取得制御)
        return "POST", {**headers, "Content-Type": "application/json; charset=utf-8",
                        **self._認証ヘッダー}, self._要求本文


class GoogleCloud検索供給器:
    """管理者の固定appに通常検索を一度送信し、許可ホストのURL・題名だけ返す。"""

    利用条件URL = "https://cloud.google.com/terms/service-terms"

    def __init__(self, プロジェクトID, アプリID, 認証器, *, 送信予算, 許可ホスト, timeout=8.0, 公開条件必須=False):
        if type(公開条件必須) is not bool:
            raise TypeError("Google検索の公開条件必須はbool")
        if type(プロジェクトID) is not str or not re.fullmatch(r"[a-z][a-z0-9-]{4,28}[a-z0-9]", プロジェクトID):
            raise ValueError("GoogleプロジェクトID不正")
        if type(アプリID) is not str or not re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,62}", アプリID):
            raise ValueError("Google検索app ID不正")
        if not callable(getattr(認証器, "認証する", None)):
            raise TypeError("Google検索には明示認証器が必要")
        if type(許可ホスト) is not tuple or not 許可ホスト or len(許可ホスト) > 50:
            raise ValueError("Google検索の公開ホスト設定不正")
        for host in 許可ホスト:
            if (type(host) is not str or any(c in host for c in "/?#@:*")
                    or urlsplit(公開URL("https://" + host)).hostname != host):
                raise ValueError("Google検索には正規化済み完全ホスト名が必要")
        if len(set(許可ホスト)) != len(許可ホスト):
            raise ValueError("Google検索公開ホスト重複")
        self.許可ホスト = 許可ホスト
        self.URL = ("https://discoveryengine.googleapis.com/v1/projects/" + プロジェクトID
                    + "/locations/global/collections/default_collection/engines/" + アプリID
                    + "/servingConfigs/default_search:search")
        if not isinstance(送信予算, GCS検索送信予算) or 送信予算.設定.検索URL != self.URL:
            raise ValueError("Google検索の固定対象に一致する累積送信予算が必要")
        self._取得器 = _検索POST取得器(self.URL, 認証器, 送信予算, timeout=timeout, 公開条件必須=公開条件必須)

    def 制御を束縛(self, 制御):
        if not isinstance(制御, 共有取得制御):
            raise TypeError("取得制御の型不正")
        obj = copy(self)
        obj._取得器 = self._取得器.制御を束縛(制御)
        return obj

    def 検索来歴付き(self, query, limit=5):
        if type(query) is not str or not query.strip() or len(query) > 512 or re.search(r"[\x00-\x1f\x7f]", query):
            raise ValueError("Google検索語不正")
        if type(limit) is not int or not 1 <= limit <= 10:
            raise ValueError("Google候補数不正")
        基底 = {"供給器": "Google Cloud Agent Search", "要求URL": self.URL,
                "利用条件URL": self.利用条件URL, "方式": "通常Search・生成指定なし"}
        try:
            取得 = self._取得器.検索JSONを取得(query.strip(), limit)
            rows = 取得.値.get("results", [])
            if type(rows) is not list or len(rows) > limit:
                raise ValueError("Google検索候補形不正")
            出力, 既出 = [], set()
            除外 = {"形式": 0, "URL": 0, "ホスト": 0, "重複": 0}
            for row in rows:
                doc = row.get("document") if type(row) is dict else None
                fields = doc.get("derivedStructData") if type(doc) is dict else None
                if type(fields) is not dict:
                    除外["形式"] += 1
                    continue
                title, link = fields.get("title"), fields.get("link")
                if (type(title) is not str or not title.strip() or len(title) > 1024
                        or re.search(r"[\x00-\x1f\x7f]", title) or type(link) is not str):
                    除外["形式"] += 1
                    continue
                try:
                    url = 公開URL(link)
                except ValueError:
                    除外["URL"] += 1
                    continue
                if urlsplit(url).hostname not in self.許可ホスト:
                    除外["ホスト"] += 1
                    continue
                if url in 既出:
                    除外["重複"] += 1
                    continue
                既出.add(url)
                title = title.strip()
                key = sha256((title + "\n" + url).encode("utf-8")).hexdigest()[:20]
                # snippet・summary・抽出回答・raw検索JSONを本文や意味根拠にしない。
                出力.append(参照資料(key, title, "Google Cloud Agent Search", url, None, ""))
            来歴 = asdict(取得)
            del 来歴["値"]
            return tuple(出力), ({**基底, "状態": "取得", "通信": 来歴,
                                  "返却候補数": len(rows), "採用候補数": len(出力), "除外": 除外},)
        except 公開送信停止:
            raise
        except 取得資源不足 as exc:
            # 受信量・期限停止も課金結果は不明。制御停止自体は再探索で握り潰さない。
            raise 取得資源不足(exc.理由, 課金結果未確認=True) from exc
        except Exception as exc:
            # token・秘密header・検索応答・例外自由文は監査へ保存しない。
            診断 = {**基底, "状態": "失敗", "種類": type(exc).__name__,
                    "検索課金": "結果未確認。送信失敗を未課金とは扱わない"}
            if isinstance(exc, JSON待機要求):
                診断["待機要求秒"] = round(exc.待機秒, 3) if exc.待機秒 != float("inf") else "自動再試行停止"
            return (), (診断,)

    def 検索(self, query, limit=5):
        return self.検索来歴付き(query, limit)[0]


def GoogleCloud知識取得器(プロジェクトID, アプリID, 認証器, *, 送信予算, 許可ホスト, timeout=8.0, 公開条件必須=False):
    from ..知識取得 import 知識取得器

    検索 = GoogleCloud検索供給器(プロジェクトID, アプリID, 認証器,
                              送信予算=送信予算, 許可ホスト=許可ホスト, timeout=timeout, 公開条件必須=公開条件必須)
    from ..公開本文取得 import 公開PDF本文取得器
    本文 = 公開PDF本文取得器(timeout=timeout, 最大バイト数=2_000_000, 許可ホスト=許可ホスト)
    return 知識取得器(検索, 本文)
