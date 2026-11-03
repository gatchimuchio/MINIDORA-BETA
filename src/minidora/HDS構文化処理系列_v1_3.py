from __future__ import annotations

from dataclasses import dataclass, replace
from typing import Any

from .HDS中間表現 import HDSIR, HDS実行核
from .命令 import 手順
from .命令計算降下 import 命令計算降下
from .計算中間表現 import 計算中間表現
from .言語 import 言語計画


HDSコンパイラパイプライン版 = "v1.3"


class HDS意味専用計画器:
    '基礎意味フロントエンドから計算計画責任を外すための無作用計画器。\n\n    基礎構文化器は歴史的理由で ``計画()`` 呼出を持つため、その呼出契約だけを満たす。\n    実計算Pは別の計算意図形成器が生成する。\n    '

    def 計画(self, 問合せ: str) -> 言語計画:
        return 言語計画(
            手順("HDS意味専用", ()),
            {},
            False,
            "意味",
        )


@dataclass(frozen=True, slots=True)
class HDSコンパイル束:
    '意味IRと計算計画を同一フィールドへ混ぜず並列保持する構文化器成果。'

    意味IR: HDSIR
    計算計画: 言語計画
    版: str = HDSコンパイラパイプライン版

    def 互換IR(self) -> HDSIR:
        """旧HDSIR.手順利用者だけに計算Pを再付与する最外周互換橋。"""

        plan = self.計算計画
        return replace(
            self.意味IR,
            実行核=HDS実行核(
                plan.種別,
                (),
                "結果",
                境界=("HDS-IR", "日本語基底", "COMPATIBILITY_BRIDGE"),
                検証=('公開構文化器', "意味IRと計算Pを正本では分離"),
            ),
            初期状態=dict(plan.初期状態),
            参照必須=bool(plan.参照必須),
            種別=plan.種別,
            閉包状態="作用閉包",
            手順=plan.手順,
        )


@dataclass(frozen=True, slots=True)
class HDS計算コンパイル成果:
    意味IR: HDSIR
    計算IR: 計算中間表現
    初期状態: dict[str, Any]
    参照必須: bool
    種別: str
    版: str = HDSコンパイラパイプライン版



def HDS意味IR化(base: HDSIR, plan: 言語計画) -> HDSIR:
    """互換基礎IRから計算Pと実行状態を外し、意味正本へ閉じる。"""

    return replace(
        base,
        実行核=HDS実行核(
            plan.種別,
            (),
            "結果",
            境界=('HDS-意味', "日本語基底"),
            検証=("計算P非内包",),
        ),
        初期状態={},
        参照必須=bool(plan.参照必須),
        種別=plan.種別,
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        手順=None,
    )


class HDS計算降下バックエンド:
    """形成済み計算Pを計算中間表現へ降下する。自然言語は再解析しない。"""

    版 = HDSコンパイラパイプライン版

    def 降下(self, bundle: HDSコンパイル束) -> HDS計算コンパイル成果:
        plan = bundle.計算計画
        compute_ir = 命令計算降下(plan.手順)
        refs: list[str] = [bundle.意味IR.認知世界ID]
        refs.extend(item.座標ID for item in bundle.意味IR.座標[:16])
        compute_ir = replace(
            compute_ir,
            名称=plan.種別 or compute_ir.名称,
            由来=f"HDS意味IR:{bundle.意味IR.認知世界ID}",
            由来参照=tuple(dict.fromkeys(refs)),
            境界=("HDS意味IRと計算P分離",),
            検証=("自然言語再解析なし", "計算実行境界v1"),
        )
        return HDS計算コンパイル成果(
            意味IR=bundle.意味IR,
            計算IR=compute_ir,
            初期状態=dict(plan.初期状態),
            参照必須=bool(plan.参照必須),
            種別=plan.種別,
        )


__all__ = [
    "HDSコンパイラパイプライン版",
    "HDS意味専用計画器",
    "HDSコンパイル束",
    "HDS計算コンパイル成果",
    "HDS意味IR化",
    "HDS計算降下バックエンド",
]
