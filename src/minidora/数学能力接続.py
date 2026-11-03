'記号演算・線形方程式・確定結果採用を既存能力へ接続する。'
from __future__ import annotations

from copy import deepcopy

from .記号演算 import 数学能力版, 記号を処理, 数学記録整合
from .線形方程式 import 線形を解く
from .応答構成 import 能力結果を復元
from .能力合成 import 登録能力
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈


class 数学能力モジュール:
    版 = 数学能力版
    優先度 = 0

    def __init__(self, 操作: str):
        if 操作 not in ("記号演算", "線形方程式", "数学結果採用"):
            raise ValueError("未対応数学能力")
        self.名前 = 操作

    def _入力(self, 文脈: 能力文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError("明示された合成入力が必要")
        rows = 文脈.補助.get("合成入力", ())
        settings = 文脈.補助.get("合成設定", {})
        if type(rows) is not tuple or len(rows) != 1 or type(settings) is not dict:
            raise ValueError('単一の数式資料と設定が必要')
        value = 能力結果を復元(rows[0]["結果"])
        if not value.成立:
            raise ValueError("上流不成立")
        資料 = value.データ
        if self.名前 == "記号演算":
            if set(settings) - {"操作", "対象変数", "代入値", "比較式", "最大演算数"}:
                raise ValueError("未知記号演算設定")
            if set(資料) == {"式", "変数"}:
                expr, names = 資料["式"], 資料["変数"]
            elif 資料.get("処理") == "記号演算" and 数学記録整合(value):
                expr, names = 資料["多項式"], 資料["多項式"]["変数"]
            else:
                raise ValueError('数式資料または成立した記号処理結果が必要')
            if type(names) not in (list, tuple):
                raise ValueError("変数宣言は配列")
            return value, (expr, tuple(names)), settings
        if self.名前 == "線形方程式":
            if set(資料) != {"変数", "方程式"} or set(settings) - {"最大演算数"}:
                raise ValueError('線形方程式資料または設定不正')
            if type(資料["変数"]) not in (list, tuple) or type(資料["方程式"]) not in (list, tuple):
                raise ValueError("変数・方程式は配列")
            return value, (tuple(資料["変数"]), tuple(資料["方程式"])), settings
        if set(settings) != {"期待"} or settings["期待"] not in ("定数値", "恒等", "一意解"):
            raise ValueError("採用条件を明示する")
        return value, (), settings

    def 判定(self, 文脈: 能力文脈) -> float:
        try:
            self._入力(文脈)
            return 1.0
        except (TypeError, ValueError, KeyError, AttributeError, RecursionError):
            return 0.0

    def 実行(self, 文脈: 能力文脈) -> 能力結果:
        try:
            value, args, settings = self._入力(文脈)
            if self.名前 == "記号演算":
                return 記号を処理(*args, **settings)
            if self.名前 == "線形方程式":
                return 線形を解く(*args, **settings)
            if not 数学記録整合(value):
                raise ValueError("数学報告の再計算不一致")
            資料 = value.データ
            expect = settings["期待"]
            if expect == "定数値":
                # 比較の差や積分の代表元を元の問いの確定数値へ昇格しない。
                ok = (資料["処理"] == "記号演算" and 資料["入力"]["操作"] in ("正規化", "代入", "微分")
                      and 資料["定数値"] is not None)
                body = 資料.get("定数値", "")
            else:
                ok = ((expect == "恒等" and 資料["処理"] == "記号演算"
                       and 資料["入力"]["操作"] == "同値比較")
                      or (expect == "一意解" and 資料["処理"] == "線形方程式")) and 資料.get("判定") == expect
                body = value.本文
            if not ok:
                return 能力結果(False, "", 保留理由="数学結果は明示した採用条件を満たさない")
            return 能力結果(True, body, 参照=value.参照, データ={
                "採用条件": expect, "結果": deepcopy(資料), "数学記録SHA256": 資料["記録SHA256"]})
        except (TypeError, ValueError, KeyError, AttributeError, RecursionError):
            return 能力結果(False, "", 保留理由="数学能力入力不正")

    def 登録(self) -> 登録能力:
        return 登録能力(self)


def 数学能力群() -> tuple[登録能力, ...]:
    return tuple(数学能力モジュール(name).登録() for name in ("記号演算", "線形方程式", "数学結果採用"))
