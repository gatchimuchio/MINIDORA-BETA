from __future__ import annotations
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from hashlib import sha256
import html
import re
from typing import Protocol
from urllib.request import Request, urlopen
from xml.etree import ElementTree as ET
from .型 import 参照資料, 能力結果

ニュース版 = "rss-news-v2"

class ニュース供給器(Protocol):
    def 取得(self, query: str, limit: int = 6) -> tuple[参照資料, ...]: ...

def _strip_html(text: str) -> str:
    return re.sub(r"\s+", " ", re.sub(r"<[^>]+>", " ", html.unescape(text or ""))).strip()

class RSSニュース供給器:
    def __init__(self, urls: tuple[str, ...] | None = None, timeout: float = 8.0) -> None:
        self.urls = urls or ("https://news.google.com/rss?hl=ja&gl=JP&ceid=JP:ja",)
        self.timeout = timeout

    def 取得(self, query: str, limit: int = 6) -> tuple[参照資料, ...]:
        out: list[参照資料] = []
        seen: set[str] = set()
        for url in self.urls:
            req = Request(url, headers={"User-Agent":"MINIDORA/0.5 product-demo"})
            with urlopen(req, timeout=self.timeout) as r:
                raw = r.read(2_000_000)
            root = ET.fromstring(raw)
            for item in root.findall(".//item"):
                title = _strip_html(item.findtext("title") or "")
                link = (item.findtext("link") or "").strip()
                desc = _strip_html(item.findtext("description") or "")
                情報源 = _strip_html(item.findtext('情報源') or "RSS")
                pub = None
                p = item.findtext("pubDate")
                if p:
                    try: pub = parsedate_to_datetime(p)
                    except Exception: pass
                key = sha256((title+link).encode("utf-8")).hexdigest()[:20]
                if not title or key in seen: continue
                seen.add(key)
                out.append(参照資料(key, title, 情報源, link, pub, desc or title))
                if len(out) >= limit: return tuple(out)
        return tuple(out)

class 固定ニュース供給器:
    def __init__(self, items: tuple[参照資料, ...]): self.items = items
    def 取得(self, query: str, limit: int = 6) -> tuple[参照資料, ...]: return self.items[:limit]

class ニュースモジュール:
    版 = ニュース版
    def __init__(self, provider: ニュース供給器): self.provider = provider
    def 実行(self, query: str) -> 能力結果:
        try: items = self.provider.取得(query, 6)
        except Exception as exc: return 能力結果(False, "", 保留理由=f"ニュース取得失敗:{type(exc).__name__}")
        if not items: return 能力結果(False, "", 保留理由="ニュース参照を取得できない")
        lines = ["今日の主要ニュースです。"]
        for i, x in enumerate(items,1): lines.append(f"{i}. {x.題名}（{x.出典}）")
        return 能力結果(True, "\n".join(lines), 根拠=tuple(x.識別子 for x in items), 参照=items, データ={"件数":len(items)})
