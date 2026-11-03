"""数量の構文化・評価・コード・回答を既存HDS工程として登録する。"""
from __future__ import annotations
from copy import deepcopy
from ..能力合成 import 登録能力, _参照結合
from ..能力結果復元 import 能力結果を復元
from ..製品版.型 import 能力結果
from ..製品版.能力契約 import 能力文脈
from .数量構造 import 数量を構成, 数量を評価
from .数量生成 import 数量コード仕様, 数量コード実行, 数量を照合, 数量を説明, 数量回答を検査
from .値 import 指紋

数量接続版 = "HDS数量接続-v1"
_個数 = {"数量文構文化":None, "数量式評価":1, "数量コード仕様":1,
          "数量コード実行":3, "数量結果照合":3, "数量回答構成":4, "数量回答再表現":1}


class 数量能力:
    版 = 数量接続版
    優先度 = 0

    def __init__(self, 名前):
        if 名前 not in _個数: raise ValueError("数量作用名が未対応")
        self.名前 = 名前

    def _入力(self, 文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError("数量作用に構造化入力が必要")
        群 = 文脈.補助.get("合成入力")
        設定 = 文脈.補助.get("合成設定", {})
        if type(群) not in (list, tuple) or not 1 <= len(群) <= 16 or type(設定) is not dict:
            raise ValueError("数量作用の入力数・設定が不正")
        if _個数[self.名前] is not None and len(群) != _個数[self.名前]:
            raise ValueError("数量作用の入力数不一致")
        値 = tuple(能力結果を復元(行["結果"]) for 行 in 群)
        if any(not 行.成立 for 行 in 値): raise ValueError("数量作用の上流不成立")
        if self.名前 not in ("数量文構文化", "数量回答再表現") and 設定:
            raise ValueError("数量作用に未知の設定")
        return 群, 値, 設定

    def 判定(self, 文脈):
        try:
            self._入力(文脈)
            return 1.0
        except (ValueError, TypeError, KeyError):
            return 0.0

    def 実行(self, 文脈):
        try:
            群, 値, 設定 = self._入力(文脈)
            参照 = _参照結合(参照 for 項 in 値 for 参照 in 項.参照)
            if self.名前 == "数量文構文化":
                資料 = {}
                for 行, 項 in zip(群, 値):
                    名前 = 行["参照"]["識別子"]
                    if 名前 in 資料: raise ValueError("数量素材が重複")
                    資料[名前] = 項.本文
                構造 = 数量を構成(資料, 設定)
                return 能力結果(True,"数量の依存構造を構成しました。", 参照=参照, データ=構造)
            if self.名前 == "数量式評価":
                成果 = 数量を評価(値[0].データ)
            elif self.名前 == "数量コード仕様":
                成果 = 数量コード仕様(値[0].データ)
            elif self.名前 == "数量コード実行":
                if 値[2].データ != 数量を評価(値[0].データ):
                    raise ValueError("コード実行の前提計算が不一致")
                成果 = 数量コード実行(値[0].データ, 値[1].本文)
            elif self.名前 == "数量結果照合":
                成果 = 数量を照合(*(項.データ for 項 in 値))
            elif self.名前 == "数量回答構成":
                本文, 成果 = 数量を説明(*(項.データ for 項 in 値))
                return 能力結果(True, 本文, 参照=参照, データ=成果)
            else:
                旧 = 値[0]
                if not 数量回答を検査(旧):
                    raise ValueError("数量再表現の起点不一致")
                成果 = 旧.データ
                表示 = {**成果["表示"], **設定}
                本文, 成果 = 数量を説明(成果["構造"],成果["計算"],成果["検算"],成果["照合"],表示=表示)
                return 能力結果(True, 本文, 参照=参照, データ=成果)
            return 能力結果(True,self.名前 + "が成立しました。",参照=参照,データ=成果)
        except (ValueError, KeyError, TypeError, RecursionError, ZeroDivisionError, OverflowError) as 例外:
            return 能力結果(False,"", 保留理由="数量処理不成立:"+str(例外),
                            データ={"段階":self.名前,"例外":type(例外).__name__,"理由":str(例外)})

    def 登録(self):
        return 登録能力(self)


def 数量能力群():
    return tuple(数量能力(名前).登録() for 名前 in _個数)
