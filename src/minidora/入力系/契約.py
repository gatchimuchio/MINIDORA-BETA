"""入力系の原本・出所・文脈・束。確定という構文状態を世界事実の承認へ変換しない。"""
from __future__ import annotations
from dataclasses import dataclass
from enum import StrEnum
from hashlib import sha256
from ..コア.値 import 文字, 整数
from .封緘 import 封緘値, 封緘する


class 入力出所(StrEnum):
    指示 = '利用者指示'
    資料 = '外部資料'
    観測 = '提供観測'
    自生成 = '自生成内容'
    互換 = '出所未指定' 


@dataclass(frozen=True, slots=True)
class 入力原本:
    案件ID: str
    入力ID: str
    本文: str
    出所: 入力出所 = 入力出所.指示
    版: int = 1
    選択肢: tuple[str, ...] = ()
    由来: str = '利用者提供'
    取得時点: str = '未指定'
    元符号化: str = 'Unicode'

    def __post_init__(self):
        for n in ('案件ID','入力ID','本文','由来','取得時点','元符号化'): 文字(getattr(self,n),n)
        整数(self.版,'入力版',1)
        if not isinstance(self.出所,入力出所): raise TypeError('入力出所型が必要')
        if not isinstance(self.選択肢,tuple) or self.選択肢 and not 2<=len(self.選択肢)<=26:
            raise ValueError('選択肢は空または2..26件のtuple')
        for x in self.選択肢: 文字(x,'選択肢')
        if self.選択肢 and self.出所!=入力出所.指示: raise ValueError('資料を選択問題の指示へ昇格できない')

    @property
    def 鍵(self): return (self.案件ID,self.入力ID,self.版)

    @property
    def 署名(self): return 封緘する(self).署名


@dataclass(frozen=True, slots=True)
class 入力政策:
    最大入力バイト: int = 1000000
    最大選択総バイト: int = 1000000
    最大封緘バイト: int = 16000000
    最大封緘要素: int = 200000
    最大受付件数: int = 512
    最大保持バイト: int = 64_000_000

    def __post_init__(self):
        for n in ('最大入力バイト','最大選択総バイト','最大封緘バイト','最大封緘要素','最大受付件数','最大保持バイト'):
            整数(getattr(self,n),n,1,256_000_000 if 'バイト' in n else 1_000_000)


@dataclass(frozen=True, slots=True)
class 入力診断:
    種別: str
    理由: str
    影響参照: tuple[str, ...] = ()
    def __post_init__(self):
        文字(self.種別); 文字(self.理由)
        if not isinstance(self.影響参照,tuple) or any(not isinstance(x,str) for x in self.影響参照):
            raise TypeError('診断参照は文字列tuple')


@dataclass(frozen=True, slots=True)
class 入力束:
    原本: 入力原本
    契約版: str
    文脈署名: str
    内容: 封緘値
    方式: str
    診断: tuple[入力診断,...] = ()
    文脈正本: 封緘値 | None = None
    操作履歴: tuple[str,...] = ('取得','変換','射影')

    def __post_init__(self):
        if not isinstance(self.原本,入力原本) or not isinstance(self.内容,封緘値): raise TypeError('原本と封緘内容が必要')
        文字(self.契約版); 文字(self.文脈署名)
        if not isinstance(self.文脈正本,封緘値) or self.文脈正本.署名!=self.文脈署名:
            raise ValueError('文脈正本と署名の対応が必要')
        if self.方式 not in ('カーネル','コア入力'): raise ValueError('未知の入力方式')
        if not isinstance(self.診断,tuple) or any(not isinstance(x,入力診断) for x in self.診断): raise TypeError('診断tupleが必要')
        if self.操作履歴!=('取得','変換','射影'): raise ValueError('入力操作履歴不整合')

    @property
    def 全域署名(self):
        return 封緘する((self.原本.署名,self.契約版,self.文脈署名,self.内容.署名,self.方式,self.診断,self.操作履歴)).署名

    @property
    def カーネル正本(self): return self.内容.復元() if self.方式=='カーネル' else None

    @property
    def コア入力(self):
        return self.内容.欄を復元('コア入力') if self.方式=='カーネル' else self.内容.復元()

    @property
    def 目的候補(self):
        return self.コア入力.目的 if self.原本.出所==入力出所.指示 else ()

    @property
    def 指示として使用可能(self): return self.原本.出所==入力出所.指示

    @property
    def 残差(self): return self.コア入力.残差

    def 文脈を読む(self):
        return self.文脈正本.復元()

    def 読む(self, 名前: str):
        """同じ意味決定から用途別の成果を読む。再コンパイルしない。"""
        if 名前=='コア入力': return self.コア入力
        if self.方式!='カーネル': raise ValueError('旧コア入力方式には要求されたカーネル成果がない')
        許可=('意味IR','計算計画','参照観測要求','候補意味IR','候補検証契約','数量計算契約',
              '失敗署名候補','チェックリスト','認知世界差分','監査参照候補','作用差分構造','版')
        if 名前 not in 許可: raise ValueError('未定義の入力射影先: '+str(名前))
        return self.内容.欄を復元(名前)
