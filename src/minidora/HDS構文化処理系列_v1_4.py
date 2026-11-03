from __future__ import annotations

from dataclasses import dataclass, replace
from typing import Any

from .HDS構文化記録_v1_3 import HDS作用差分構造
from .HDS構文化記録_v1_1 import (
    HDS失敗署名候補, HDSチェックリスト項目, HDS認知世界差分, HDS監査参照候補,
)
from .HDS中間表現 import HDSIR, HDS実行核
from .HDSコア入力 import HDSコア入力束
from .HDS観測計画 import HDS参照観測要求
from .HDS候補検証契約 import HDS候補検証契約
from .HDS数量計算契約 import HDS数量計算契約
from .命令 import 手順
from .命令計算降下 import 命令計算降下
from .計算中間表現 import 計算中間表現
from .言語 import 言語計画


HDSコンパイラパイプライン版 = "v2.0"

_互換除外接頭辞 = ("監査.", "保持.", "暫定性.", "帰還.")


def _旧consumer互換意味射影(ir: HDSIR) -> HDSIR:
    """Kernel正本から旧consumerが扱える意味面だけを射影する。

    意味決定をやり直さず、監査・保持・帰還sidebandだけを除く。
    関係・残差・意味作用も残存座標へ閉じるため、旧consumerへ未処理監査座標を漏らさない。
    """
    def sideband(x) -> bool:
        cid = str(x.座標ID)
        origin = str(getattr(x, "由来", ""))
        return (
            str(x.種別).startswith(_互換除外接頭辞)
            or cid.startswith(("archv1:", "archv11:", "archv13:"))
            or origin.startswith("公開HDS 構文化器 構造 v1")
            or origin.startswith("公開HDS 構文化器 v1.1")
        )

    座標 = tuple(x for x in ir.座標 if not sideband(x))
    ids = {x.座標ID for x in 座標}
    関係 = tuple(
        r for r in ir.関係
        if all(ref in ids for ref in (*r.始点, *r.終点))
    )
    残差 = tuple(
        r for r in ir.残差
        if not r.影響座標 or any(ref in ids for ref in r.影響座標)
    )
    履歴 = tuple(
        op for op in ir.意味作用履歴
        if not op.出力参照 or any(ref in ids for ref in op.出力参照)
    )
    return replace(ir, 座標=座標, 関係=関係, 残差=残差, 意味作用履歴=履歴)


class HDS意味専用計画器:
    """基礎意味フロントエンドから計算計画責任を外すための無作用計画器。"""

    def 計画(self, 問合せ: str, *, 文脈参照: Any = None) -> 言語計画:
        return 言語計画(手順("HDS意味専用", ()), {}, False, "意味")


@dataclass(frozen=True, slots=True)
class HDSカーネル束:
    """MINIDORA内部で一度だけ形成し、全下流が共有するCompiler Kernel成果。

    原文の意味決定はこの束を形成する時点で閉じる。Core/R/計算/既存能力は、
    ここから必要な射影を読むだけで原文を再解釈しない。
    """

    意味IR: HDSIR
    計算計画: 言語計画
    コア入力: HDSコア入力束
    参照観測要求: tuple[HDS参照観測要求, ...] = ()
    候補意味IR: tuple[tuple[str, HDSIR], ...] = ()
    候補検証契約: tuple[HDS候補検証契約, ...] = ()
    数量計算契約: HDS数量計算契約 = HDS数量計算契約()
    失敗署名候補: tuple[HDS失敗署名候補, ...] = ()
    チェックリスト: tuple[HDSチェックリスト項目, ...] = ()
    認知世界差分: HDS認知世界差分 = HDS認知世界差分()
    監査参照候補: tuple[HDS監査参照候補, ...] = ()
    作用差分構造: HDS作用差分構造 = HDS作用差分構造()
    版: str = HDSコンパイラパイプライン版

    @property
    def 候補意味IR辞書(self) -> dict[str, HDSIR]:
        return {str(ラベル): ir for ラベル, ir in self.候補意味IR}

    @property
    def 正本(self) -> HDSコア入力束:
        """旧利用側互換。MINIDORA全体の正本はこの束自身で、これはCore射影だけを返す。"""
        return self.コア入力

    @property
    def カーネル正本(self) -> "HDSカーネル束":
        return self

    @property
    def カーネル署名(self) -> str:
        # Core入力の意味署名に、外部観測と計算降下の決定済み表現も結合する。
        from .コア.値 import 署名
        return 署名((
            self.コア入力.意味署名,
            tuple((x.ID, x.外部検索表層, x.段階, x.優先度) for x in self.参照観測要求),
            self.候補意味IR,
            self.候補検証契約,
            self.数量計算契約,
            self.計算計画,
            self.失敗署名候補,
            self.チェックリスト,
            self.認知世界差分,
            self.監査参照候補,
            self.作用差分構造,
            self.版,
        ))

    def 互換IR(self) -> HDSIR:
        """旧実行系向け射影。Kernel正本を変更しない。"""
        plan = self.計算計画
        互換 = _旧consumer互換意味射影(self.意味IR)
        return replace(
            互換,
            実行核=HDS実行核(
                plan.種別,
                (),
                "結果",
                境界=("HDS-IR", "日本語基底", "互換橋", "Kernel互換橋"),
                検証=("公開構文化器", "Compiler Kernelからの一方向射影"),
            ),
            初期状態=dict(plan.初期状態),
            参照必須=bool(plan.参照必須),
            種別=plan.種別,
            閉包状態="作用閉包",
            手順=plan.手順,
        )


# 公開互換名は維持するが、一般入力と選択入力は同じKernel契約を使う。
HDSコンパイル束 = HDSカーネル束
HDS選択コンパイル束 = HDSカーネル束


@dataclass(frozen=True, slots=True)
class HDS計算コンパイル成果:
    意味IR: HDSIR
    計算IR: 計算中間表現
    初期状態: dict[str, Any]
    参照必須: bool
    種別: str
    作用差分構造: HDS作用差分構造 = HDS作用差分構造()
    コア入力: HDSコア入力束 | None = None
    参照観測要求: tuple[HDS参照観測要求, ...] = ()
    版: str = HDSコンパイラパイプライン版


def HDS意味IR化(base: HDSIR, plan: 言語計画) -> HDSIR:
    """Legacy意味IRを計算Pから分離する。Kernel正本は別束で形成する。"""
    return replace(
        base,
        実行核=HDS実行核(
            plan.種別,
            (),
            "結果",
            境界=("HDS意味", "日本語基底", "Legacy意味IR"),
            検証=("計算P非内包", "Compiler Kernelへ統合"),
        ),
        初期状態={},
        参照必須=bool(plan.参照必須),
        種別=plan.種別,
        閉包状態="CLOSED_FOR_意味_TRANSFER",
        手順=None,
    )


class HDS計算降下バックエンド:
    """Kernelに形成済みの計算PをLegacy計算中間表現へ降下する。原文は再解析しない。"""

    版 = HDSコンパイラパイプライン版

    def 降下(self, bundle: HDSカーネル束) -> HDS計算コンパイル成果:
        plan = bundle.計算計画
        compute_ir = 命令計算降下(plan.手順)
        refs: list[str] = [bundle.意味IR.認知世界ID]
        refs.extend(item.座標ID for item in bundle.意味IR.座標[:16])
        compute_ir = replace(
            compute_ir,
            名称=plan.種別 or compute_ir.名称,
            由来=f"HDSKernel:{bundle.意味IR.認知世界ID}",
            由来参照=tuple(dict.fromkeys(refs)),
            境界=("Compiler Kernelから計算Pを降下", "作用差分構造は計算Pへ自動降下しない"),
            検証=("自然言語再解析なし", "計算実行境界v2"),
        )
        return HDS計算コンパイル成果(
            意味IR=bundle.意味IR,
            計算IR=compute_ir,
            初期状態=dict(plan.初期状態),
            参照必須=bool(plan.参照必須),
            種別=plan.種別,
            作用差分構造=bundle.作用差分構造,
            コア入力=bundle.コア入力,
            参照観測要求=bundle.参照観測要求,
        )


__all__ = [
    "HDSコンパイラパイプライン版",
    "HDS意味専用計画器",
    "HDSカーネル束",
    "HDSコンパイル束",
    "HDS選択コンパイル束",
    "HDS計算コンパイル成果",
    "HDS意味IR化",
    "HDS計算降下バックエンド",
]
