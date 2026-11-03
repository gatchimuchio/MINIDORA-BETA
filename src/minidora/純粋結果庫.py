"""純粋結果の再利用庫。統合能力.pyから契約を変えずに分離した。"""
from collections import OrderedDict
from copy import deepcopy
from threading import RLock
from .能力合成 import _符号化, _結果辞書

class 純粋結果庫:
    """セッション内LRU。上限は保存するJSONバイト数で、OSメモリ上限ではない。"""
    def __init__(self, *, 有効=True, 最大件数=64, 最大バイト数=4000000):
        if type(有効) is not bool:
            raise ValueError("再利用指定はbool")
        for value, maximum in ((最大件数, 256), (最大バイト数, 16000000)):
            if type(value) is not int or not 1 <= value <= maximum:
                raise ValueError("再利用上限不正")
        self.有効 = 有効
        self._限度 = (最大件数, 最大バイト数)
        self._値 = OrderedDict()
        self._サイズ = 0
        self._統計 = {}
        self._ロック = RLock()

    def 計数(self, 名前, 項目):
        with self._ロック:
            row = self._統計.setdefault(名前, {"要求": 0, "再利用": 0, "原実行": 0})
            row[項目] += 1

    def 取得(self, key):
        with self._ロック:
            if not self.有効 or key not in self._値:
                return None
            value, size = self._値.pop(key)
            self._値[key] = (value, size)
            return deepcopy(value)

    def 保存(self, key, value):
        raw = _符号化(_結果辞書(value))
        size = len(raw) + len(key.encode())
        with self._ロック:
            if not self.有効 or not value.成立 or size > self._限度[1]:
                return
            if key in self._値:
                self._サイズ -= self._値.pop(key)[1]
            while self._値 and (len(self._値) >= self._限度[0] or self._サイズ+size > self._限度[1]):
                self._サイズ -= self._値.popitem(last=False)[1][1]
            self._値[key] = (deepcopy(value), size)
            self._サイズ += size

    def 統計(self):
        with self._ロック:
            return {"能力別": deepcopy(self._統計), "件数": len(self._値), "保存バイト数": self._サイズ}

    def 消去(self):
        with self._ロック:
            self._値.clear()
            self._サイズ = 0
            self._統計.clear()


