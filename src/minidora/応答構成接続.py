'複数の証拠報告を、既存能力合成から応答構成へ渡す。'
from __future__ import annotations

from .応答構成 import 応答構成器, 応答仕様, 応答構成版, 能力結果を復元
from .能力合成 import 登録能力
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈


class 応答構成モジュール:
    名前 = "応答構成"
    版 = 応答構成版
    優先度 = 0

    @staticmethod
    def _入力(文脈: 能力文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError("合成入力が必要")
        settings = 文脈.補助.get("合成設定", {})
        if type(settings) is not dict or set(settings) - set(応答仕様.__dataclass_fields__):
            raise ValueError("未対応の応答設定")
        spec = 応答仕様(**settings)
        spec.検証()
        rows = 文脈.補助.get("合成入力", ())
        if type(rows) is not tuple or not 1 <= len(rows) <= 8:
            raise ValueError("証拠報告は1〜8件")
        reports = tuple(能力結果を復元(row["結果"]) for row in rows)
        return reports, spec

    def 判定(self, 文脈: 能力文脈) -> float:
        try:
            self._入力(文脈)
            return 1.0
        except (TypeError, ValueError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈: 能力文脈) -> 能力結果:
        try:
            reports, spec = self._入力(文脈)
            return 応答構成器().実行(reports, spec)
        except (TypeError, ValueError, KeyError, AttributeError, RecursionError):
            return 能力結果(False, "", 保留理由="応答構成入力不正")

    def 登録(self) -> 登録能力:
        return 登録能力(self)
