from __future__ import annotations

from collections.abc import Callable, Mapping
import json
from threading import Lock
from typing import Any
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

from .参照 import 参照記録


JSON取得器 = Callable[[str, Mapping[str, str], float], Mapping[str, Any]]

_外部鍵_結果種別 = "resultType"
_外部鍵_結果一覧 = "resultList"
_外部鍵_結果 = "result"
_外部鍵_情報源 = "source"


def _JSON取得(url: str, headers: Mapping[str, str], timeout: float) -> Mapping[str, Any]:
    request = Request(url, headers=dict(headers), method="GET")
    with urlopen(request, timeout=timeout) as response:
        payload = response.read()
    decoded = json.loads(payload.decode("utf-8"))
    if not isinstance(decoded, Mapping):
        raise ValueError("JSON objectを期待した")
    return decoded


def _text(value: object) -> str:
    return " ".join(str(value or "").split()).strip()


def _truthy(value: object) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().casefold() in {"1", "true", "yes", "y"}


def _doi_identifier(doi: str) -> str:
    normalized = doi.strip().casefold()
    for prefix in ("https://doi.org/", "http://doi.org/", "doi:"):
        if normalized.startswith(prefix):
            normalized = normalized[len(prefix):]
            break
    return "doi:" + normalized


class EuropePMC参照供給器:
    'Europe PMC REST searchを使う、API key不要の科学文献参照Provider。\n\n    検索順位は文献候補の取得順にだけ利用し、真偽信頼度へ変換しない。\n    abstract本文がある文献を主証拠とし、titleしかないレコードは低信頼度で保持する。\n    DOIがある文献はProvider横断で共通識別子を使い、同一論文の独立情報源水増しを防ぐ。\n    '

    名称 = "EuropePMC"
    BASE_URL = "https://www.ebi.ac.uk/europepmc/webservices/rest/search"
    並列安全 = False
    ABSTRACT信頼 = 0.82
    TITLE_ONLY信頼 = 0.55

    def __init__(
        self,
        *,
        timeout: float = 12.0,
        user_agent: str = "MINIDORA/0.5 (Europe PMC reference provider)",
        JSON取得: JSON取得器 | None = None,
        最大本文文字数: int = 12000,
        同義語展開: bool = False,
    ) -> None:
        self.timeout = float(timeout)
        self.user_agent = str(user_agent)
        self._get_json = JSON取得 or _JSON取得
        self.最大本文文字数 = max(256, int(最大本文文字数))
        self.同義語展開 = bool(同義語展開)
        self.最後のエラー: str | None = None
        self._error_lock = Lock()

    def _error(self, value: str | None) -> None:
        with self._error_lock:
            self.最後のエラー = value

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        query = _text(問合せ)
        if not query or 上限 <= 0:
            return ()

        params = {
            "query": query,
            _外部鍵_結果種別: "core",
            "pageSize": str(min(max(1, int(上限)), 1000)),
            "format": "json",
            "synonym": "true" if self.同義語展開 else "false",
        }
        url = self.BASE_URL + "?" + urlencode(params)
        try:
            payload = self._get_json(
                url,
                {"User-Agent": self.user_agent, "Accept": "application/json"},
                self.timeout,
            )
            self._error(None)
        except Exception as exc:
            self._error(f"{type(exc).__name__}: {exc}")
            return ()

        結果_list = payload.get(_外部鍵_結果一覧)
        if not isinstance(結果_list, Mapping):
            self._error("ProtocolError: resultList missing")
            return ()
        rows = 結果_list.get(_外部鍵_結果)
        if not isinstance(rows, list):
            self._error("ProtocolError: resultList.result missing")
            return ()

        records: list[参照記録] = []
        seen: set[str] = set()
        for row in rows:
            if not isinstance(row, Mapping):
                continue
            if _truthy(row.get("isRetracted")) or _truthy(row.get("retracted")):
                continue

            情報源 = _text(row.get(_外部鍵_情報源)) or "未知"
            article_id = _text(row.get("id") or row.get("pmid") or row.get("pmcid") or row.get("doi"))
            if not article_id:
                continue
            doi = _text(row.get("doi"))
            identifier = _doi_identifier(doi) if doi else f"europepmc:{情報源}:{article_id}"
            key = identifier.casefold()
            if key in seen:
                continue

            title = _text(row.get("title"))
            abstract = _text(row.get("abstractText"))
            journal = _text(row.get("journalTitle"))
            publication_type = ""
            pub_types = row.get("pubTypeList")
            if isinstance(pub_types, Mapping):
                raw_types = pub_types.get("pubType", ())
                if isinstance(raw_types, list):
                    publication_type = "; ".join(_text(value) for value in raw_types if _text(value))

            pieces = [value for value in (title, abstract) if value]
            if not pieces:
                continue
            content = "\n".join(pieces)[: self.最大本文文字数]
            信頼度 = self.ABSTRACT信頼 if abstract else self.TITLE_ONLY信頼

            origin = (
                "https://doi.org/" + quote(doi, safe="/:()-.;")
                if doi
                else f"https://europepmc.org/article/{quote(情報源, safe='')}/{quote(article_id, safe='')}"
            )
            date = _text(row.get("firstPublicationDate") or row.get("firstIndexDate") or row.get("pubYear")) or None
            conditions: list[tuple[str, str]] = []
            if doi:
                conditions.append(('canonical_情報源', _doi_identifier(doi)))
            if journal:
                conditions.append(("journal", journal))
            if publication_type:
                conditions.append(("publication_type", publication_type))
            conditions.append(('証拠_範囲', "abstract" if abstract else "title"))

            records.append(
                参照記録(
                    識別子=identifier,
                    対象=title or article_id,
                    内容=content,
                    由来=origin,
                    供給器=self.名称,
                    信頼=信頼度,
                    時点=date,
                    条件=tuple(conditions),
                )
            )
            seen.add(key)
            if len(records) >= 上限:
                break
        return tuple(records)


__all__ = ["EuropePMC参照供給器"]
