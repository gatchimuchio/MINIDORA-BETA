from __future__ import annotations

from collections.abc import Callable, Mapping
from html.parser import HTMLParser
import html
import json
from threading import Lock
from typing import Any
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

from .参照 import 参照記録


JSON取得器 = Callable[[str, Mapping[str, str], float], Mapping[str, Any]]


def _JSON取得(url: str, headers: Mapping[str, str], timeout: float) -> Mapping[str, Any]:
    request = Request(url, headers=dict(headers), method="GET")
    with urlopen(request, timeout=timeout) as response:
        payload = response.read()
    decoded = json.loads(payload.decode("utf-8"))
    if not isinstance(decoded, Mapping):
        raise ValueError("JSON objectを期待した")
    return decoded


class _MarkupText(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []

    def handle_data(self, 資料: str) -> None:
        self.parts.append(資料)

    def text(self) -> str:
        return " ".join("".join(self.parts).split()).strip()


def _text(value: object) -> str:
    if value is None:
        return ""
    if isinstance(value, list):
        return " ".join(_text(item) for item in value if _text(item)).strip()
    return " ".join(str(value).split()).strip()


def _markup_text(value: object) -> str:
    raw = str(value or "")
    parser = _MarkupText()
    try:
        parser.feed(raw)
        return html.unescape(parser.text())
    except Exception:
        return _text(raw)


def _doi_identifier(doi: str) -> str:
    normalized = doi.strip().casefold()
    for prefix in ("https://doi.org/", "http://doi.org/", "doi:"):
        if normalized.startswith(prefix):
            normalized = normalized[len(prefix):]
            break
    return "doi:" + normalized


def _date(row: Mapping[str, Any]) -> str | None:
    for field in ("published-print", "published-online", "published", "created"):
        value = row.get(field)
        if not isinstance(value, Mapping):
            continue
        parts = value.get("date-parts")
        if isinstance(parts, list) and parts and isinstance(parts[0], list) and parts[0]:
            nums = [str(item) for item in parts[0] if isinstance(item, int)]
            if nums:
                return "-".join(nums)
        raw = _text(value.get("date-time"))
        if raw:
            return raw
    return None


class Crossref参照供給器:
    'Crossref REST APIを使う、分野横断のkey不要学術メタデータProvider。\n\n    公開poolの同時接続制限を超えないようProvider内部でHTTP呼出しを直列化する。\n    APIの検索順位・被引用数は真偽信頼度へ変換しない。DOIがある資料は他Providerと\n    共通識別子を使い、同一論文を複数独立情報源として数えない。\n    '

    名称 = "Crossref"
    BASE_URL = "https://api.crossref.org/works"
    並列安全 = False
    ABSTRACT信頼 = 0.72
    TITLE_ONLY信頼 = 0.46

    def __init__(
        self,
        *,
        timeout: float = 12.0,
        user_agent: str = "MINIDORA/0.5 (Crossref reference provider)",
        連絡先メール: str | None = None,
        JSON取得: JSON取得器 | None = None,
        最大本文文字数: int = 12000,
    ) -> None:
        self.timeout = float(timeout)
        self.user_agent = str(user_agent)
        self.連絡先メール = _text(連絡先メール) or None
        self._get_json = JSON取得 or _JSON取得
        self.最大本文文字数 = max(256, int(最大本文文字数))
        self.最後のエラー: str | None = None
        self._lock = Lock()
        self._cache: dict[tuple[str, int], tuple[参照記録, ...]] = {}

    @property
    def cache件数(self) -> int:
        with self._lock:
            return len(self._cache)

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        query = _text(問合せ)
        limit = min(max(0, int(上限)), 1000)
        if not query or limit <= 0:
            return ()
        cache_key = (query.casefold(), limit)

        with self._lock:
            cached = self._cache.get(cache_key)
            if cached is not None:
                return cached

            params = {"query": query, "rows": str(limit)}
            if self.連絡先メール:
                params["mailto"] = self.連絡先メール
            url = self.BASE_URL + "?" + urlencode(params)
            try:
                payload = self._get_json(
                    url,
                    {"User-Agent": self.user_agent, "Accept": "application/json"},
                    self.timeout,
                )
                self.最後のエラー = None
            except Exception as exc:
                self.最後のエラー = f"{type(exc).__name__}: {exc}"
                return ()

            message = payload.get("message")
            if not isinstance(message, Mapping):
                self.最後のエラー = "ProtocolError: message missing"
                return ()
            items = message.get("items")
            if not isinstance(items, list):
                self.最後のエラー = "ProtocolError: message.items missing"
                return ()

            records: list[参照記録] = []
            seen: set[str] = set()
            for row in items:
                if not isinstance(row, Mapping):
                    continue
                doi = _text(row.get("DOI"))
                title = _text(row.get("title"))
                abstract = _markup_text(row.get("abstract"))
                if not doi and not title:
                    continue

                identifier = _doi_identifier(doi) if doi else "crossref:title:" + title.casefold()
                key = identifier.casefold()
                if key in seen:
                    continue

                pieces = [value for value in (title, abstract) if value]
                content = "\n".join(pieces)[: self.最大本文文字数]
                if not content:
                    continue
                信頼度 = self.ABSTRACT信頼 if abstract else self.TITLE_ONLY信頼
                origin = "https://doi.org/" + quote(doi, safe="/:()-.;") if doi else "https://api.crossref.org/works"

                conditions: list[tuple[str, str]] = [
                    ('証拠_範囲', "abstract" if abstract else "title"),
                ]
                if doi:
                    conditions.append(('canonical_情報源', _doi_identifier(doi)))
                container = _text(row.get("container-title"))
                work_type = _text(row.get("type"))
                if container:
                    conditions.append(("container", container))
                if work_type:
                    conditions.append(("work_type", work_type))

                records.append(
                    参照記録(
                        識別子=identifier,
                        対象=title or doi,
                        内容=content,
                        由来=origin,
                        供給器=self.名称,
                        信頼=信頼度,
                        時点=_date(row),
                        条件=tuple(conditions),
                    )
                )
                seen.add(key)
                if len(records) >= limit:
                    break

            結果 = tuple(records)
            self._cache[cache_key] = 結果
            return 結果


__all__ = ["Crossref参照供給器"]
