from __future__ import annotations

from collections.abc import Callable, Mapping
from html.parser import HTMLParser
import html
import json
from threading import Lock
from typing import Any
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

from .意味字句 import 意味語
from .参照 import 参照記録, 参照取得診断


JSON取得器 = Callable[[str, Mapping[str, str], float], Mapping[str, Any]]


def _JSON取得(url: str, headers: Mapping[str, str], timeout: float) -> Mapping[str, Any]:
    request = Request(url, headers=dict(headers), method="GET")
    with urlopen(request, timeout=timeout) as response:
        payload = response.read()
    decoded = json.loads(payload.decode("utf-8"))
    if not isinstance(decoded, Mapping):
        raise ValueError("JSON objectを期待した")
    return decoded


def _abstract(index: object) -> str:
    if not isinstance(index, Mapping):
        return ""
    positioned: list[tuple[int, str]] = []
    for word, positions in index.items():
        if not isinstance(positions, list):
            continue
        for position in positions:
            if isinstance(position, int):
                positioned.append((position, str(word)))
    positioned.sort(key=lambda item: item[0])
    return " ".join(word for _, word in positioned)


class _HTMLTextExtractor(HTMLParser):
    _SKIP = {"script", "style", "noscript", "table", "sup"}

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._skip_depth = 0
        self.parts: list[str] = []

    def handle_starttag(self, tag: str, attrs) -> None:
        if tag.casefold() in self._SKIP:
            self._skip_depth += 1
        elif self._skip_depth == 0 and tag.casefold() in {"p", "h1", "h2", "h3", "li", "br"}:
            self.parts.append("\n")

    def handle_endtag(self, tag: str) -> None:
        if tag.casefold() in self._SKIP and self._skip_depth > 0:
            self._skip_depth -= 1
        elif self._skip_depth == 0 and tag.casefold() in {"p", "h1", "h2", "h3", "li"}:
            self.parts.append("\n")

    def handle_data(self, 資料: str) -> None:
        if self._skip_depth == 0:
            self.parts.append(資料)

    def text(self) -> str:
        lines = [" ".join(part.split()) for part in "".join(self.parts).splitlines()]
        return "\n".join(line for line in lines if line).strip()


def _html_text(raw: object) -> str:
    parser = _HTMLTextExtractor()
    try:
        parser.feed(str(raw or ""))
        return html.unescape(parser.text())
    except Exception:
        return " ".join(str(raw or "").split())


def _Wikipedia本文選択(
    *,
    query: str,
    title: str,
    description: str,
    excerpt: str,
    full_text: str,
    limit: int,
) -> str:
    """検索意味に近い段落を優先し、長文の先頭切捨てによるrelevant fact損失を抑える。"""
    q_terms = 意味語(query)
    paragraphs = [line.strip() for line in full_text.splitlines() if line.strip()]
    scored: list[tuple[int, int, str]] = []
    for index, paragraph in enumerate(paragraphs):
        p_terms = 意味語(paragraph)
        overlap = len(q_terms & p_terms) if q_terms else 0
        scored.append((overlap, index, paragraph))

    matched = [(score, index, paragraph) for score, index, paragraph in scored if score > 0]
    if matched:
        matched.sort(key=lambda item: (-item[0], item[1]))
        # 強い段落を選んだ後、本文順へ戻して局所文脈を維持する。
        selected_indices = {index for _, index, _ in matched[:12]}
        selected = [paragraph for _, index, paragraph in scored if index in selected_indices]
    else:
        selected = paragraphs

    pieces: list[str] = []
    seen: set[str] = set()
    for value in (title, description, excerpt, *selected):
        normalized = " ".join(str(value).split()).strip()
        if not normalized:
            continue
        key = normalized.casefold()
        if key in seen:
            continue
        seen.add(key)
        pieces.append(normalized)

    out: list[str] = []
    used = 0
    for piece in pieces:
        extra = len(piece) + (1 if out else 0)
        if out and used + extra > limit:
            continue
        if not out and len(piece) > limit:
            out.append(piece[:limit])
            break
        out.append(piece)
        used += extra
        if used >= limit:
            break
    return "\n".join(out).strip()


class OpenAlex参照供給器:
    名称 = "OpenAlex"
    BASE_URL = "https://api.openalex.org/works"
    並列安全 = False

    def __init__(self, api_key: str, *, timeout: float = 12.0, user_agent: str = "MINIDORA/0.4 (OpenAlex reference provider)", JSON取得: JSON取得器 | None = None, 最大本文文字数: int = 12000) -> None:
        if not str(api_key).strip():
            raise ValueError("OpenAlex API keyが必要")
        self.api_key = str(api_key).strip()
        self.timeout = float(timeout)
        self.user_agent = user_agent
        self._get_json = JSON取得 or _JSON取得
        self.最大本文文字数 = int(最大本文文字数)
        self.最後のエラー: str | None = None
        self._error_lock = Lock()

    def _error(self, value: str | None) -> None:
        with self._error_lock:
            self.最後のエラー = value

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        query = " ".join(str(問合せ).split()).strip()
        if not query or 上限 <= 0:
            return ()
        params = {"search": query, "per_page": str(min(max(1, int(上限)), 100)), "select": "id,doi,display_name,publication_year,abstract_inverted_index,is_retracted,relevance_score", "api_key": self.api_key}
        url = self.BASE_URL + "?" + urlencode(params)
        try:
            payload = self._get_json(url, {"User-Agent": self.user_agent, "Accept": "application/json"}, self.timeout)
            self._error(None)
        except Exception as exc:
            self._error(f"{type(exc).__name__}: {exc}")
            return ()
        rows = payload.get("results")
        if not isinstance(rows, list):
            self._error("ProtocolError: results missing")
            return ()
        records: list[参照記録] = []
        for row in rows:
            if not isinstance(row, Mapping) or row.get("is_retracted") is True:
                continue
            work_id = str(row.get("id") or "").strip()
            title = str(row.get("display_name") or "").strip()
            abstract = _abstract(row.get("abstract_inverted_index"))
            content = "\n".join(value for value in (title, abstract) if value).strip()
            if not work_id or not content:
                continue
            year = row.get("publication_year")
            doi = str(row.get("doi") or "").strip()
            records.append(参照記録("openalex:" + work_id.rsplit("/", 1)[-1], title or work_id, content[: self.最大本文文字数], doi or work_id, self.名称, 1.0, 時点=str(year) if year is not None else None))
            if len(records) >= 上限:
                break
        return tuple(records)


class Wikipedia参照供給器:
    並列安全 = False
    # 限定アダプターだけが候補検索を選ぶ。従来の本文参照の挙動は変えない。
    候補のみ = False

    def __init__(self, *, 言語: str = "en", timeout: float = 12.0, user_agent: str = "MINIDORA/0.4 (Wikipedia reference provider)", JSON取得: JSON取得器 | None = None, 最大本文文字数: int = 12000) -> None:
        言語 = str(言語).strip().casefold()
        if not 言語 or not 言語.replace("-", "").isalnum():
            raise ValueError("Wikipedia言語コードが不正")
        self.言語 = 言語
        self.名称 = f"Wikipedia:{言語}"
        self.timeout = float(timeout)
        self.user_agent = user_agent
        self._get_json = JSON取得 or _JSON取得
        self.最大本文文字数 = int(最大本文文字数)
        self.最後のエラー: str | None = None
        self._page_cache: dict[str, Mapping[str, Any] | None] = {}
        self._cache_lock = Lock()
        self._page_locks: dict[str, Lock] = {}
        self._error_lock = Lock()

    @property
    def base(self) -> str:
        return f"https://{self.言語}.wikipedia.org/w/rest.php/v1"

    @property
    def 本文cache件数(self) -> int:
        with self._cache_lock:
            return len(self._page_cache)

    def _error(self, value: str | None) -> None:
        with self._error_lock:
            self.最後のエラー = value

    def _key_lock(self, key: str) -> Lock:
        with self._cache_lock:
            lock = self._page_locks.get(key)
            if lock is None:
                lock = Lock()
                self._page_locks[key] = lock
            return lock

    def _page(self, key: str) -> Mapping[str, Any] | None:
        with self._cache_lock:
            if key in self._page_cache:
                return self._page_cache[key]
        with self._key_lock(key):
            with self._cache_lock:
                if key in self._page_cache:
                    return self._page_cache[key]
            url = f"{self.base}/page/{quote(key, safe='')}/with_html"
            try:
                value = self._get_json(url, {"User-Agent": self.user_agent, "Accept": "application/json"}, self.timeout)
            except Exception as exc:
                self._error(f"{type(exc).__name__}: {exc}")
                value = None
            if isinstance(value, Mapping):
                with self._cache_lock:
                    self._page_cache[key] = value
            return value

    def 検索診断(self, 問合せ: str, 上限: int = 8) -> tuple[tuple[参照記録, ...], 参照取得診断]:
        query = " ".join(str(問合せ).split()).strip()
        if not query or 上限 <= 0:
            return (), 参照取得診断(query, self.名称, "空", 0)
        url = self.base + "/search/page?" + urlencode({"q": query, "limit": str(min(max(1, int(上限)), 100))})
        try:
            payload = self._get_json(url, {"User-Agent": self.user_agent, "Accept": "application/json"}, self.timeout)
            self._error(None)
        except Exception as exc:
            error = f"{type(exc).__name__}: {exc}"
            self._error(error)
            return (), 参照取得診断(query, self.名称, "失敗", 0, 1, error)
        pages = payload.get("pages")
        if not isinstance(pages, list):
            error = "ProtocolError: pages missing"
            self._error(error)
            return (), 参照取得診断(query, self.名称, "失敗", 0, 1, error)

        records: list[参照記録] = []
        detail_errors: list[str] = []
        for row in pages:
            if not isinstance(row, Mapping):
                continue
            key = str(row.get("key") or row.get("title") or "").strip()
            title = str(row.get("title") or key).strip()
            page_id = str(row.get("id") or key).strip()
            description = str(row.get("description") or "").strip()
            excerpt = _html_text(row.get("excerpt"))
            self._error(None)
            detail = self._page(key) if key and not self.候補のみ else None
            detail_error = self.最後のエラー
            if detail is None and detail_error:
                detail_errors.append(f"{key}:{detail_error}")
            full_text = _html_text(detail.get("html")) if isinstance(detail, Mapping) else ""
            content = _Wikipedia本文選択(
                query=query,
                title=title,
                description=description,
                excerpt=excerpt,
                full_text=full_text,
                limit=self.最大本文文字数,
            )
            if not content and not self.候補のみ:
                continue
            情報源_url = f"https://{self.言語}.wikipedia.org/wiki/{quote(key.replace(' ', '_'), safe='')}"
            条件 = (("wikipedia_detail", "degraded"),) if detail is None and detail_error else ()
            records.append(参照記録(
                f"wikipedia:{self.言語}:{page_id}",
                title or key,
                content,
                情報源_url,
                self.名称,
                1.0 if not 条件 else 0.8,
                条件=条件,
            ))
            if len(records) >= 上限:
                break

        if detail_errors:
            error = "; ".join(detail_errors)
            self._error(error)
            return tuple(records), 参照取得診断(query, self.名称, "縮退", len(records), 1, error)
        self._error(None)
        状態 = "取得" if records else "空"
        return tuple(records), 参照取得診断(query, self.名称, 状態, len(records))

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        return self.検索診断(問合せ, 上限)[0]


__all__ = ["OpenAlex参照供給器", "Wikipedia参照供給器"]
