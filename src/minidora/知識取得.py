"""必要語の不足に応じて検索・本文取得・抜粋を行う。事実性の採否は行わない。"""
from __future__ import annotations

from collections.abc import Callable
from copy import copy, deepcopy
from dataclasses import asdict, dataclass, fields
from datetime import datetime
from hashlib import sha256
import json
import re
from typing import Protocol
import unicodedata
from urllib.parse import urlsplit

from .公開本文取得 import 公開URL, 公開本文取得器, 取得本文
from .製品版.型 import 能力結果, 参照資料
from .取得実行制御 import 共有取得制御, 取得資源不足, 制御を配送する
from .公開送信条件 import 公開語を検査, 公開送信停止

知識取得版 = "MINIDORA-知識取得-v0.1"
知識継続版 = "MINIDORA-知識取得継続-v1"


def _記録印(値):
    return sha256(json.dumps(値, ensure_ascii=False, sort_keys=True, allow_nan=False).encode("utf-8")).hexdigest()


class 検索供給器(Protocol):
    def 検索(self, query: str, limit: int = 5) -> tuple[参照資料, ...]: ...


class 本文供給器(Protocol):
    def 取得(self, url: str) -> 取得本文: ...


def _照合語(text: str) -> str:
    return unicodedata.normalize("NFKC", text).casefold()


@dataclass(frozen=True, slots=True)
class 知識取得要求:
    検索語: str
    必要語: tuple[str, ...]
    最低資料数: int = 1
    最大検索回数: int = 3
    最大候補数: int = 5
    最大取得数: int = 8
    最大資料数: int = 4
    最大抜粋文字数: int = 12000
    優先ホスト: tuple[str, ...] = ()

    def 検証(self) -> None:
        if type(self.検索語) is not str or not self.検索語.strip() or len(self.検索語) > 512:
            raise ValueError("検索語不正")
        if type(self.必要語) is not tuple or not 1 <= len(self.必要語) <= 16:
            raise ValueError("必要語は1〜16語のtuple")
        for word in (self.検索語, *self.必要語):
            if type(word) is not str or not word.strip() or re.search(r"[\x00-\x1f\x7f]", word):
                raise ValueError("検索語・必要語の型または制御文字が不正")
            word.encode("utf-8")
        if any(len(w) > 80 or w != w.strip() for w in self.必要語):
            raise ValueError("必要語の長さまたは前後空白が不正")
        if len({_照合語(w) for w in self.必要語}) != len(self.必要語):
            raise ValueError("必要語重複")
        for name, upper in (("最低資料数", 8), ("最大検索回数", 8), ("最大候補数", 10),
                            ("最大取得数", 32), ("最大資料数", 8), ("最大抜粋文字数", 50000)):
            value = getattr(self, name)
            if type(value) is not int or not 1 <= value <= upper:
                raise ValueError(f"{name}範囲不正")
        if not self.最低資料数 <= self.最大資料数 <= self.最大取得数:
            raise ValueError("資料数・取得数の上限矛盾")
        if type(self.優先ホスト) is not tuple or len(self.優先ホスト) > 16:
            raise ValueError("優先ホスト不正")
        for host in self.優先ホスト:
            if type(host) is not str or not host or any(c in host for c in "/?#@:"):
                raise ValueError("優先ホスト不正")
            if urlsplit(公開URL("https://" + host)).hostname != host:
                raise ValueError("優先ホストは正規化済みの完全ホスト名")


def _本文検証(doc: 取得本文, requested: str) -> None:
    if not isinstance(doc, 取得本文) or doc.要求URL != requested:
        raise ValueError("本文の要求先不一致")
    if 公開URL(doc.最終URL) != doc.最終URL:
        raise ValueError("最終URL不正")
    if type(doc.経路) is not tuple or not doc.経路 or doc.経路[0] != requested or doc.経路[-1] != doc.最終URL:
        raise ValueError("本文の取得経路不一致")
    if any(公開URL(u) != u for u in doc.経路) or len(set(doc.経路)) != len(doc.経路):
        raise ValueError("本文の取得経路不正")
    for text in (doc.題名, doc.本文, doc.取得時刻, doc.内容種別, doc.文字コード):
        if type(text) is not str:
            raise ValueError("本文メタデータ型不正")
    if not doc.本文.strip() or doc.本文SHA256 != sha256(doc.本文.encode("utf-8")).hexdigest():
        raise ValueError("本文内容の不一致")
    if not re.fullmatch(r"[0-9a-f]{64}", doc.転送SHA256):
        raise ValueError("転送hash不正")
    if datetime.fromisoformat(doc.取得時刻).utcoffset() is None:
        raise ValueError("取得時刻に時差が必要")
    if type(doc.転送バイト数) is not int or not 0 < doc.転送バイト数 <= 2_000_000:
        raise ValueError("本文サイズ不正")
    if type(doc.除外要素) is not tuple or any(type(x) is not str for x in doc.除外要素):
        raise ValueError("除外記録不正")
    if type(doc.抽出頁範囲) is not tuple:
        raise ValueError("抽出頁記録不正")
    if doc.内容種別 == "application/pdf":
        from .公開PDF抽出 import PDF抽出版, 最大頁数, 最大本文文字数
        if doc.文字コード != PDF抽出版 or not 1 <= len(doc.抽出頁範囲) <= 最大頁数 or len(doc.本文) > 最大本文文字数:
            raise ValueError("PDF抽出記録不正")
        end = 0
        for number, row in enumerate(doc.抽出頁範囲, 1):
            if (type(row) is not tuple or len(row) != 3 or any(type(x) is not int for x in row)
                    or row[0] != number or row[1] != end or not end < row[2] <= len(doc.本文)):
                raise ValueError("PDF抽出頁対応不正")
            end = row[2]
            if number < len(doc.抽出頁範囲):
                if doc.本文[end:end + 1] != "\n":
                    raise ValueError("PDF抽出頁区切り不正")
                end += 1
        if end != len(doc.本文):
            raise ValueError("PDF抽出頁対応不正")
    elif doc.抽出頁範囲:
        raise ValueError("非PDFの抽出頁記録")


def _本文記録を復元(metadata: dict, text: str) -> 取得本文:
    """JSON交換形から取得本文を復元。旧非PDF記録だけ頁欄の省略を許す。"""
    if type(metadata) is not dict:
        raise ValueError("本文記録の型不正")
    raw = {f.name: text if f.name == "本文" else metadata[f.name]
           for f in fields(取得本文) if f.name != "抽出頁範囲"}
    for name in ("経路", "除外要素"):
        if type(raw[name]) not in (tuple, list):
            raise ValueError("本文記録の列型不正")
        raw[name] = tuple(raw[name])
    if "抽出頁範囲" not in metadata and raw["内容種別"] == "application/pdf":
        raise ValueError("PDF抽出頁記録なし")
    pages = metadata.get("抽出頁範囲", ())
    if (type(pages) not in (tuple, list)
            or any(type(row) not in (tuple, list) or len(row) != 3 for row in pages)):
        raise ValueError("抽出頁記録不正")
    raw["抽出頁範囲"] = tuple(tuple(row) for row in pages)
    doc = 取得本文(**raw)
    _本文検証(doc, doc.要求URL)
    return doc


def _抜粋(doc: 取得本文, terms: tuple[str, ...], remaining: int) -> tuple[list[dict], set[str]]:
    """抽出本文の段落をそのまま保持。照合はNFKC/casefold部分一致に限定する。"""
    passages, covered = [], set()
    for match in re.finditer(r"[^\n]+", doc.本文):
        text = match[0]
        hits = [w for w in terms if _照合語(w) in _照合語(text)]
        if not hits or len(text) > remaining:
            continue
        passages.append({"開始": match.start(), "終了": match.end(), "本文": text, "一致語": hits})
        remaining -= len(text)
        covered.update(hits)
    return passages, covered


def _資料を追加(doc, 題名, 検索語, 要求, documents, passages, refs):
    remaining = 要求.最大抜粋文字数 - sum(len(p["本文"]) for p in passages)
    selected, hits = _抜粋(doc, 要求.必要語, remaining)
    if not selected:
        return None
    key = "web:" + sha256((doc.最終URL + "\n" + doc.本文SHA256).encode()).hexdigest()[:24]
    refs.append(参照資料(key, doc.題名 or 題名, "公開HTTPS本文", doc.最終URL, None, doc.本文))
    passages.extend({"参照ID": key, **p} for p in selected)
    metadata = asdict(doc)
    del metadata["本文"]
    metadata.update({"参照ID": key, "検索語": 検索語,
                     "優先元一致": urlsplit(doc.最終URL).hostname in 要求.優先ホスト,
                     "公開時刻": None})
    documents.append(metadata)
    return key, hits


def _取得結果(要求, 成立, 理由, documents, passages, refs, traces, attempts, covered,
             検索来歴, 継続束, 制御):
    資料 = {"版": 知識取得版, "状態": "合格" if 成立 else "保留",
            "意味的事実検証": "未実施", "要求": asdict(要求),
            "不足語": [w for w in 要求.必要語 if w not in covered],
            "資料": documents, "抜粋": passages, "試行": traces,
            "本文取得数": attempts, "資料数": len(refs), "継続束": deepcopy(継続束)}
    if 制御 is not None:
        資料["資源制御"] = 制御.記録()
    if 検索来歴:
        資料["検索来歴"] = 検索来歴
    encoded = json.dumps(資料, ensure_ascii=False, sort_keys=True, allow_nan=False).encode("utf-8")
    if len(encoded) + sum(len(r.本文.encode("utf-8")) for r in refs) > 1_000_000:
        return 能力結果(False, "", 保留理由="取得記録サイズ上限")
    資料["記録SHA256"] = sha256(encoded).hexdigest()
    return 能力結果(成立, "\n\n".join(p["本文"] for p in passages) if 成立 else "",
                    根拠=tuple(r.識別子 for r in refs), 参照=tuple(refs), データ=資料, 保留理由=理由)


class 知識取得器:
    def __init__(self, 検索: 検索供給器, 本文: 本文供給器 | None = None):
        if not callable(getattr(検索, "検索", None)):
            raise ValueError("検索供給器が必要")
        self.検索 = 検索
        self.本文 = 本文 if 本文 is not None else 公開本文取得器()
        if not callable(getattr(self.本文, "取得", None)):
            raise ValueError("本文供給器が必要")
        self.取得制御 = None

    def 制御を束縛(self, 制御):
        if not isinstance(制御, 共有取得制御):
            raise TypeError("取得制御の型不正")
        obj = copy(self)
        obj.取得制御 = 制御
        return obj

    def 実行(self, 要求: 知識取得要求, *, 外部読取許可: bool = False,
             停止要求: Callable[[], bool] | None = None) -> 能力結果:
        traces, documents, passages, refs, 検索来歴 = [], [], [], [], []
        covered: set[str] = set()
        attempts = 0
        継続束 = {"版": 知識継続版, "要求SHA256": None, "候補": [], "消費位置": 0,
                 "検索済み語": [], "試行済みURL": [], "親記録SHA256": None}

        def 結果(成立: bool, 理由: str = "") -> 能力結果:
            return _取得結果(要求, 成立, 理由, documents, passages, refs, traces, attempts,
                         covered, 検索来歴, 継続束, self.取得制御)

        def 停止() -> bool:
            if 停止要求 is None:
                return False
            value = 停止要求()
            if type(value) is not bool:
                raise ValueError("停止要求型不正")
            return value

        try:
            if not isinstance(要求, 知識取得要求):
                raise ValueError("取得要求型不正")
            要求.検証()
            if type(外部読取許可) is not bool or not 外部読取許可:
                return 能力結果(False, "", 保留理由="外部読取未許可")
            制御 = self.取得制御
            公開語を検査(要求.検索語, 要求.必要語, 制御.公開条件 if 制御 is not None else None)
            継続束["要求SHA256"] = _記録印(asdict(要求))
            検索器 = 制御を配送する(self.検索, 制御, "検索供給器")
            本文器 = 制御を配送する(self.本文, 制御, "本文供給器")
            tried_urls, tried_queries, hashes, final_urls = set(), set(), set(), set()
            query = 要求.検索語.strip()
            for round_no in range(要求.最大検索回数):
                公開語を検査(query, 条件=制御.公開条件 if 制御 is not None else None)
                if 停止():
                    return 結果(False, "停止要求")
                if query in tried_queries:
                    break
                候補上限 = 要求.最大候補数
                if 制御 is not None:
                    候補上限 = min(候補上限, 制御.残量("候補数"))
                    if 候補上限 <= 0:
                        raise 取得資源不足("候補数上限")
                    if 制御.残量("本文取得数") <= 0:
                        raise 取得資源不足("本文取得数上限")
                    制御.消費("検索回数")
                tried_queries.add(query)
                継続束["検索済み語"].append(query)
                traces.append({"作用": "検索", "検索語": query, "回": round_no + 1})
                try:
                    来歴検索 = getattr(検索器, "検索来歴付き", None)
                    if callable(来歴検索):
                        candidates, 来歴 = 来歴検索(query, 候補上限)
                        if type(来歴) is not tuple or len(来歴) > 10 or any(type(row) is not dict for row in 来歴):
                            raise ValueError("検索来歴型不正")
                        encoded = json.dumps(来歴, ensure_ascii=False, allow_nan=False)
                        if len(encoded.encode("utf-8")) > 64_000:
                            raise ValueError("検索来歴サイズ上限")
                        検索来歴.append({"検索語": query, "回": round_no + 1,
                                        "取得": json.loads(encoded)})
                    else:
                        candidates = 検索器.検索(query, 候補上限)
                    if type(candidates) is not tuple:
                        raise ValueError("検索結果型不正")
                except (取得資源不足, 公開送信停止):
                    raise
                except Exception as exc:
                    traces.append({"作用": "検索失敗", "種類": type(exc).__name__})
                    candidates = ()
                if 停止():
                    return 結果(False, "停止要求")
                ordered = []
                for item in candidates[:候補上限]:
                    if 制御 is not None:
                        制御.消費("候補数")
                    try:
                        if (not isinstance(item, 参照資料) or type(item.題名) is not str
                                or len(item.題名) > 1024):
                            raise ValueError("検索候補型不正")
                        url = 公開URL(item.URL)
                        優先度 = urlsplit(url).hostname in 要求.優先ホスト
                        ordered.append((not 優先度, url, item))
                    except (ValueError, TypeError):
                        traces.append({"作用": "候補除外", "理由": "候補型または公開URL不正"})
                ordered.sort(key=lambda row: row[0])  # 同順位は検索供給器の順序を保持する。
                継続束["候補"].extend({"URL": url, "題名": item.題名, "検索語": query,
                                       "回": round_no + 1, "順": 順}
                                      for 順, (_, url, item) in enumerate(ordered))
                for _, url, 候補 in ordered:
                    if 停止():
                        return 結果(False, "停止要求")
                    if attempts >= 要求.最大取得数 or len(refs) >= 要求.最大資料数:
                        break
                    継続束["消費位置"] += 1
                    if url in tried_urls or url in final_urls:
                        traces.append({"作用": "重複URL", "URL": url})
                        continue
                    tried_urls.add(url)
                    if 制御 is not None:
                        制御.消費("本文取得数")
                    attempts += 1
                    継続束["試行済みURL"].append(url)
                    try:
                        doc = 本文器.取得(url)
                        _本文検証(doc, url)
                    except 取得資源不足:
                        raise
                    except Exception as exc:
                        traces.append({"作用": "本文取得失敗", "URL": url, "種類": type(exc).__name__})
                        continue
                    if 停止():
                        return 結果(False, "停止要求")
                    if doc.本文SHA256 in hashes or doc.最終URL in final_urls:
                        traces.append({"作用": "重複資料", "URL": url, "最終URL": doc.最終URL})
                        continue
                    追加 = _資料を追加(doc, 候補.題名, query, 要求, documents, passages, refs)
                    if 追加 is None:
                        traces.append({"作用": "本文非採用", "URL": url, "理由": "必要語不一致または段落長上限"})
                        continue
                    key, hits = 追加
                    hashes.add(doc.本文SHA256)
                    final_urls.add(doc.最終URL)
                    covered.update(hits)
                    traces.append({"作用": "本文採用", "URL": url, "一致語": sorted(hits), "参照ID": key})
                    if all(w in covered for w in 要求.必要語) and len(refs) >= 要求.最低資料数:
                        return 結果(True)
                if attempts >= 要求.最大取得数 or len(refs) >= 要求.最大資料数:
                    break
                missing = [w for w in 要求.必要語 if w not in covered]
                query = 要求.検索語.strip() + " " + " ".join(missing or 要求.必要語)
            return 結果(False, "必要語または最低資料数が未充足")
        except 公開送信停止 as exc:
            return 能力結果(False, "", データ=exc.記録(), 保留理由="公開送信条件不成立")
        except 取得資源不足 as exc:
            traces.append({"作用": "取得資源保留", "理由": exc.理由})
            if exc.課金結果未確認:
                traces.append({"作用": "外部検索課金", "状態": "結果未確認"})
            return 結果(False, "取得資源:" + exc.理由)
        except Exception as exc:
            # 故障した停止判定器・不正入力から検索や代替回答へ進まない。
            return 能力結果(False, "", 保留理由=f"知識取得契約・制御失敗:{type(exc).__name__}")

    def 継続(self, 要求, 前成果, *, 外部読取許可=False, 停止要求=None):
        """検証済みの未消費候補だけを読む。新しい検索・採否判断はしない。"""
        from .知識取得継続 import 継続束を読む
        try:
            要求.検証()
            公開語を検査(要求.検索語, 要求.必要語,
                         self.取得制御.公開条件 if self.取得制御 is not None else None)
            束 = 継続束を読む(前成果, 要求)
            if type(外部読取許可) is not bool or not 外部読取許可:
                return 能力結果(False, "", 保留理由="外部読取未許可")
            制御 = self.取得制御
            if "資源制御" in 前成果.データ:
                if 制御 is None:
                    raise ValueError("当回の共有取得制御が必要")
                旧, 今 = 前成果.データ["資源制御"], 制御.記録()
                if (旧["版"] != "公開取得資源-v2" or 旧["当回ID"] != 今["当回ID"]
                        or 旧["上限"] != 今["上限"] or
                        any(今["使用"][名] < 数 for 名, 数 in 旧["使用"].items())):
                    raise ValueError("共有取得資源の巻き戻し")
            本文器 = 制御を配送する(self.本文, 制御, "本文供給器")
            d = deepcopy(前成果.データ)
            documents, passages, refs, traces = d["資料"], d["抜粋"], list(前成果.参照), d["試行"]
            attempts = d["本文取得数"]
            covered = {w for p in passages for w in p["一致語"]}
            hashes = {行["本文SHA256"] for 行 in documents}
            final_urls = {行["最終URL"] for 行 in documents}
            tried_urls = set(束["試行済みURL"])
            束["親記録SHA256"] = d["記録SHA256"]

            def 結果(成立, 理由=""):
                return _取得結果(要求, 成立, 理由, documents, passages, refs, traces, attempts,
                             covered, d.get("検索来歴", []), 束, 制御)

            while 束["消費位置"] < len(束["候補"]):
                if 停止要求 is not None:
                    停止 = 停止要求()
                    if type(停止) is not bool:
                        raise ValueError("停止要求型不正")
                    if 停止:
                        return 結果(False, "停止要求")
                if attempts >= 要求.最大取得数 or len(refs) >= 要求.最大資料数:
                    return 結果(False, "取得数または資料数上限")
                候補 = 束["候補"][束["消費位置"]]
                url = 候補["URL"]
                束["消費位置"] += 1
                if url in tried_urls or url in final_urls:
                    traces.append({"作用": "重複URL", "URL": url})
                    continue
                if 制御 is not None:
                    制御.消費("本文取得数")
                tried_urls.add(url)
                束["試行済みURL"].append(url)
                attempts += 1
                try:
                    doc = 本文器.取得(url)
                    _本文検証(doc, url)
                except 取得資源不足:
                    raise
                except Exception as exc:
                    traces.append({"作用": "本文取得失敗", "URL": url, "種類": type(exc).__name__})
                    continue
                if 停止要求 is not None:
                    停止 = 停止要求()
                    if type(停止) is not bool:
                        raise ValueError("停止要求型不正")
                    if 停止:
                        return 結果(False, "停止要求")
                if doc.本文SHA256 in hashes or doc.最終URL in final_urls:
                    traces.append({"作用": "重複資料", "URL": url, "最終URL": doc.最終URL})
                    continue
                追加 = _資料を追加(doc, 候補["題名"], 候補["検索語"], 要求, documents, passages, refs)
                if 追加 is None:
                    traces.append({"作用": "本文非採用", "URL": url, "理由": "必要語不一致または段落長上限"})
                    continue
                key, hits = 追加
                hashes.add(doc.本文SHA256)
                final_urls.add(doc.最終URL)
                covered.update(hits)
                traces.append({"作用": "本文採用", "URL": url, "一致語": sorted(hits), "参照ID": key})
                # 新しい原本文の追加だけで帰還し、意味上の充足は同じHDSに委ねる。
                return 結果(True)
            return 結果(False, "未消費候補がありません")
        except 公開送信停止 as exc:
            return 能力結果(False, "", データ=exc.記録(), 保留理由="公開送信条件不成立")
        except 取得資源不足 as exc:
            traces.append({"作用": "取得資源保留", "理由": exc.理由})
            return 結果(False, "取得資源:" + exc.理由)
        except Exception:
            return 能力結果(False, "", 保留理由="知識取得継続の契約不一致")
