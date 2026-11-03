'取得資料→証拠報告→記載値採用を、既存能力合成へ接続する。'
from __future__ import annotations

from datetime import datetime

from .証拠統合 import 証拠統合器, 証拠照合要求, 証拠統合版, 記載値を採用
from .能力合成 import 登録能力
from .製品版.能力契約 import 能力文脈
from .製品版.型 import 能力結果, 参照資料


class 証拠統合モジュール:
    名前 = "証拠統合"
    版 = 証拠統合版
    優先度 = 0

    @staticmethod
    def _要求(文脈: 能力文脈) -> 証拠照合要求:
        if not isinstance(文脈, 能力文脈):
            raise ValueError("能力文脈型不正")
        settings = (文脈.補助 or {}).get("合成設定", {})
        if type(settings) is not dict or set(settings) - set(証拠照合要求.__dataclass_fields__):
            raise ValueError("未対応の証拠設定")
        request = 証拠照合要求(**settings)
        request.検証()
        return request

    def 判定(self, 文脈: 能力文脈) -> float:
        try:
            self._要求(文脈)
            return 1.0
        except (TypeError, ValueError, AttributeError):
            return 0.0

    def 実行(self, 文脈: 能力文脈) -> 能力結果:
        try:
            request = self._要求(文脈)
            return 証拠統合器().実行(request, 文脈.直前参照)
        except (TypeError, ValueError, AttributeError):
            return 能力結果(False, "", 保留理由="証拠設定不正")

    def 登録(self) -> 登録能力:
        return 登録能力(self)


class 記載値採用モジュール:
    名前 = "記載値採用"
    版 = 証拠統合版
    優先度 = 0

    def 判定(self, 文脈: 能力文脈) -> float:
        return 1.0

    def 実行(self, 文脈: 能力文脈) -> 能力結果:
        try:
            if not isinstance(文脈, 能力文脈) or (文脈.補助 or {}).get("合成設定", {}):
                raise ValueError("採用設定は不要")
            inputs = (文脈.補助 or {}).get("合成入力", ())
            if type(inputs) is not tuple or len(inputs) != 1:
                raise ValueError("単一の証拠報告が必要")
            raw = inputs[0]["結果"]
            refs = []
            for 情報源 in raw["参照"]:
                情報源 = dict(情報源)
                if 情報源["公開時刻"] is not None:
                    情報源["公開時刻"] = datetime.fromisoformat(情報源["公開時刻"])
                refs.append(参照資料(**情報源))
            report = 能力結果(raw["成立"], raw["本文"], tuple(raw["根拠"]),
                              tuple(refs), raw["データ"], raw["保留理由"])
            return 記載値を採用(report)
        except (KeyError, TypeError, ValueError, AttributeError):
            return 能力結果(False, "", 保留理由="単一の証拠報告が必要")

    def 登録(self) -> 登録能力:
        return 登録能力(self)
