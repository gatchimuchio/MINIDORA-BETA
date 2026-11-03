"""出力の一回性・改訂・失効・有限送達を所有する。駆動の状態には触れない。"""
from __future__ import annotations
from dataclasses import replace
from threading import RLock
from ..共通契約.出力 import (出力束, 出力政策, 表現結果, 表現器, 送達器,
    送達要求, 送達記録, 出力返却)
from ..共通契約.封緘 import 封緘する
from .取得 import 内容を取得
from .変換 import 表現を変換
from .射影 import 外部へ射影, 関数返却先


class HDS出力系:
    def __init__(self, *, 政策: 出力政策 | None = None, 表現器群=(), 停止要求=None):
        self.政策 = 政策 if 政策 is not None else 出力政策()
        if not isinstance(self.政策, 出力政策):
            raise TypeError('出力政策型が必要')
        self.政策.__post_init__()
        if 停止要求 is not None and not callable(停止要求):
            raise TypeError('停止要求は呼出可能である必要がある')
        self.停止要求 = 停止要求
        self._器 = {}
        for 器 in tuple(表現器群):
            if not isinstance(器, 表現器) or 器.ID == '標準' or (器.ID, 器.版) in self._器:
                raise ValueError('表現器型・予約ID・重複を確認する')
            self._器[(器.ID, 器.版)] = 器
        self._施錠 = RLock()
        self._受付 = {}  # 署名 -> (出力束, 表現結果|None, 保持バイト)
        self._内容 = {}  # 案件・出力・判断版 -> 内容署名
        self._最新 = {}  # 案件・出力 -> 判断版
        self._失効 = {}  # 案件・出力 -> 失効した最大判断版
        self._送達 = {}
        self._送達器 = {}
        self._保持バイト = 0
        self._送信中 = set()
        self._不明履歴 = set()
        self._変換数 = 0

    def _停止(self):
        if self.停止要求 is None:
            return False
        値 = self.停止要求()
        if type(値) is not bool:
            raise TypeError('停止要求はboolを返す必要がある')
        return 値

    def _失効中(self, 束):
        鍵 = 束.鍵[:2]
        return 束.判断版 < self._最新.get(鍵, 束.判断版) or 束.判断版 <= self._失効.get(鍵, 0)

    def 表現(self, 束: 出力束) -> 表現結果:
        with self._施錠:
            取得 = 内容を取得(束, self.政策)
            識別 = 束.署名
            if 識別 in self._受付:
                _, 既存, _ = self._受付[識別]
                if 既存 is None:
                    raise ValueError('出力は処理中または解放済み。同じ版で無断再生成しない')
                return 既存
            if 束.鍵 in self._内容 and self._内容[束.鍵] != 束.内容署名:
                raise ValueError('同じ判断版の内容を差し替えられない')
            if len(self._受付) >= self.政策.最大受付件数:
                raise ValueError('出力受付上限。駆動を再実行しない')
            if self._失効中(束):
                raise ValueError('古いまたは失効した判断版')
            # 結果の最大バイトも予約し、変換後に切り捨てない。
            予約 = 取得.保持バイト + 束.表現.最大バイト
            if self._保持バイト + 予約 > self.政策.最大保持バイト:
                raise ValueError('出力保持容量不足。別形式または資源契約を明示する')
            self._内容[束.鍵] = 束.内容署名
            self._最新[束.鍵[:2]] = 束.判断版
            self._受付[識別] = (束, None, 予約)
            self._保持バイト += 予約
            try:
                if self._停止():
                    結果 = 表現結果(識別, 束.内容署名, 束.表現.署名, '', 'text/plain; charset=utf-8', False, ('出力変換前の停止',))
                else:
                    self._変換数 += 1
                    結果 = 表現を変換(取得, self._器.get((束.表現.表現器ID, 束.表現.表現器版)))
                    if self._停止():
                        結果 = 表現結果(識別, 束.内容署名, 束.表現.署名, '', 'text/plain; charset=utf-8', False, ('出力変換後の停止',))
            except Exception as 例外:
                結果 = 表現結果(識別, 束.内容署名, 束.表現.署名, '', 'text/plain; charset=utf-8', False,
                    ('出力段階境界の失敗:' + type(例外).__name__,))
            保持 = 取得.保持バイト + len(結果.バイト列)
            self._保持バイト += 保持 - 予約
            self._受付[識別] = (束, 結果, 保持)
            return 結果

    def 送達(self, 表現: 表現結果, 器: 送達器 = 関数返却先, *, 再試行: bool = False) -> 送達記録:
        if not isinstance(表現, 表現結果) or not isinstance(器, 送達器) or type(再試行) is not bool:
            raise TypeError('表現結果・送達器・再試行指定が必要')
        with self._施錠:
            受付 = self._受付.get(表現.出力署名)
            if 受付 is None or 受付[1] != 表現:
                raise ValueError('この出力系で受理・検証した表現だけ送達できる')
            束 = 受付[0]
            識別 = 封緘する((表現.署名, 器.ID, 器.版)).署名
            既存 = self._送達.get(識別)
            if 識別 in self._送信中:
                raise ValueError('同じ出力を再入して二重送信できない')
            # 同じ住所・版を別実装へ黙って差し替えない。
            宛先 = (器.ID, 器.版)
            if 宛先 in self._送達器 and self._送達器[宛先] is not 器:
                raise ValueError('同じ送達先・版への別実装の混入')
            if 識別 not in self._送達 and len(self._送達) >= self.政策.最大送達件数:
                raise ValueError('送達受付上限')
            self._送達器[宛先] = 器
            if self._失効中(束):
                return 送達記録(識別, 表現.署名, 器.ID, '失効', 既存.試行回数 if 既存 else 0,
                    ('旧受領記録は保持するが、現在の返却成立へ転用しない',))
            if 既存 and 既存.返却成立:
                return 既存
            回数 = 既存.試行回数 if 既存 else 0
            def 記録(状態, 理由):
                out = 送達記録(識別, 表現.署名, 器.ID, 状態, 回数, (理由,))
                if 識別 not in self._送達 and len(self._送達) >= self.政策.最大送達件数:
                    raise ValueError('送達受付上限')
                self._送達[識別] = out
                return out
            if not 表現.合格:
                return 記録('表現不成立', '不成立の表現は外部へ送らない')
            if self._失効中(束):
                return 記録('失効', '改訂または明示失効により送達を停止')
            if self._停止():
                return 記録('停止', '送達前の停止')
            if 既存 and not 再試行:
                return 既存
            if 識別 in self._不明履歴 and not 器.重複排除対応:
                raise ValueError('送達結果不明。重複排除契約なしに再試行しない')
            if 回数 >= self.政策.最大送達試行:
                return 記録('試行上限', '消費済み試行回数は再送でリセットしない')
            if 識別 not in self._送達 and len(self._送達) >= self.政策.最大送達件数:
                raise ValueError('送達受付上限')
            要求 = 送達要求(識別, 束.案件ID, 束.出力ID, 束.判断版,
                表現.署名, 表現.本文署名, 表現.バイト列, 表現.媒体型)
            # 副作用より先に試行消費を記録。中断も結果不明として保持する。
            self._送達[識別] = 送達記録(識別, 表現.署名, 器.ID, '結果不明', 回数 + 1, ('送達実行中',))
            self._送信中.add(識別)
            self._不明履歴.add(識別)
            try:
                out = 外部へ射影(要求, 器, 回数 + 1)
                self._送達[識別] = out
                if out.状態 != '結果不明':
                    self._不明履歴.discard(識別)
                return out
            finally:
                self._送信中.discard(識別)

    def 返却(self, 束: 出力束, 器: 送達器 = 関数返却先, *, 再試行=False) -> 出力返却:
        表現 = self.表現(束)
        記録 = self.送達(表現, 器, 再試行=再試行)
        return 出力返却(表現, 記録, 束.状態 == '成立')

    def 失効させる(self, 案件ID: str, 出力ID: str, 判断版: int):
        from ..コア.値 import 文字, 整数
        文字(案件ID); 文字(出力ID); 整数(判断版, '失効版', 1)
        with self._施錠:
            鍵 = (案件ID, 出力ID)
            if 鍵 not in self._最新:
                raise KeyError('未登録出力は失効できない')
            self._失効[鍵] = max(self._失効.get(鍵, 0), 判断版)

    def 解放(self, 表現: 表現結果):
        with self._施錠:
            if any(x.表現署名 == 表現.署名 and k in self._送信中 for k, x in self._送達.items()):
                raise ValueError('送達中の内容は解放しない')
            受付 = self._受付.get(表現.出力署名)
            if 受付 is None or 受付[1] != 表現:
                raise ValueError('登録済み表現だけ解放できる')
            self._保持バイト -= 受付[2]
            # 内容と表現を解放し、同じ版の再生成禁止キーだけを残す。
            self._受付[表現.出力署名] = (None, None, 0)

    @property
    def 計装(self):
        with self._施錠:
            return {'表現実行数': self._変換数, '受付件数': len(self._受付),
                '保持バイト': self._保持バイト, '送達受付数': len(self._送達),
                '送達試行数': sum(x.試行回数 for x in self._送達.values()),
                '送達成立数': sum(x.返却成立 for x in self._送達.values())}
