'明示したブラウザ要求を外部読取能力として接続する。純粋な多段探索へは登録しない。'
from __future__ import annotations

from .ブラウザ閲覧 import ブラウザ閲覧器, ブラウザ要求, ブラウザ工程
from .ブラウザ通信 import ブラウザ版
from .能力合成 import 登録能力
from .応答構成 import 能力結果を復元
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈


def ブラウザ要求を復元(raw: dict) -> ブラウザ要求:
    if type(raw) is not dict or set(raw) != set(ブラウザ要求.__dataclass_fields__):
        raise ValueError("閲覧要求の項目不一致")
    if type(raw["許可URL"]) not in (list, tuple) or type(raw["工程"]) not in (list, tuple):
        raise ValueError("閲覧要求の配列型不正")
    for step in raw["工程"]:
        if type(step) is not dict or set(step) != set(ブラウザ工程.__dataclass_fields__):
            raise ValueError("閲覧工程の項目不一致")
    request = ブラウザ要求(raw["開始URL"], tuple(raw["許可URL"]),
                          tuple(ブラウザ工程(**s) for s in raw["工程"]), raw["待機ミリ秒"])
    request.検証()
    return request


class ブラウザ閲覧モジュール:
    名前 = "ブラウザ閲覧"
    版 = ブラウザ版
    優先度 = 0

    def __init__(self, 閲覧器: ブラウザ閲覧器, *, 外部読取許可: bool = False):
        if not isinstance(閲覧器, ブラウザ閲覧器) or type(外部読取許可) is not bool:
            raise ValueError("閲覧器と明示した許可が必要")
        self._閲覧器, self._許可 = 閲覧器, 外部読取許可

    @staticmethod
    def _要求(文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError('明示した閲覧資料が必要')
        rows = 文脈.補助.get("合成入力", ())
        settings = 文脈.補助.get("合成設定", {})
        if type(rows) is not tuple or len(rows) != 1 or type(settings) is not dict or settings:
            raise ValueError("単一の要求と空の設定が必要")
        value = 能力結果を復元(rows[0]["結果"])
        if not value.成立 or set(value.データ) != {"要求"}:
            raise ValueError('成立した閲覧要求資料が必要')
        return ブラウザ要求を復元(value.データ["要求"])

    def 判定(self, 文脈):
        try:
            self._要求(文脈)
            return 1.0
        except (TypeError, ValueError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            return self._閲覧器.実行(self._要求(文脈), 外部読取許可=self._許可)
        except (TypeError, ValueError, KeyError, AttributeError):
            return 能力結果(False, "", 保留理由='閲覧要求資料不正')

    def 登録(self):
        return 登録能力(self, 外部読取=True)
