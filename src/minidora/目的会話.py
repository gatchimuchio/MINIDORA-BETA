"""実HDS→目的IR→作用契約の経路探索→既存統合セッションを閉じる追加入口。"""
from __future__ import annotations
from dataclasses import dataclass
from threading import Lock
from .統合実行 import 統合セッション, 統合応答
from .能力意味カタログ import 能力意味カタログ
from .目的計画 import 目的計画器, 目的計画結果
from .HDS目的射影 import HDSから目的要求, 目的射影結果
from .HDS構文化器 import 公開HDSコンパイラ
from .要求境界契約 import 被覆台帳印, 要求境界契約印

@dataclass(frozen=True, slots=True)
class 目的会話応答:
    状態: str
    本文: str
    理由: str = ''
    射影: 目的射影結果 | None = None
    計画: 目的計画結果 | None = None
    実行: 統合応答 | None = None
    要求境界契約印: str = ''

    @property
    def 成立(self):
        return self.状態 == '合格' and self.実行 is not None and self.実行.成立

    def 辞書化(self):
        return {'状態': self.状態, '本文': self.本文, '理由': self.理由,
                'HDS原文': self.射影.HDS保持.原文 if self.射影 and self.射影.HDS保持 else None,
                '局所解消': self.射影.局所解消 if self.射影 else (),
                '計画経路': self.計画.作用経路 if self.計画 else (),
                '要求境界契約印': self.要求境界契約印,
                '実行': self.実行.辞書化() if self.実行 else None}


class 目的会話セッション:
    """標準チャットを置換しない追加入口。実行器・採用状態は既存の一つを共有する。"""
    def __init__(self, セッションID: str, *, 統合: 統合セッション | None = None):
        self.統合 = 統合 if 統合 is not None else 統合セッション(セッションID)
        if type(self.統合) is not 統合セッション or self.統合.起点().セッションID != セッションID:
            raise ValueError('統合セッションの所有が不一致')
        self.カタログ = 能力意味カタログ(self.統合.能力一覧())
        self._計画器 = 目的計画器(self.カタログ)
        self._ロック = Lock()

    def 応答(self, 原文: str, 資料=None, *, 停止要求=None) -> 目的会話応答:
        if not self._ロック.acquire(blocking=False):
            return 目的会話応答('保留', '', '同じ目的会話の処理中')
        射影 = plan = None
        try:
            start, history = self.統合.採用履歴スナップショット()
            current = 能力意味カタログ(self.統合.能力一覧())
            if current.ハッシュ != self.カタログ.ハッシュ:
                raise ValueError('意味カタログの変更')
            self.統合._停止(停止要求)
            if type(原文) is not str or not 原文.strip() or len(原文) > 8192:
                raise ValueError('依頼文の範囲外')
            ir = 公開HDSコンパイラ().コンパイル(原文)
            prior = history[-1]['出力'] if history else ()
            射影 = HDSから目的要求(ir, {} if 資料 is None else 資料, 前回成果=prior)
            if not 射影.成立:
                return 目的会話応答('保留', '', 射影.理由, 射影)
            self.統合._停止(停止要求)
            plan = self._計画器.計画する(射影.要求)
            if not plan.成立:
                return 目的会話応答('保留', '', plan.理由, 射影, plan)
            coverage_seal = 被覆台帳印(plan.要求被覆)
            prepared = self.統合.準備(plan.計画, plan.資料, 依頼文=原文)
            if prepared.起点 != start:
                return 目的会話応答('保留', '', '解釈後に会話状態が変化', 射影, plan)
            境界_seal = 要求境界契約印(
                原文=原文, 計画印=prepared.ハッシュ, 要求被覆印=coverage_seal)
            結果 = self.統合.実行(prepared, 停止要求=停止要求)
            return 目的会話応答(結果.状態, 結果.本文, 結果.理由,
                            射影, plan, 結果, 境界_seal)
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError, InterruptedError) as exc:
            状態 = '中止' if isinstance(exc, InterruptedError) else '保留'
            return 目的会話応答(状態, '', str(exc), 射影, plan)
        finally:
            self._ロック.release()
