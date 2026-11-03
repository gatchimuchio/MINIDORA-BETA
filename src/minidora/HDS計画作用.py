from __future__ import annotations

from hashlib import sha256
from typing import Callable, Sequence

from .HDS実行主体 import HDS作用機会, HDS作用結果, HDS作用状態, HDS実行状態
from .能力合成 import 能力合成器, 合成結果
from .製品版.能力契約 import 能力文脈


class HDS目的計画作用:
    """既存の目的計画器をHDSの計画作用として使う適合器。

    計画成功は実行成功・最終採用ではない。計画と資料をHDS成果へ帰還し、次作用へ渡す。
    同一性は要求・禁止作用・計画器契約だけで決め、無関係なHDS状態変化で再計画しない。
    計画器objectのreprは入力署名へ使わない。契約差がある計画器は版または明示契約署名で区別する。
    """

    def __init__(
        self,
        計画器,
        要求,
        *,
        入力状態: Sequence[str] = (),
        出力状態: str = "目的計画済み",
        解消対象: Sequence[str] = ("計画未形成",),
        禁止作用: Sequence[str] = (),
        作用ID: str = "目的計画",
        計画器契約署名: str | None = None,
        資源負荷: int = 1,
    ) -> None:
        if not callable(getattr(計画器, "計画する", None)):
            raise TypeError("HDS目的計画作用には計画する()を持つ計画器が必要")
        if not isinstance(作用ID, str) or not 作用ID.strip():
            raise ValueError("目的計画作用IDは空にできない")
        if 計画器契約署名 is not None and (
            not isinstance(計画器契約署名, str) or not 計画器契約署名.strip()
        ):
            raise ValueError("計画器契約署名は空でない文字列またはNoneである必要がある")
        self.計画器 = 計画器
        self.要求 = 要求
        self.入力状態 = frozenset(str(x) for x in 入力状態)
        self.出力状態 = str(出力状態)
        self.解消対象 = frozenset(str(x) for x in 解消対象)
        self.禁止作用 = tuple(str(x) for x in 禁止作用)
        self.資源負荷 = max(0, int(資源負荷))
        self.作用ID = 作用ID.strip()
        版 = str(getattr(計画器, "版", "") or getattr(計画器, "契約版", "")).strip()
        self.計画器契約署名 = (
            計画器契約署名.strip()
            if 計画器契約署名 is not None
            else f"{type(計画器).__module__}.{type(計画器).__qualname__}:{版}"
        )
        材料 = repr((
            self.作用ID,
            self.要求,
            self.禁止作用,
            self.計画器契約署名,
        )).encode("utf-8")
        self._入力署名 = sha256(材料).hexdigest()

    def 機会(self, 状態: HDS実行状態) -> HDS作用機会 | None:
        if not self.入力状態.issubset(状態.成立状態):
            return None
        if self.出力状態 in 状態.成立状態:
            return None
        return HDS作用機会(
            self.作用ID,
            self._入力署名,
            self.入力状態,
            frozenset({self.出力状態}),
            self.解消対象,
            self.資源負荷,
            0.0,
            True,
            ("PURPOSE_PLANNER_AS_HDS_ACTION",),
        )

    def 実行(self, 状態: HDS実行状態) -> HDS作用結果:
        try:
            結果 = self.計画器.計画する(self.要求, 禁止作用=self.禁止作用)
        except TypeError:
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({"計画器契約不一致"}),
                理由=("PURPOSE_PLANNER_CONTRACT_MISMATCH",),
            )
        except Exception as exc:
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({f"計画失敗:{type(exc).__name__}"}),
                理由=("PURPOSE_PLANNER_EXCEPTION", type(exc).__name__),
            )

        if bool(getattr(結果, "成立", False)) and getattr(結果, "計画", None) is not None:
            return HDS作用結果(
                HDS作用状態.成立,
                追加状態=frozenset({self.出力状態}),
                解消残差=self.解消対象,
                成果=(("目的計画結果", 結果),),
                理由=("PURPOSE_PLAN_FORMED",),
            )

        理由 = str(getattr(結果, "理由", "") or "計画未閉包")
        return HDS作用結果(
            HDS作用状態.保留,
            追加残差=frozenset({f"計画未閉包:{理由}"}),
            成果=(("目的計画結果", 結果),),
            理由=("PURPOSE_PLAN_SUSPENDED", 理由),
        )


class HDS能力合成作用:
    """計画済みの既存能力合成をHDSの実行作用として使う。

    統合セッションの会話採用状態は更新しない。能力合成結果をHDS成果へ帰還し、
    HDSが別の検証作用を経てCOMMITする余地を維持する。作用入力署名は実際に消費する
    計画・資料・能力文脈・外部読取許可だけから作る。
    """

    def __init__(
        self,
        合成器: 能力合成器,
        *,
        計画成果名: str = "目的計画結果",
        入力状態: Sequence[str] = ("目的計画済み",),
        出力状態: str = "能力合成済み",
        解消対象: Sequence[str] = ("実行未完了",),
        文脈: 能力文脈 | None = None,
        文脈生成: Callable[[HDS実行状態], 能力文脈] | None = None,
        外部読取許可: bool = False,
        停止要求=None,
        作用ID: str = "能力合成",
        資源負荷: int = 2,
    ) -> None:
        if not isinstance(合成器, 能力合成器):
            raise TypeError("HDS能力合成作用には能力合成器が必要")
        if 文脈 is not None and 文脈生成 is not None:
            raise ValueError("固定文脈と文脈生成は同時指定できない")
        if 文脈 is not None and not isinstance(文脈, 能力文脈):
            raise TypeError("固定文脈は能力文脈である必要がある")
        if not isinstance(作用ID, str) or not 作用ID.strip():
            raise ValueError("能力合成作用IDは空にできない")
        self.合成器 = 合成器
        self.計画成果名 = str(計画成果名)
        self.入力状態 = frozenset(str(x) for x in 入力状態)
        self.出力状態 = str(出力状態)
        self.解消対象 = frozenset(str(x) for x in 解消対象)
        self.文脈 = 文脈
        self.文脈生成 = 文脈生成
        self.外部読取許可 = bool(外部読取許可)
        self.停止要求 = 停止要求
        self.資源負荷 = max(0, int(資源負荷))
        self.作用ID = 作用ID.strip()

    def _文脈(self, 状態: HDS実行状態) -> 能力文脈:
        if self.文脈生成 is not None:
            文脈値 = self.文脈生成(状態)
            if not isinstance(文脈値, 能力文脈):
                raise TypeError("HDS能力合成作用の文脈生成は能力文脈を返す必要がある")
            return 文脈値
        if self.文脈 is not None:
            return self.文脈
        目的文 = " / ".join(状態.目的)
        return 能力文脈(目的文, "HDS実行主体")

    def _入力署名(self, 計画成果, 文脈値: 能力文脈) -> str:
        材料 = repr((
            self.作用ID,
            計画成果,
            文脈値,
            self.外部読取許可,
            type(self.合成器).__module__,
            type(self.合成器).__qualname__,
        )).encode("utf-8")
        return sha256(材料).hexdigest()

    def 機会(self, 状態: HDS実行状態) -> HDS作用機会 | None:
        if not self.入力状態.issubset(状態.成立状態):
            return None
        計画成果 = 状態.成果辞書().get(self.計画成果名)
        if 計画成果 is None or not bool(getattr(計画成果, "成立", False)):
            return None
        if self.出力状態 in 状態.成立状態:
            return None
        try:
            文脈値 = self._文脈(状態)
        except Exception:
            return None
        return HDS作用機会(
            self.作用ID,
            self._入力署名(計画成果, 文脈値),
            self.入力状態,
            frozenset({self.出力状態}),
            self.解消対象,
            self.資源負荷,
            0.0,
            True,
            ("CAPABILITY_COMPOSER_AS_HDS_ACTION",),
        )

    def 実行(self, 状態: HDS実行状態) -> HDS作用結果:
        計画成果 = 状態.成果辞書().get(self.計画成果名)
        計画 = getattr(計画成果, "計画", None)
        資料 = getattr(計画成果, "資料", None)
        if 計画 is None or not isinstance(資料, dict):
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({"能力合成入力欠落"}),
                理由=("CAPABILITY_COMPOSITION_INPUT_MISSING",),
            )
        try:
            文脈値 = self._文脈(状態)
            結果 = self.合成器.実行(
                計画,
                資料,
                文脈=文脈値,
                外部読取許可=self.外部読取許可,
                停止要求=self.停止要求,
            )
        except Exception as exc:
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({f"能力合成例外:{type(exc).__name__}"}),
                理由=("CAPABILITY_COMPOSITION_EXCEPTION", type(exc).__name__),
            )

        if not isinstance(結果, 合成結果):
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({"能力合成結果型不正"}),
                理由=("CAPABILITY_COMPOSITION_RETURN_TYPE_INVALID",),
            )

        成果群 = [("能力合成結果", 結果)]
        成果群.extend((f"合成出力:{名前}", 値) for 名前, 値 in 結果.出力)
        if 結果.成立:
            return HDS作用結果(
                HDS作用状態.成立,
                追加状態=frozenset({self.出力状態}),
                解消残差=self.解消対象,
                成果=tuple(成果群),
                理由=("CAPABILITY_COMPOSITION_SUCCEEDED",),
            )

        残差 = f"能力合成未閉包:{結果.状態}:{結果.理由}"
        作用状態 = HDS作用状態.失敗 if 結果.状態 == "失敗" else HDS作用状態.保留
        return HDS作用結果(
            作用状態,
            追加残差=frozenset({残差}),
            成果=tuple(成果群),
            理由=("CAPABILITY_COMPOSITION_NOT_CLOSED", 結果.状態, 結果.理由),
        )


__all__ = ["HDS目的計画作用", "HDS能力合成作用"]
