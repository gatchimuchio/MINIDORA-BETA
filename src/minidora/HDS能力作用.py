from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from typing import Callable, Sequence

from .HDS実行主体 import (
    HDS作用機会,
    HDS作用結果,
    HDS作用状態,
    HDS実行状態,
)
from .製品版.能力契約 import 能力モジュール, 能力文脈
from .採否 import 実行状態
from .統合駆動_v2.計画 import HDS作用仕様


@dataclass(frozen=True, slots=True)
class HDS能力作用設定:
    出力状態: frozenset[str]
    入力状態: frozenset[str] = frozenset()
    解消対象: frozenset[str] = frozenset()
    資源負荷: int = 1
    追加残差接頭辞: str = "能力未成立"
    最低判定: float = 0.01


class HDS能力モジュール作用:
    """既存能力モジュールをHDS実行主体から呼ぶ適合器。

    判定値は作用候補内の優先度情報としてのみ扱う。最終採用・回答正しさ・
    HDS最終判断には読み替えない。COMMITはHDS実行主体だけが行う。

    固定文脈だけでなく、現在のHDS実行状態から能力文脈を生成できる。作用入力署名は
    HDS全状態ではなく、実際に能力が受け取る能力文脈と能力版から作る。
    """

    def __init__(
        self,
        モジュール: 能力モジュール,
        文脈: 能力文脈 | None = None,
        *,
        文脈生成: Callable[[HDS実行状態], 能力文脈] | None = None,
        出力状態: Sequence[str] = (),
        入力状態: Sequence[str] = (),
        解消対象: Sequence[str] = (),
        資源負荷: int = 1,
        最低判定: float = 0.01,
    ) -> None:
        名前 = str(getattr(モジュール, "名前", "")).strip()
        版 = str(getattr(モジュール, "版", "")).strip()
        if not 名前 or not 版:
            raise ValueError("HDS能力作用には名前と版を持つ能力モジュールが必要")
        if 文脈 is not None and not isinstance(文脈, 能力文脈):
            raise TypeError("HDS能力作用の固定文脈は能力文脈である必要がある")
        if 文脈生成 is not None and not callable(文脈生成):
            raise TypeError("HDS能力作用の文脈生成はcallableである必要がある")
        if 文脈 is None and 文脈生成 is None:
            raise ValueError("HDS能力作用には固定文脈または文脈生成が必要")
        if 文脈 is not None and 文脈生成 is not None:
            raise ValueError("固定文脈と文脈生成は同時指定できない")
        閾値 = float(最低判定)
        if not 0.0 <= 閾値 <= 1.0:
            raise ValueError("HDS能力作用の最低判定は0..1である必要がある")
        self.モジュール = モジュール
        self.文脈 = 文脈
        self.文脈生成 = 文脈生成
        self.作用ID = f"能力:{名前}@{版}"
        self.設定 = HDS能力作用設定(
            frozenset(str(x) for x in (出力状態 or (f"能力:{名前}:成立",))),
            frozenset(str(x) for x in 入力状態),
            frozenset(str(x) for x in 解消対象),
            max(0, int(資源負荷)),
            "能力未成立",
            閾値,
        )
        # 能力適合器も汎用目的逆算へ参加できる静的作用契約を持つ。
        self.計画仕様 = HDS作用仕様(
            self.作用ID,
            self.設定.入力状態,
            self.設定.出力状態,
            解消残差=self.設定.解消対象,
            資源負荷=self.設定.資源負荷,
            版=str(getattr(self.モジュール, "版", "")),
        )

    def _文脈(self, 状態: HDS実行状態) -> 能力文脈:
        if self.文脈生成 is None:
            assert self.文脈 is not None
            return self.文脈
        文脈値 = self.文脈生成(状態)
        if not isinstance(文脈値, 能力文脈):
            raise TypeError("HDS能力作用の文脈生成は能力文脈を返す必要がある")
        return 文脈値

    def _判定値(self, 文脈: 能力文脈) -> float:
        try:
            return max(0.0, min(1.0, float(self.モジュール.判定(文脈))))
        except Exception:
            return 0.0

    def 機会(self, 状態: HDS実行状態) -> HDS作用機会 | None:
        if not self.設定.入力状態.issubset(状態.成立状態):
            return None
        try:
            文脈値 = self._文脈(状態)
        except Exception:
            return None
        判定値 = self._判定値(文脈値)
        if 判定値 < self.設定.最低判定:
            return None
        署名材料 = repr((
            self.作用ID,
            str(getattr(self.モジュール, "版", "")),
            文脈値,
            判定値,
        )).encode("utf-8")
        入力署名 = sha256(署名材料).hexdigest()
        return HDS作用機会(
            self.作用ID,
            入力署名,
            self.設定.入力状態,
            self.設定.出力状態,
            self.設定.解消対象,
            self.設定.資源負荷,
            判定値,
            True,
            (f"CAPABILITY_SCORE:{判定値:.12f}", "HDS_CAPABILITY_OPPORTUNITY"),
        )

    def 実行(self, 状態: HDS実行状態) -> HDS作用結果:
        try:
            文脈値 = self._文脈(状態)
            結果 = self.モジュール.実行(文脈値)
        except Exception as exc:
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({f"能力例外:{self.作用ID}:{type(exc).__name__}"}),
                理由=("CAPABILITY_EXCEPTION", type(exc).__name__),
            )

        実効状態 = 結果.状態
        成果名 = f"能力結果:{self.モジュール.名前}"
        if 結果.成立 is True and 実効状態 == 実行状態.合格:
            return HDS作用結果(
                HDS作用状態.成立,
                追加状態=self.設定.出力状態,
                解消残差=self.設定.解消対象,
                成果=((成果名, 結果),),
                理由=("CAPABILITY_SUCCEEDED",),
            )

        理由 = str(getattr(結果, "保留理由", "") or 実効状態.value)
        残差 = f"{self.設定.追加残差接頭辞}:{self.モジュール.名前}:{理由}"
        作用状態 = HDS作用状態.失敗 if 実効状態 == 実行状態.失敗 else HDS作用状態.保留
        return HDS作用結果(
            作用状態,
            追加残差=frozenset({残差}),
            成果=((成果名, 結果),),
            理由=("CAPABILITY_NOT_COMMITTED", 理由),
        )


__all__ = ["HDS能力作用設定", "HDS能力モジュール作用"]
