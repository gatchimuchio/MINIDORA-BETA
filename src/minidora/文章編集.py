'文章の起点と期待原文を照合し、指定範囲だけを原子的に編集する。\n\n修正文は上位の資料。修正案の発見や、保護外の意味保持の自動証明はしない。\n'
from __future__ import annotations

from copy import deepcopy
from dataclasses import asdict, dataclass
import re

from .文章作成 import (文章版, 文章境界違反, _文字, _名前, _範囲, _項目,
    _符号, _初期構成, _原本結果, _結果, _失敗, _複写対応)
from .能力合成 import _結果辞書
from .製品版.型 import 能力結果


@dataclass(frozen=True, slots=True)
class 文章修正:
    識別子: str
    開始: int
    終了: int
    期待原文: str
    置換文: str
    理由: str


def _修正を適用(状態, modifications, revision):
    if type(modifications) is not list or not 1 <= len(modifications) <= 64:
        raise 文章境界違反("修正は1〜64件")
    old = 状態["本文"]
    ids = set()
    for row in modifications:
        _項目(row, 文章修正)
        _名前(row["識別子"])
        if row["識別子"] in ids:
            raise 文章境界違反("同一編集内の修正ID重複")
        ids.add(row["識別子"])
        _範囲(row["開始"], row["終了"], old, 空可=True)
        _文字(row["期待原文"], 100000)
        _文字(row["置換文"], 100000)
        _文字(row["理由"], 512, 空可=False)
        if old[row["開始"]:row["終了"]] != row["期待原文"]:
            raise 文章境界違反("期待原文と指定範囲が一致しない")
        if row["期待原文"] == row["置換文"]:
            raise 文章境界違反("変化のない修正")
    ordered = sorted(modifications, key=lambda r: (r["開始"], r["終了"]))
    for a, b in zip(ordered, ordered[1:]):
        if a["終了"] > b["開始"] or a["開始"] == b["開始"]:
            raise 文章境界違反("修正範囲が交差または同位置で曖昧")
    for row in ordered:
        start, end = row["開始"], row["終了"]
        for lock in 状態["保護"]:
            a, b = lock["開始"], lock["終了"]
            if (start < b and a < end) or (start == end and a < start < b):
                raise 文章境界違反("保護された引用・条件・指定範囲への修正")
    # 全修正を検査後にまとめて適用する。座標は全て編集前の同じ本文が基点。
    pieces, spans, changes = [], [], []
    cursor, pos = 0, 0
    for row in ordered:
        a, b = row["開始"], row["終了"]
        unchanged = old[cursor:a]
        pieces.append(unchanged)
        spans.extend(_複写対応(状態["対応"], cursor, a, pos)); pos += len(unchanged)
        inserted = row["置換文"]
        if inserted:
            pieces.append(inserted)
            spans.append({"開始": pos, "終了": pos+len(inserted), "由来": {
                "種別": "修正文", "ID": row["識別子"], "改訂": revision, "開始": 0, "終了": len(inserted)}})
        changes.append({"修正ID": row["識別子"], "旧開始": a, "旧終了": b,
            "新開始": pos, "新終了": pos+len(inserted), "旧文": row["期待原文"],
            "新文": inserted, "理由": row["理由"]})
        pos += len(inserted); cursor = b
    pieces.append(old[cursor:])
    spans.extend(_複写対応(状態["対応"], cursor, len(old), pos))
    new = deepcopy(状態)
    new.update(本文="".join(pieces), 対応=spans, 直近差分=changes)
    for lock in new["保護"]:
        shift = sum(len(r["置換文"])-(r["終了"]-r["開始"]) for r in ordered if r["終了"] <= lock["開始"])
        lock["開始"] += shift; lock["終了"] += shift
    return new


def _再構成(root, history):
    if type(history) is not list or len(history) > 16:
        raise 文章境界違反("編集履歴は16回以内")
    結果 = _原本結果(root)
    状態 = _初期構成(root)
    for i, event in enumerate(history):
        if type(event) is not dict or set(event) != {"起点SHA256", "修正"}:
            raise 文章境界違反("編集履歴の項目不正")
        if event["起点SHA256"] != 結果.データ["記録SHA256"]:
            raise 文章境界違反("履歴の編集起点が一致しない")
        状態 = _修正を適用(状態, event["修正"], i+1)
        結果 = _結果(root, history[:i+1], 状態)
    return 結果


def 文章記録整合(結果: 能力結果) -> bool:
    try:
        _結果辞書(結果)
        if (結果.成立 is not True or 結果.根拠 or 結果.保留理由
                or 結果.データ.get("版") != 文章版 or len(_符号(_結果辞書(結果))) > 2000000):
            return False
        再生 = _再構成(結果.データ["原本"], 結果.データ["編集履歴"])
        # 参照資料は合成器が追加し得る。参照一覧自体の認証をこの検査に含めない。
        return 再生.本文 == 結果.本文 and _符号(再生.データ) == _符号(結果.データ)
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return False


def 文章を編集(文章: 能力結果, 起点SHA256: str, 修正: tuple[文章修正, ...]) -> 能力結果:
    try:
        if not 文章記録整合(文章):
            raise 文章境界違反("文章記録が原本・編集履歴と不一致")
        if (type(起点SHA256) is not str or not re.fullmatch('[0-9a-f]{64}', 起点SHA256)
                or 起点SHA256 != 文章.データ["記録SHA256"]):
            raise 文章境界違反("古い文章または異なる記録を指す編集起点")
        if type(修正) is not tuple or any(type(r) is not 文章修正 for r in 修正):
            raise 文章境界違反("修正の型不正")
        history = deepcopy(文章.データ["編集履歴"]) + [{"起点SHA256": 起点SHA256, "修正": [asdict(r) for r in 修正]}]
        return _再構成(deepcopy(文章.データ["原本"]), history)
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
        return _失敗(exc)


def 編集箇所を特定(文章: 能力結果, 検索文: str, 置換文: str, *,
                 識別子: str = "置換", 理由: str = "明示された文字列置換") -> dict:
    """完全一致が一箇所だけなら修正案を返す。適用はしない。曖昧な全置換をしない。"""
    if not 文章記録整合(文章):
        raise 文章境界違反("文章の整合不成立")
    _文字(検索文, 100000, 空可=False); _文字(置換文, 100000)
    _名前(識別子); _文字(理由, 512, 空可=False)
    first = 文章.本文.find(検索文)
    if first < 0 or 文章.本文.find(検索文, first+1) >= 0:
        raise 文章境界違反("検索文は存在する一箇所に限定する")
    return {"起点SHA256": 文章.データ["記録SHA256"], "修正": [asdict(
        文章修正(識別子, first, first+len(検索文), 検索文, 置換文, 理由))]}
