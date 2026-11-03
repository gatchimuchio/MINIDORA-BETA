'文章作成・差分編集を既存能力へ接続。自由文からの編集意図推定はしない。'
from __future__ import annotations

from .文章作成 import 文章版, 文章仕様を復元, 文章を作る, 文章を取り込む, 保護範囲
from .文章編集 import 文章を編集, 文章修正, 編集箇所を特定
from .応答構成 import 能力結果を復元
from .能力合成 import 登録能力, _結果辞書
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈


class 文章能力モジュール:
    版 = 文章版
    優先度 = 0

    def __init__(self, 操作: str):
        if 操作 not in ("文章作成", "文章取込", "文章編集", "文章置換箇所"):
            raise ValueError("未対応の文章能力")
        self.名前 = 操作

    def _入力(self, 文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError("明示した文章入力が必要")
        rows, settings = 文脈.補助.get("合成入力", ()), 文脈.補助.get("合成設定", {})
        if type(rows) is not tuple or len(rows) != 1 or type(settings) is not dict:
            raise ValueError("単一文章入力と設定が必要")
        value = 能力結果を復元(rows[0]["結果"])
        if not value.成立:
            raise ValueError("上流が不成立")
        keys = set(settings)
        if self.名前 == "文章作成":
            if keys or set(value.データ) != {"素材", "仕様"} or type(value.データ["素材"]) is not dict:
                raise ValueError("文章素材と構成仕様を明示する")
        elif self.名前 == "文章取込":
            if keys - {"保護", "最大文字数"}:
                raise ValueError("未知の取込設定")
        elif self.名前 == "文章置換箇所":
            if not {"検索文", "置換文"} <= keys or keys - {"検索文", "置換文", "識別子", "理由"}:
                raise ValueError("未知または不足した箇所指定")
        elif keys and keys != {"起点SHA256", "修正"}:
            raise ValueError("編集起点と修正を指定する")
        return value, settings

    def 判定(self, 文脈):
        try:
            self._入力(文脈)
            return 1.0
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
            return 0.0

    def 実行(self, 文脈):
        try:
            value, settings = self._入力(文脈)
            if self.名前 == "文章作成":
                資料 = value.データ
                return 文章を作る({k: 能力結果を復元(v) for k, v in 資料["素材"].items()}, 文章仕様を復元(資料["仕様"]))
            if self.名前 == "文章取込":
                raw = settings.get("保護", ())
                if type(raw) not in (tuple, list) or any(type(x) is not dict or set(x) != set(保護範囲.__dataclass_fields__) for x in raw):
                    raise ValueError("保護範囲の項目不正")
                return 文章を取り込む(value.本文, tuple(保護範囲(**x) for x in raw), 最大文字数=settings.get("最大文字数", 32768))
            if self.名前 == "文章置換箇所":
                proposal = 編集箇所を特定(value, **settings)
                return 能力結果(True, "置換箇所を特定", データ={"文章": _結果辞書(value), "編集要求": proposal})
            if settings:
                document, request = value, settings
            else:
                if set(value.データ) != {"文章", "編集要求"}:
                    raise ValueError("文章と編集要求を明示する")
                document = 能力結果を復元(value.データ["文章"])
                request = value.データ["編集要求"]
            if type(request) is not dict or set(request) != {"起点SHA256", "修正"} or type(request["修正"]) not in (list, tuple):
                raise ValueError("編集要求不正")
            for row in request["修正"]:
                if type(row) is not dict or set(row) != set(文章修正.__dataclass_fields__):
                    raise ValueError("修正項目不正")
            return 文章を編集(document, request["起点SHA256"], tuple(文章修正(**r) for r in request["修正"]))
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
            return 能力結果(False, "", 保留理由="文章能力入力不正")

    def 登録(self):
        return 登録能力(self)



def 文章能力群() -> tuple[登録能力, ...]:
    return tuple(文章能力モジュール(name).登録() for name in ("文章作成", "文章取込", "文章編集", "文章置換箇所"))
