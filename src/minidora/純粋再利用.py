'明示的に純粋と確認された能力だけを再利用する。\n\n全入力・文脈・参照・版を鍵に含め、意味結果へ実行計測値を混入しない。\n外部作用・可変状態を持つ任意のモジュールを安全化する仕組みではない。\n'
from copy import deepcopy
from hashlib import sha256
from .能力合成 import _文脈辞書, _結果辞書, _符号化

class 純粋再利用能力:
    def __init__(self, モジュール, cache):
        self._本体, self._庫 = モジュール, cache
        self.名前, self.版 = モジュール.名前, モジュール.版
        self.優先度 = モジュール.優先度

    def _版確認(self):
        if (self._本体.名前, self._本体.版) != (self.名前, self.版):
            raise ValueError("登録後の能力変更")

    def 判定(self, 文脈):
        self._版確認()
        return self._本体.判定(文脈)

    def 実行(self, 文脈):
        self._版確認()
        # 全入力・文脈・設定・参照・版を含める。本文だけのキーで取り違えない。
        key = sha256(_符号化({"名前": self.名前, "版": self.版, "入力": _文脈辞書(文脈)})).hexdigest()
        self._庫.計数(self.名前, "要求")
        結果 = self._庫.取得(key)
        if 結果 is not None:
            self._庫.計数(self.名前, "再利用")
            return 結果
        self._庫.計数(self.名前, "原実行")
        結果 = self._本体.実行(deepcopy(文脈))
        self._版確認()
        _結果辞書(結果)
        self._庫.保存(key, 結果)
        return 結果
