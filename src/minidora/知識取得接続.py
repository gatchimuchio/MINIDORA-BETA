'知識取得を既存能力契約へ接続する。検索の実値は合成設定資料から受け取る。'
from __future__ import annotations

from .知識取得 import 知識取得器, 知識取得要求, 知識取得版
from .能力合成 import 登録能力
from .製品版.能力契約 import 能力文脈
from .製品版.型 import 能力結果
from .取得実行制御 import 制御を配送する
from .能力結果復元 import 能力結果を復元
from .知識取得継続 import 継続束を読む, 継続進展を検査


class 知識取得モジュール:
    名前 = "知識取得"
    版 = 知識取得版
    優先度 = 0

    def __init__(self, 取得器: 知識取得器, *, 外部読取許可: bool = False):
        if type(外部読取許可) is not bool:
            raise ValueError("外部読取許可はbool")
        self.取得器, self.外部読取許可 = 取得器, 外部読取許可

    @staticmethod
    def _要求(文脈: 能力文脈) -> 知識取得要求:
        if not isinstance(文脈, 能力文脈):
            raise ValueError("能力文脈型不正")
        settings = (文脈.補助 or {}).get("合成設定", {})
        allowed = set(知識取得要求.__dataclass_fields__)
        if type(settings) is not dict or set(settings) - allowed or not {"検索語", "必要語"} <= set(settings):
            raise ValueError("取得設定が未確定または未対応")
        values = dict(settings)
        for field in ("必要語", "優先ホスト"):
            if field in values and type(values[field]) is list:
                values[field] = tuple(values[field])
        request = 知識取得要求(**values)
        request.検証()
        return request

    def 判定(self, 文脈: 能力文脈) -> float:
        try:
            要求 = self._要求(文脈)
            self._継続元(文脈, 要求)
            return 1.0
        except (TypeError, ValueError, AttributeError, KeyError):
            return 0.0

    @staticmethod
    def _継続元(文脈, 要求):
        入力 = (文脈.補助 or {}).get("合成入力", ())
        if type(入力) is not tuple or len(入力) > 1:
            raise ValueError("取得入力は初回0件・継続1件に限定します")
        if not 入力:
            return None
        行 = 入力[0]
        if (type(行) is not dict or set(行) != {"参照", "結果"}
                or type(行["参照"]) is not dict
                or set(行["参照"]) != {"領域", "識別子"}
                or 行["参照"]["領域"] != "入力"):
            raise ValueError("取得継続元は明示した入力素材に限定します")
        前 = 能力結果を復元(行["結果"])
        継続束を読む(前, 要求)
        return 前

    def 実行(self, 文脈: 能力文脈) -> 能力結果:
        try:
            request = self._要求(文脈)
            前 = self._継続元(文脈, request)
        except (TypeError, ValueError, AttributeError, KeyError):
            return 能力結果(False, "", 保留理由="知識取得設定不正")
        取得器 = 制御を配送する(self.取得器, 文脈.取得制御, "取得器")
        if 前 is not None:
            結果 = 取得器.継続(request, 前, 外部読取許可=self.外部読取許可,
                            停止要求=文脈.停止要求)
            if 結果.成立:
                try:
                    継続進展を検査(前, 結果, request)
                except (TypeError, ValueError, KeyError, AttributeError):
                    return 能力結果(False, "", 保留理由="取得継続の進展不一致")
            return 結果
        return 取得器.実行(request, 外部読取許可=self.外部読取許可,
                          停止要求=文脈.停止要求)

    def 登録(self) -> 登録能力:
        return 登録能力(self, 外部読取=True)
