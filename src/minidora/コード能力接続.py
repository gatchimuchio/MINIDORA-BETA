'コードの読解・生成・評価・試験を既存能力と多段解決へ接続する。'
from __future__ import annotations

from .コード能力 import コード能力版, コードを読む, コードを評価, コードを検証
from .コード生成 import 関数を生成
from .応答構成 import 能力結果を復元
from .能力合成 import 登録能力
from .製品版.能力契約 import 能力文脈
from .製品版.型 import 能力結果


class コード能力モジュール:
    版 = コード能力版
    優先度 = 0

    def __init__(self, 操作: str):
        if 操作 not in ("コード読解", "コード生成", "コード評価", "コード検証"):
            raise ValueError("未対応のコード能力")
        self.名前 = 操作

    def _入力(self, 文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError("明示入力が必要")
        rows = 文脈.補助.get("合成入力", ())
        settings = 文脈.補助.get("合成設定", {})
        if type(rows) is not tuple or len(rows) != 1 or type(settings) is not dict:
            raise ValueError("単一のコード素材と設定が必要")
        value = 能力結果を復元(rows[0]["結果"])
        if not value.成立:
            raise ValueError("上流不成立")
        allowed = {"コード読解": set(), "コード生成": set(),
                   "コード評価": {"引数", "最大手数"}, "コード検証": {"試験", "最大手数"}}[self.名前]
        required = {"コード評価": {"引数"}, "コード検証": {"試験"}}.get(self.名前, set())
        if set(settings) - allowed or not required <= set(settings):
            raise ValueError("設定項目不正")
        if self.名前 == "コード生成" and set(value.データ) != {"仕様", "定数"}:
            raise ValueError('構造化仕様と定数資料が必要')
        return value, settings

    def 判定(self, 文脈):
        try:
            self._入力(文脈)
            return 1.0
        except (ValueError, TypeError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            value, settings = self._入力(文脈)
            if self.名前 == "コード生成":
                return 関数を生成(value.データ["仕様"], value.データ["定数"])
            if self.名前 == "コード読解":
                return コードを読む(value.本文)
            if self.名前 == "コード評価":
                return コードを評価(value.本文, settings["引数"], 最大手数=settings.get("最大手数", 50000))
            cases = settings["試験"]
            if type(cases) not in (list, tuple):
                raise ValueError("試験は配列")
            return コードを検証(value.本文, tuple(cases), 最大手数=settings.get("最大手数", 50000))
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
            return 能力結果(False, "", 保留理由="コード能力入力不正")

    def 登録(self):
        return 登録能力(self)


def コード能力群() -> tuple[登録能力, ...]:
    return tuple(コード能力モジュール(name).登録() for name in ("コード読解", "コード生成", "コード評価", "コード検証"))
