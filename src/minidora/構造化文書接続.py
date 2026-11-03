'文書読取・文書操作を既存能力へ接続する。外部ファイルや命令は実行しない。'
from __future__ import annotations

from .構造化文書 import 構造化文書版, 構造化文書を読む
from .構造化文書操作 import 文書を処理
from .応答構成 import 能力結果を復元
from .能力合成 import 登録能力
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈


class 構造化文書モジュール:
    版 = 構造化文書版
    優先度 = 0

    def __init__(self, 操作: str):
        if 操作 not in ('文書読取', '文書操作'):
            raise ValueError('未対応の文書能力')
        self.名前 = 操作

    def _入力(self, 文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError('明示した文書入力が必要')
        rows, settings = 文脈.補助.get('合成入力', ()), 文脈.補助.get('合成設定', {})
        if type(rows) is not tuple or len(rows) != 1 or type(settings) is not dict:
            raise ValueError('単一文書と設定が必要')
        value = 能力結果を復元(rows[0]['結果'])
        if not value.成立:
            raise ValueError('上流不成立')
        if self.名前 == '文書読取':
            if '形式' not in settings or set(settings) - {'形式', '区切り', '見出し'}:
                raise ValueError('読取形式・設定を明示する')
        elif set(settings) != {'操作', '設定'}:
            raise ValueError('文書操作の設定項目不一致')
        return value, settings

    def 判定(self, 文脈):
        try:
            self._入力(文脈)
            return 1.0
        except (TypeError, ValueError, KeyError, AttributeError, RecursionError):
            return 0.0

    def 実行(self, 文脈):
        try:
            value, settings = self._入力(文脈)
            if self.名前 == '文書読取':
                return 構造化文書を読む(value.本文, **settings)
            return 文書を処理(value, **settings)
        except (TypeError, ValueError, KeyError, AttributeError, RecursionError):
            return 能力結果(False, '', 保留理由='文書能力入力不正')

    def 登録(self):
        return 登録能力(self)


def 構造化文書能力群() -> tuple[登録能力, ...]:
    return tuple(構造化文書モジュール(name).登録() for name in ('文書読取', '文書操作'))
