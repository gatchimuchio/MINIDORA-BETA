from __future__ import annotations

from hashlib import sha256
import json
import os
import re
from threading import Lock
from typing import Protocol
from urllib.parse import urlencode, urlsplit
from urllib.request import Request, urlopen

from .型 import 参照資料, 能力結果

検索版 = "local-web-search-v1"


class Web検索供給器(Protocol):
    def 検索(self, query: str, limit: int = 5) -> tuple[参照資料, ...]: ...


class SearXNG検索供給器:
    'ローカルSearXNGのJSON Search APIを外部資料参照として利用する。'

    def __init__(self, base_url: str | None = None, timeout: float = 10.0) -> None:
        self.base_url = (
            base_url
            or os.environ.get("MINIDORA_SEARXNG_URL")
            or "http://127.0.0.1:8888"
        ).rstrip("/")
        self.timeout = timeout

    def _get(self, url: str) -> dict:
        req = Request(
            url,
            headers={
                "User-Agent": "MINIDORA/0.5 local-web-search",
                "Accept": "application/json",
            },
        )
        with urlopen(req, timeout=self.timeout) as response:
            return json.loads(response.read(2_000_000).decode("utf-8"))

    @staticmethod
    def _text(value: object) -> str:
        text = str(value or "")
        text = re.sub(r"<[^>]+>", "", text)
        return re.sub(r"\s+", " ", text).strip()

    def 検索(self, query: str, limit: int = 5) -> tuple[参照資料, ...]:
        q = str(query or "").strip()
        if not q:
            return ()
        limit = max(1, min(int(limit), 10))
        params = urlencode(
            {
                "q": q,
                "format": "json",
                '言語': "ja-JP",
                "safesearch": "0",
            }
        )
        資料 = self._get(f"{self.base_url}/search?{params}")
        out: list[参照資料] = []
        for item in 資料.get("results", []) or []:
            if len(out) >= limit:
                break
            if not isinstance(item, dict):
                continue
            title = self._text(item.get("title"))
            url = str(item.get("url") or "").strip()
            if not title or not url:
                continue
            content = self._text(item.get("content"))
            engines = item.get("engines")
            if isinstance(engines, list):
                情報源 = ", ".join(self._text(x) for x in engines if self._text(x))
            else:
                情報源 = self._text(item.get("engine"))
            情報源 = 情報源 or "Web"
            key = sha256((title + "\n" + url).encode("utf-8")).hexdigest()[:20]
            out.append(参照資料(key, title, 情報源, url, None, content))
        return tuple(out)


class 固定Web検索供給器:
    def __init__(self, items: tuple[参照資料, ...]) -> None:
        self.items = items

    def 検索(self, query: str, limit: int = 5) -> tuple[参照資料, ...]:
        return self.items[: max(1, int(limit))]


class Wikipedia検索供給器:
    """既存Wikipedia検索の候補だけ供給する。本文・意味判断は後段の別責任。"""

    許可ホスト = ("ja.wikipedia.org", "en.wikipedia.org")
    許可経路 = ("/w/rest.php/v1/search/page",)
    利用条件URL = "https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use"

    def __init__(self, *, timeout: float = 8.0):
        from ..HTTP参照 import Wikipedia参照供給器
        from ..公開本文取得 import 公開JSON取得器

        class 候補参照供給器(Wikipedia参照供給器):
            候補のみ = True

        self._取得器 = 公開JSON取得器(許可ホスト=self.許可ホスト,
                                      許可経路=self.許可経路, timeout=timeout)
        self._供給器 = tuple(候補参照供給器(言語=言語, timeout=timeout,
                                              JSON取得=self._JSONを取得)
                              for 言語 in ("ja", "en"))
        self._排他 = Lock()
        self._通信: list[dict] = []

    def 制御を束縛(self, 制御):
        # 各参照供給器のcallbackは新しいselfへ束縛する。
        obj = Wikipedia検索供給器(timeout=self._取得器.timeout)
        obj._取得器 = self._取得器.制御を束縛(制御)
        return obj

    def _JSONを取得(self, url: str, headers, timeout: float) -> dict:
        from dataclasses import asdict
        from ..公開本文取得 import JSON待機要求

        try:
            取得 = self._取得器.取得(url)
        except Exception as exc:
            # 応答本文や例外の自由文は記録しない。失敗を本文成功に変換しない。
            記録 = {"状態": "失敗", "要求URL": url, "種類": type(exc).__name__}
            if isinstance(exc, JSON待機要求):
                記録["待機要求秒"] = round(exc.待機秒, 3) if exc.待機秒 != float("inf") else "自動再試行停止"
            self._通信.append(記録)
            raise
        来歴 = asdict(取得)
        del 来歴["値"]
        self._通信.append({"状態": "取得", **来歴})
        return 取得.値

    def 検索来歴付き(self, query: str, limit: int = 5) -> tuple[tuple[参照資料, ...], tuple[dict, ...]]:
        from ..公開本文取得 import 公開URL

        if type(query) is not str or not query.strip() or len(query) > 512 or re.search(r"[\x00-\x1f\x7f]", query):
            raise ValueError("Wikipedia検索語不正")
        if type(limit) is not int or not 1 <= limit <= 10:
            raise ValueError("Wikipedia候補数不正")
        with self._排他:
            出力, 来歴, 既出 = [], [], set()
            for 供給器 in self._供給器:
                self._通信 = []
                候補, 診断 = 供給器.検索診断(query, limit - len(出力))
                来歴.append({"供給器": 供給器.名称, "状態": 診断.状態,
                              "通信": tuple(self._通信), "利用条件URL": self.利用条件URL})
                for 候補資料 in 候補:
                    url = 公開URL(候補資料.由来)
                    部分 = urlsplit(url)
                    if 部分.hostname not in self.許可ホスト or not 部分.path.startswith("/wiki/") or 部分.query:
                        raise ValueError("Wikipedia記事URL不正")
                    if url in 既出:
                        continue
                    既出.add(url)
                    出力.append(参照資料(候補資料.識別子, 候補資料.対象,
                                           供給器.名称, url, None, ""))
                    if len(出力) >= limit:
                        break
                if len(出力) >= limit:
                    break
            # 各検索の通信記録と候補を同時に返す。別要求の最後の検索を参照しない。
            return tuple(出力), tuple(来歴)

    def 検索(self, query: str, limit: int = 5) -> tuple[参照資料, ...]:
        return self.検索来歴付き(query, limit)[0]


def Wikipedia知識取得器(*, timeout: float = 8.0):
    """明示設定用。従来のSearXNG・一般Wikipedia参照のdefaultは変えない。"""
    from ..公開本文取得 import 公開本文取得器
    from ..知識取得 import 知識取得器

    本文 = 公開本文取得器(timeout=timeout, 最大バイト数=2_000_000,
                         許可ホスト=Wikipedia検索供給器.許可ホスト)
    本文._利用者識別 = "MINIDORA/knowledge-retrieval-0.1 (+https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA)"
    return 知識取得器(Wikipedia検索供給器(timeout=timeout), 本文)


class Web検索モジュール本体:
    版 = 検索版

    def __init__(self, provider: Web検索供給器, limit: int = 5) -> None:
        self.provider = provider
        self.limit = max(1, min(int(limit), 10))

    def 実行(self, query: str) -> 能力結果:
        q = str(query or "").strip()
        if not q:
            return 能力結果(False, "", 保留理由="検索語が空")
        try:
            refs = self.provider.検索(q, self.limit)
        except Exception as exc:
            return 能力結果(False, "", 保留理由=f"Web検索失敗:{type(exc).__name__}")
        if not refs:
            return 能力結果(False, "", 保留理由="Web検索結果が見つからない")

        rows: list[str] = []
        for index, ref in enumerate(refs, 1):
            row = f"{index}. {ref.題名}"
            if ref.本文:
                row += f"\n   {ref.本文}"
            if ref.URL:
                row += f"\n   {ref.URL}"
            rows.append(row)
        body = "Web検索結果:\n" + "\n".join(rows)
        return 能力結果(
            True,
            body,
            根拠=tuple(ref.識別子 for ref in refs),
            参照=refs,
            データ={
                "検索語": q,
                "参照数": len(refs),
                "検索基盤": type(self.provider).__name__,
            },
        )
