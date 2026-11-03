"""所有する原記録から選択した全素材を、既存の能力契約へ接続する。

標準チャット・既存短期会話状態を置換しない。状態変更は明示APIでのみ行う。
"""
from __future__ import annotations

from dataclasses import asdict

from .長文脈管理 import 長文脈庫, 長文脈起点, 文脈選択要求, 長文脈版
from .応答構成 import 能力結果を復元
from .能力合成 import 登録能力
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈


def 長文脈要求資料(庫: 長文脈庫, 要求: 文脈選択要求) -> 能力結果:
    if not isinstance(庫, 長文脈庫) or not isinstance(要求, 文脈選択要求):
        raise ValueError("長文脈庫と選択要求が必要")
    要求.検証()
    return 能力結果(True, "", データ={"起点": asdict(庫.起点()), "選択要求": asdict(要求)})


class 長文脈選択モジュール:
    名前 = "長文脈選択"
    版 = 長文脈版
    優先度 = 0

    def __init__(self, 庫: 長文脈庫):
        if not isinstance(庫, 長文脈庫):
            raise ValueError("所有する長文脈庫が必要")
        self._庫 = 庫

    def _入力(self, 文脈: 能力文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError("明示した文脈選択入力が必要")
        if 文脈.セッションID != self._庫.起点().セッションID:
            raise ValueError("セッション不一致")
        rows = 文脈.補助.get("合成入力", ())
        if type(rows) is not tuple or len(rows) != 1 or 文脈.補助.get("合成設定", {}):
            raise ValueError("単一入力・空設定が必要")
        value = 能力結果を復元(rows[0]["結果"])
        raw = value.データ
        if not value.成立 or set(raw) != {"起点", "選択要求"}:
            raise ValueError('文脈選択資料不正')
        if (type(raw["起点"]) is not dict or set(raw["起点"]) != set(長文脈起点.__dataclass_fields__)
                or type(raw["選択要求"]) is not dict or set(raw["選択要求"]) != set(文脈選択要求.__dataclass_fields__)):
            raise ValueError("起点または要求の項目不一致")
        settings = dict(raw["選択要求"])
        for name in ("必須ID", "検索語"):
            if type(settings[name]) not in (tuple, list):
                raise ValueError("文脈指定は配列")
            settings[name] = tuple(settings[name])
        request = 文脈選択要求(**settings)
        request.検証()
        start = 長文脈起点(**raw["起点"])
        # boolと整数の等値で起点の版検査を通さない。
        if type(start.世代) is not int or type(start.改訂) is not int:
            raise ValueError("起点の版型不正")
        if start != self._庫.起点():
            raise ValueError("別所有者・旧状態")
        return request, start

    def 判定(self, 文脈: 能力文脈) -> float:
        try:
            self._入力(文脈)
            return 1.0
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
            return 0.0

    def 実行(self, 文脈: 能力文脈) -> 能力結果:
        try:
            request, start = self._入力(文脈)
            selected = self._庫.選択(request, 起点=start)
            if not selected.成立:
                return 能力結果(False, "", 保留理由=selected.理由,
                    データ={"必須バイト数": selected.必須バイト数, "予算": request.最大バイト数})
            return self._庫.資料化(selected)
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
            return 能力結果(False, "", 保留理由="文脈選択入力不正・状態変更")

    def 登録(self) -> 登録能力:
        return 登録能力(self)
