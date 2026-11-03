'局所会話状態→実HDS→要求計画→既存能力→次状態を接続する。\n\n標準チャット入口の置換ではなく、文書操作能力用の同期セッション。\n'
from __future__ import annotations

from collections.abc import Callable, Mapping
from copy import deepcopy
from dataclasses import dataclass
from threading import Lock

from .HDS適合器 import HDSコンパイラProtocol
from .HDS中間表現 import HDSIR
from .要求解釈 import 要求計画器, 要求解釈結果
from .要求解釈実行 import 要求計画を実行, 要求実行結果
from .能力合成 import 能力合成器
from .能力合成_局所接続 import 局所能力群
from .文脈照応 import 会話参照記憶, 会話参照スナップショット
from .製品版.型 import 能力結果


@dataclass(frozen=True, slots=True)
class 文脈要求計画:
    起点: 会話参照スナップショット
    解釈: 要求解釈結果


@dataclass(frozen=True, slots=True)
class 文脈付き応答:
    状態: str
    出力: tuple[tuple[str, 能力結果], ...]
    理由: tuple[str, ...]
    起点識別子: str
    更新後識別子: str
    解釈: 要求解釈結果 | None = None
    実行: 要求実行結果 | None = None

    @property
    def 成立(self) -> bool:
        return self.状態 == "合格"


class _準備不成立(ValueError):
    pass


class 文脈付き要求セッション:
    def __init__(self, セッションID: str, *, コンパイラ: HDSコンパイラProtocol | None = None,
                 最大応答数: int = 64, 最大記録バイト数: int = 2_000_000):
        if コンパイラ is None:
            from .HDS構文化器 import 公開HDSコンパイラ
            コンパイラ = 公開HDSコンパイラ()
        self._コンパイラ = コンパイラ
        self._文脈 = 会話参照記憶(セッションID, 最大応答数=最大応答数,
                                最大記録バイト数=最大記録バイト数)
        self._計画器 = 要求計画器()
        # この版では副作用のない既存3能力だけを登録する。
        self._合成器 = 能力合成器(局所能力群())
        self._ロック = Lock()
        self._最大応答数 = 最大応答数

    def 起点(self) -> 会話参照スナップショット:
        return self._文脈.起点()

    def 初期化(self) -> 会話参照スナップショット:
        if not self._ロック.acquire(blocking=False):
            raise ValueError("処理中のセッションは初期化できない")
        try:
            return self._文脈.初期化()
        finally:
            self._ロック.release()

    def _準備(self, 依頼: str, 資料: Mapping[str, 能力結果] | None) -> 文脈要求計画:
        if type(依頼) is not str:
            raise _準備不成立("依頼は文字列")
        if not 0 < len(依頼) <= 8192:
            raise _準備不成立("依頼文字数範囲外")
        if 資料 is not None and not isinstance(資料, Mapping):
            raise _準備不成立("資料は名前付きの写像")
        s = self.起点()
        if s.局所起点.版 >= self._最大応答数:
            raise _準備不成立("会話応答数上限:明示初期化が必要")
        ir = self._コンパイラ.コンパイル(依頼, 文脈=s.HDS文脈へ(),
                                         HDS履歴=deepcopy(s.局所起点.IR履歴))
        if not isinstance(ir, HDSIR) or ir.原文 != 依頼:
            raise _準備不成立('構文化器出力と依頼原文が不一致')
        r = self._計画器.コンパイル(ir, {} if 資料 is None else 資料, 文脈=s)
        return 文脈要求計画(s, r)

    def 準備(self, 依頼: str, 資料: Mapping[str, 能力結果] | None = None) -> 文脈要求計画:
        """状態を更新せず計画を作る。実行時に同じ所有者・世代・状態版を要求する。"""
        if not self._ロック.acquire(blocking=False):
            raise ValueError("同じセッションの処理中")
        try:
            return self._準備(依頼, 資料)
        finally:
            self._ロック.release()

    def _実行(self, 計画: 文脈要求計画, 停止要求) -> 文脈付き応答:
        if (not isinstance(計画, 文脈要求計画)
                or not isinstance(計画.解釈, 要求解釈結果)
                or not 計画.解釈.整合確認()
                or not self._文脈.現行確認(計画.起点)
                or 計画.解釈.文脈識別子 != 計画.起点.識別子):
            return self._失敗("古い計画・別セッション・改変された計画は実行しない")
        固定 = deepcopy(計画)
        if 固定.起点.局所起点.版 >= self._最大応答数:
            return self._失敗("会話応答数上限:明示初期化が必要")
        ir = 固定.解釈.HDS保持
        if ir is None:
            return self._失敗("更新に利用できるHDS原文がない")
        r = 要求計画を実行(固定.解釈, self._合成器, 文脈起点=固定.起点, 停止要求=停止要求)
        try:
            後 = self._文脈.更新(固定.起点, ir.原文, r.状態, 出力=r.出力 if r.成立 else (),
                                  ir=ir, 実行ハッシュ=r.合成.ルートハッシュ if r.合成 else "")
        except (TypeError, ValueError, RecursionError) as exc:
            return 文脈付き応答("失敗", (), (f"文脈確定失敗:{type(exc).__name__}:{exc}",),
                                固定.起点.識別子, self.起点().識別子, 固定.解釈, r)
        return 文脈付き応答(r.状態, deepcopy(r.出力), r.理由, 固定.起点.識別子,
                            後.識別子, 固定.解釈, r)

    @staticmethod
    def _失敗(理由: str) -> 文脈付き応答:
        return 文脈付き応答("失敗", (), (理由,), "", "")

    def 実行(self, 計画: 文脈要求計画, *, 停止要求: Callable[[], bool] | None = None) -> 文脈付き応答:
        if not self._ロック.acquire(blocking=False):
            return 文脈付き応答("保留", (), ("同じセッションの処理中",), "", "")
        try:
            return self._実行(計画, 停止要求)
        finally:
            self._ロック.release()

    def 応答(self, 依頼: str, 資料: Mapping[str, 能力結果] | None = None, *,
             停止要求: Callable[[], bool] | None = None) -> 文脈付き応答:
        if not self._ロック.acquire(blocking=False):
            return 文脈付き応答("保留", (), ("同じセッションの処理中",), "", "")
        try:
            return self._実行(self._準備(依頼, 資料), 停止要求)
        except _準備不成立 as exc:
            return self._失敗(str(exc))
        except Exception as exc:
            # Compilerや呼出者側の故障は状態・焦点を更新しない。例外本文は出力しない。
            return self._失敗(f"文脈要求処理失敗:{type(exc).__name__}")
        finally:
            self._ロック.release()
