"""入力取得・既存意味変換・射影の一回性を、明示的な入力版単位で管理する。"""
from __future__ import annotations
from dataclasses import dataclass
from threading import RLock
from .契約 import 入力政策, 入力出所, 入力束
from .取得 import 原本を取得, 文脈を取得
from .変換 import 既存構文化器で変換
from .射影 import 入力束へ射影
from .封緘 import 封緘する


class 入力停止(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class 入力受付記録:
    案件ID: str
    入力ID: str
    版: int
    要求署名: str
    状態: str
    理由: str = ''


class HDS入力系:
    """構文化の一回性は明示された案件・入力ID・版に対する契約。文字列キャッシュではない。

    学習・最終採否・駆動状態を所有しない。失敗・解放済み受付も墓標を保持するため、
    容量超過時は新しい入力セッションを呼出側が明示的に作る。暗黙evictionで再構文化しない。
    """
    def __init__(self, コンパイラ=None, *, 契約版='MINIDORA-入力系-v1', 政策=None, 停止要求=None):
        from ..コア.値 import 文字
        文字(契約版)
        self.契約版=契約版
        self.政策=政策 or 入力政策()
        if not isinstance(self.政策,入力政策): raise TypeError('入力政策型が必要')
        if 停止要求 is not None and not callable(停止要求): raise TypeError('停止要求は関数')
        self._コンパイラ=コンパイラ
        self._停止要求=停止要求
        self._受付={}; self._束={}; self._保護=RLock(); self._変換回数=0; self._保持バイト=0; self._占有={}

    @classmethod
    def 標準(cls, **引数):
        # 欠落時に別パーサを新造しない。現行の完全ソース上ではそのまま既存実装を用いる。
        from ..HDS構文化器_v1 import 公開HDSコンパイラ
        return cls(公開HDSコンパイラ(),**引数)

    @property
    def 変換回数(self): return self._変換回数

    @property
    def 保持バイト数(self): return self._保持バイト

    def 受付記録(self):
        with self._保護: return tuple(self._受付[k] for k in sorted(self._受付))

    def _停止確認(self):
        if self._停止要求 is not None and self._停止要求(): raise 入力停止('入力処理の明示停止')

    def 受理(self, 本文, *, 案件ID, 入力ID, 版=1, 出所=入力出所.指示, 選択肢=(),
             由来='利用者提供', 取得時点='未指定', 前回結果=None, HDS履歴=(), 文脈=None,
             カーネル正本=None, 入力正本=None):
        self._停止確認()
        原本=原本を取得(本文,案件ID=案件ID,入力ID=入力ID,版=版,出所=出所,選択肢=選択肢,
                    由来=由来,取得時点=取得時点,政策=self.政策)
        文脈封緘=文脈を取得(前回結果=前回結果,HDS履歴=HDS履歴,文脈=文脈,政策=self.政策)
        # 注入データも先に隔離する。呼出後の外部可変辞書の変更を正本へ反映しない。
        既成=封緘する((カーネル正本,入力正本),最大要素=self.政策.最大封緘要素,最大バイト=self.政策.最大封緘バイト)
        要求署名=封緘する((原本.署名,文脈封緘.署名,既成.署名,self.契約版)).署名
        鍵=原本.鍵
        with self._保護:
            古=self._受付.get(鍵)
            if 古 is not None:
                if 古.要求署名!=要求署名: raise ValueError('同一入力版の原文・出所・文脈・正本は変更できない')
                if 古.状態=='完了': return self._束[鍵]
                raise ValueError('同一入力版は'+古.状態+'。再試行は新しい入力版で明示する')
            if len(self._受付)>=self.政策.最大受付件数: raise ValueError('入力受付容量上限。既受付を自動廃棄しない')
            self._受付[鍵]=入力受付記録(*鍵,要求署名,'処理中')
            try:
                self._停止確認()
                核,コア=既成.復元()
                if 核 is None and コア is None: self._変換回数+=1
                内容,_,_=既存構文化器で変換(原本,文脈封緘,self._コンパイラ,カーネル正本=核,入力正本=コア)
                self._停止確認()
                束=入力束へ射影(原本,文脈封緘,内容,契約版=self.契約版,政策=self.政策)
                self._停止確認()
                占有=束.内容.バイト数+束.文脈正本.バイト数+len(束.原本.本文.encode('utf-8'))+sum(len(x.encode('utf-8')) for x in 束.原本.選択肢)
                if self._保持バイト+占有>self.政策.最大保持バイト:
                    raise ValueError('入力保持容量上限。既受理内容を捨てず新規受理を停止する')
                self._保持バイト+=占有;self._占有[鍵]=占有
                self._束[鍵]=束
                self._受付[鍵]=入力受付記録(*鍵,要求署名,'完了')
                return 束
            except Exception as 例外:
                self._受付[鍵]=入力受付記録(*鍵,要求署名,'停止' if isinstance(例外,入力停止) else '失敗',type(例外).__name__+': '+str(例外))
                raise

    def 解放(self, 案件ID, 入力ID, 版):
        """大きい内容だけ解放。一回性記録を消さない。駆動系の学習状態には触れない。"""
        鍵=(案件ID,入力ID,版)
        with self._保護:
            旧=self._受付.get(鍵)
            if 旧 is None: raise KeyError(鍵)
            if 旧.状態=='処理中': raise ValueError('処理中の入力は解放できない')
            self._束.pop(鍵,None)
            self._保持バイト-=self._占有.pop(鍵,0)
            self._受付[鍵]=入力受付記録(*鍵,旧.要求署名,'解放済み',旧.理由)


class HDS入力コンパイラ:
    """旧呼出側との互換入口。意味演算の実装は元のコンパイラだけが持つ。

    旧APIの独立呼出は独立した受付になる。一回の意味決定から複数成果を読む場合は
    HDS入力系.受理で得た入力束を使用する。文字列だけによる無断共有はしない。
    """
    def __init__(self, コンパイラ, *, 政策=None):
        if コンパイラ is None: raise ValueError('既存コンパイラが必要')
        self._元=コンパイラ; self._政策=政策 or 入力政策()

    def __getattr__(self, 名前):
        # 詳細監査・失敗帰還等の既存APIを隠さない。ここは信頼されたPythonホストの互換面。
        if 名前.startswith('_'): raise AttributeError(名前)
        return getattr(self._元,名前)

    def _一回(self, 本文, 選択肢=(), **引数):
        return HDS入力系(self._元,政策=self._政策).受理(本文,案件ID='互換呼出',入力ID='入力',選択肢=選択肢,
            出所=入力出所.指示 if 選択肢 else 入力出所.互換,**引数)

    def コンパイル束(self, 入力, **引数):
        束=self._一回(入力,**引数)
        if 束.方式!='カーネル': raise TypeError('コンパイル束はカーネルを返す必要がある')
        return 束.カーネル正本

    def 問題コンパイル束(self, question, choices): return self._一回(question,choices).カーネル正本
    def コア入力コンパイル(self, 入力, **引数): return self._一回(入力,**引数).コア入力
    def 意味コンパイル(self, 入力, **引数): return self._一回(入力,**引数).読む('意味IR')
    def 観測要求コンパイル(self, 入力, **引数): return self._一回(入力,**引数).読む('参照観測要求')
    def 作用差分コンパイル(self, 入力, **引数): return self._一回(入力,**引数).読む('作用差分構造')
    def コンパイル(self, 入力, **引数): return self.コンパイル束(入力,**引数).互換IR()
    def 計算降下(self, 束): return self._元.計算降下(束)
    def 計算コンパイル(self, 入力, **引数): return self.計算降下(self.コンパイル束(入力,**引数))
    def 問題IR(self, question, choices): return self.問題コンパイル束(question,choices).意味IR
    def 問題コア入力(self, question, choices): return self.問題コンパイル束(question,choices).コア入力
    def 問題観測要求(self, question, choices): return self.問題コンパイル束(question,choices).参照観測要求
