'構造化関係と既存数値証拠を、関係判定・採用の能力へ接続する。'
from __future__ import annotations

from copy import deepcopy
from dataclasses import asdict
import json

from .関係制約 import (関係制約版, 関係式, 関係根拠, 関係問題, 関係問題を復元,
                        関係制約器, 関係判定を採用)
from .証拠統合 import 証拠統合器, 証拠照合要求, 証拠記録整合
from .応答構成 import 能力結果を復元
from .能力合成 import 登録能力, _結果辞書, _参照結合
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈


def 数値報告を関係化(報告: tuple[能力結果, ...], 問い: tuple[関係式, ...]) -> 能力結果:
    """同一属性・基底単位・条件・時点の報告だけを接続する。値の一点化は不要。"""
    try:
        if type(報告) is not tuple or not 1 <= len(報告) <= 8 or type(問い) is not tuple:
            raise ValueError("数値報告または問いの型不正")
        facts, links, residuals, sources, hashes, variables = [], [], [], [], [], {"@基準点"}
        範囲 = None
        for index, report in enumerate(報告, 1):
            if not 証拠記録整合(report):
                raise ValueError("証拠報告の整合違反")
            request = 証拠照合要求(**report.データ["要求"])
            再生 = 証拠統合器().実行(request, report.参照)
            raw = {**_結果辞書(report), "参照": [s.辞書化() for s in sorted(_参照結合(report.参照), key=lambda s: s.識別子)]}
            if not 再生.成立 or json.dumps(_結果辞書(再生), sort_keys=True) != json.dumps(raw, sort_keys=True):
                raise ValueError("証拠再比較不一致")
            資料 = 再生.データ
            selected = [g for g in 資料["群"] if (request.条件 is None or g["条件"] == request.条件)
                        and (request.時点 is None or g["時点"] == request.時点)]
            if len(selected) != 1:
                raise ValueError("一つの適用群の明示が必要")
            group = selected[0]
            claims = [c for c in 資料["主張"] if c["主張ID"] in group["主張ID"]]
            current = (request.属性, claims[0]["単位"], group["条件"], group["時点"])
            if 範囲 is not None and 範囲 != current:
                raise ValueError("異なる属性・単位・条件・時点を比較しない")
            範囲 = current
            if request.対象 == "@基準点":
                raise ValueError("対象名が予約変数と衝突")
            variables.add(request.対象)
            # 非一意な範囲も関係推論に使える。競合は制約系そのもので検査する。
            residuals.extend(f"報告{index}:{r}" for r in 資料["理由"] if r not in ("一意値なし", "記載競合"))
            for c in claims:
                name = f"報告{index}:{c['主張ID']}"
                facts.append(関係式(name, request.対象, c["比較"], "@基準点", c["値"]))
                links.append(関係根拠(name, c["参照ID"], c["開始"], c["終了"]))
            sources.extend(再生.参照)
            hashes.append(資料["記録SHA256"])
        p = 関係問題(tuple(sorted(variables)), tuple(facts), 問い, *範囲,
                     tuple(residuals), tuple(links))
        refs = tuple(sorted(_参照結合(sources), key=lambda s: s.識別子))
        # 構造検査は実処理に委ねるが、未宣言対象への問い等を成功で返さない。
        from .関係制約 import _検証
        _検証(p, refs)
        return 能力結果(True, "同一尺度の数値記載を関係制約へ接続しました。", 参照=refs,
                         データ={"関係問題": asdict(p), "証拠記録SHA256": hashes,
                                 "意味": "資料の数値記載からの関係。事実性と条件同一性は未確認"})
    except (TypeError, ValueError, KeyError, AttributeError, RecursionError) as exc:
        return 能力結果(False, "", 保留理由=f"数値関係接続不成立:{type(exc).__name__}")


def _入力(文脈: 能力文脈):
    if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
        raise ValueError("合成入力が必要")
    rows = 文脈.補助.get("合成入力", ())
    settings = 文脈.補助.get("合成設定", {})
    if type(rows) is not tuple or not 1 <= len(rows) <= 8 or type(settings) is not dict:
        raise ValueError("合成入力型不正")
    results = tuple(能力結果を復元(row["結果"]) for row in rows)
    if any(not r.成立 for r in results):
        raise ValueError("上流不成立")
    return results, settings


class 関係制約モジュール:
    名前 = "関係制約"
    版 = 関係制約版
    優先度 = 0

    @staticmethod
    def _問題(文脈):
        values, settings = _入力(文脈)
        if len(values) != 1 or settings or set(values[0].データ) - {"関係問題", "証拠記録SHA256", "意味"}:
            raise ValueError("単一の関係問題が必要")
        return 関係問題を復元(values[0].データ["関係問題"]), values[0].参照

    def 判定(self, 文脈):
        try:
            self._問題(文脈)
            return 1.0
        except (TypeError, ValueError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            p, refs = self._問題(文脈)
            return 関係制約器().実行(p, refs)
        except (TypeError, ValueError, KeyError, AttributeError):
            return 能力結果(False, "", 保留理由="関係入力不正")

    def 登録(self):
        return 登録能力(self)


class 数値関係化モジュール:
    名前 = "数値関係化"
    版 = 関係制約版
    優先度 = 0

    def 判定(self, 文脈):
        try:
            _, settings = _入力(文脈)
            return 1.0 if set(settings) == {"問い"} and type(settings["問い"]) in (list, tuple) else 0.0
        except (TypeError, ValueError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            reports, settings = _入力(文脈)
            if set(settings) != {"問い"} or type(settings["問い"]) not in (list, tuple):
                raise ValueError("問いの明示が必要")
            if any(type(q) is not dict or set(q) != set(関係式.__dataclass_fields__) for q in settings["問い"]):
                raise ValueError("問い項目不正")
            return 数値報告を関係化(reports, tuple(関係式(**q) for q in settings["問い"]))
        except (TypeError, ValueError, KeyError, AttributeError):
            return 能力結果(False, "", 保留理由="数値関係化入力不正")

    def 登録(self):
        return 登録能力(self)


class 関係判定採用モジュール:
    名前 = "関係判定採用"
    版 = 関係制約版
    優先度 = 0

    def 判定(self, 文脈):
        try:
            _, s = _入力(文脈)
            return 1.0 if "問いID" in s and not set(s) - {"問いID", "期待"} else 0.0
        except (TypeError, ValueError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            values, s = _入力(文脈)
            if len(values) != 1 or set(s) - {"問いID", "期待"}:
                raise ValueError("単一の関係報告が必要")
            return 関係判定を採用(values[0], s["問いID"], s.get("期待", "導出"))
        except (TypeError, ValueError, KeyError, AttributeError):
            return 能力結果(False, "", 保留理由="関係採用入力不正")

    def 登録(self):
        return 登録能力(self)
