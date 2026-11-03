'有限目的の探索を能力契約へ接続する。自由文や事実を新たに生成しない。'
from __future__ import annotations

from collections.abc import Iterable
from copy import deepcopy
from dataclasses import asdict

from .多段解決 import 多段解決器, 多段問題を復元, 多段解決版
from .応答構成 import 能力結果を復元
from .能力合成 import 登録能力, _結果辞書, _参照結合
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈
from .製品版.抽出 import 情報抽出モジュール


def _入力(文脈: 能力文脈):
    if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
        raise ValueError("明示された合成入力が必要")
    rows = 文脈.補助.get("合成入力", ())
    settings = 文脈.補助.get("合成設定", {})
    if type(rows) is not tuple or type(settings) is not dict:
        raise ValueError("合成入力型不正")
    values = tuple(能力結果を復元(row["結果"]) for row in rows)
    if any(not value.成立 for value in values):
        raise ValueError("上流不成立")
    return values, settings


class 素材引継ぎモジュール:
    名前 = "素材引継ぎ"
    版 = 多段解決版
    優先度 = 0

    def 判定(self, 文脈):
        try:
            values, settings = _入力(文脈)
            return 1.0 if len(values) == 1 and not settings else 0.0
        except (TypeError, ValueError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            values, settings = _入力(文脈)
            if len(values) != 1 or settings:
                raise ValueError("単一素材と空設定が必要")
            return deepcopy(values[0])
        except (TypeError, ValueError, KeyError, AttributeError):
            return 能力結果(False, "", 保留理由="素材引継ぎ入力不正")


class 成果検査モジュール:
    """局所的な非空／数値列保持の契約。意味全体の正しさを認定するものではない。"""
    名前 = "成果検査"
    版 = 多段解決版
    優先度 = 0

    @staticmethod
    def _検証入力(文脈):
        values, settings = _入力(文脈)
        if set(settings) != {"種別"} or settings["種別"] not in ("非空", "数値列保持"):
            raise ValueError("未対応の成果検査")
        expected = 1 if settings["種別"] == "非空" else 2
        if len(values) != expected:
            raise ValueError("成果検査の入力数不正")
        return values, settings["種別"]

    def 判定(self, 文脈):
        try:
            self._検証入力(文脈)
            return 1.0
        except (TypeError, ValueError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            values, kind = self._検証入力(文脈)
            if not values[0].本文.strip():
                return 能力結果(False, "", 保留理由="成果本文が空")
            if kind == "数値列保持":
                extractor = 情報抽出モジュール()
                actual = extractor.実行("数字", values[0].本文)
                expected = extractor.実行("数字", values[1].本文)
                if (not expected.成立 or not actual.成立 or actual.本文 != expected.本文
                        or actual.データ["件数"] != expected.データ["件数"]):
                    return 能力結果(False, "", 保留理由="元資料の数値列が保持されていない")
            return 能力結果(True, "成果検査合格", データ={"検査": kind})
        except (TypeError, ValueError, KeyError, AttributeError):
            return 能力結果(False, "", 保留理由="成果検査入力不正")


def 解決補助能力群() -> tuple[登録能力, ...]:
    return tuple(登録能力(m) for m in (素材引継ぎモジュール(), 成果検査モジュール()))


class 多段解決モジュール:
    名前 = "多段解決"
    版 = 多段解決版
    優先度 = 0

    def __init__(self, 能力: Iterable[登録能力], *, 純粋作用確認: bool = False):
        self._実行器 = 多段解決器(能力, 純粋作用確認=純粋作用確認)

    @staticmethod
    def _問題(文脈):
        values, settings = _入力(文脈)
        if len(values) != 1 or set(values[0].データ) != {"問題", '初期資料'}:
            raise ValueError("単一の多段問題が必要")
        if set(settings) - {"最大展開数", "最大呼出数", "最大深さ"}:
            raise ValueError("未知の探索設定")
        raw = values[0].データ
        if type(raw['初期資料']) is not dict:
            raise ValueError('初期資料型不正')
        return (多段問題を復元(raw["問題"]),
                {k: 能力結果を復元(v) for k, v in raw['初期資料'].items()}, settings)

    def 判定(self, 文脈):
        try:
            self._問題(文脈)
            return 1.0
        except (TypeError, ValueError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            p, 資料, settings = self._問題(文脈)
            結果 = self._実行器.実行(p, 資料, **settings)
            record = {"版": 多段解決版, "状態": 結果.状態, "理由": 結果.理由,
                      "出力": {k: _結果辞書(v) for k, v in 結果.出力},
                      "採用経路": 結果.採用経路, "探索履歴": 結果.履歴,
                      "展開数": 結果.展開数, "呼出数": 結果.呼出数,
                      "開始ハッシュ": 結果.開始ハッシュ, "ハッシュ": 結果.ハッシュ}
            return 能力結果(結果.成立, "\n\n".join(v.本文 for _, v in 結果.出力),
                根拠=(結果.ハッシュ,),
                参照=_参照結合(ref for _, v in 結果.出力 for ref in v.参照),
                データ=record, 保留理由=結果.理由)
        except (TypeError, ValueError, KeyError, AttributeError, RecursionError):
            return 能力結果(False, "", 保留理由="多段解決入力不正")

    def 登録(self):
        return 登録能力(self)
