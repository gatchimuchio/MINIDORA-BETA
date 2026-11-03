from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Protocol

from .HDS選択実行系 import HDS選択実行結果
from .HDS中間表現 import HDSIR
from .HDS参照 import HDS参照予算選択
from .HDS観測計画 import HDS参照観測要求群
from .hds能力経路_v2 import HDS参照検索V2
from .hds統合判断主体 import HDS作用種別, MINIDORA認知世界, MINIDORAHDS判断主体
from .hds適応候補調停 import HDS適応候補提案実行
from .参照 import 参照記録


class HDS駆動実行環境(Protocol):
    参照供給器: Any
    K3能力核: Any

    def コンパイル(self, 問合せ: str) -> HDSIR: ...


HDS参照実行関数 = Callable[[HDSIR], tuple[参照記録, ...]]
HDS評価実行関数 = Callable[[HDSIR, tuple[参照記録, ...]], HDS選択実行結果]


@dataclass(frozen=True, slots=True)
class HDS駆動選択結果:
    状態: str
    値: str | None
    理由: tuple[str, ...]
    参照: tuple[参照記録, ...]
    選択: HDS選択実行結果
    認知世界: MINIDORA認知世界


def _保留選択(理由: tuple[str, ...]) -> HDS選択実行結果:
    reasons = tuple(理由) or ("HDS_JUDGEMENT_SUSPEND",)
    return HDS選択実行結果("SUSPEND", None, None, reasons, None, 0, 0, 0, 0, 0, 0)


def _既定参照実行(環境: HDS駆動実行環境, ir: HDSIR) -> tuple[参照記録, ...]:
    if 環境.参照供給器 is None:
        return ()
    予算 = HDS参照予算選択(ir)
    return HDS参照検索V2(
        環境.参照供給器,
        ir,
        上限=予算.取得上限,
        一問合せ上限=予算.一問合せ上限,
        最大問合せ並列=予算.最大問合せ並列,
        観測要求=HDS参照観測要求群(ir),
    )


def _既定評価実行(環境: HDS駆動実行環境, ir: HDSIR, references: tuple[参照記録, ...]) -> HDS選択実行結果:
    return HDS適応候補提案実行(
        ir,
        references,
        コンパイル=環境.コンパイル,
        基礎能力核=環境.K3能力核,
        模型核=getattr(環境, "能力模型核", None),
    )


def HDS駆動選択実行(
    環境: HDS駆動実行環境,
    ir: HDSIR,
    *,
    参照必須: bool = False,
    委任目的: str = "入力に対して根拠を捏造せず、MINIDORAの一回の言語判断を局所閉包する",
    最大作用数: int = 6,
    参照実行: HDS参照実行関数 | None = None,
    評価実行: HDS評価実行関数 | None = None,
) -> HDS駆動選択結果:
    """J_hdsだけがCOMMIT/SUSPENDし、R/C workerは観測とPROPOSEまでに限定する。"""

    参照_fn = 参照実行 or (lambda payload: _既定参照実行(環境, payload))
    evaluate_fn = 評価実行 or (lambda payload, refs: _既定評価実行(環境, payload, refs))
    参照_available = 参照実行 is not None or 環境.参照供給器 is not None

    主体 = MINIDORAHDS判断主体()
    world = 主体.開始(
        ir,
        委任目的=委任目的,
        参照利用可能=参照_available,
        参照必須=bool(参照必須 or ir.参照必須),
        作用予算=最大作用数,
    )
    references: tuple[参照記録, ...] = ()
    selected: HDS選択実行結果 | None = None

    while True:
        request = 主体.次作用(world)
        if request.作用 == HDS作用種別.参照観測:
            references = tuple(参照_fn(ir))
            world = 主体.参照帰還(
                world,
                参照数=len(references),
                理由=('HDS_参照_OBSERVED', f"REFERENCE_COUNT:{len(references)}"),
            )
            continue
        if request.作用 == HDS作用種別.候補計算:
            selected = evaluate_fn(ir, references)
            world = 主体.評価帰還(world, selected)
            continue
        if request.作用 == HDS作用種別.確定:
            if selected is None or selected.状態 != "PROPOSE":
                raise RuntimeError("HDS COMMIT要求にPROPOSE状態の候補が存在しない")
            world = 主体.確定(world)
            reasons = tuple(dict.fromkeys(tuple(selected.理由) + request.理由 + ('HDS_JUDGEMENT_主体_COMMIT',)))
            return HDS駆動選択結果("APPROVE", selected.回答内容, reasons, references, selected, world)
        if request.作用 == HDS作用種別.留保:
            world = 主体.留保(world, request.理由)
            if selected is None:
                selected = _保留選択(request.理由)
            reasons = tuple(dict.fromkeys(tuple(selected.理由) + request.理由 + ('HDS_JUDGEMENT_主体_SUSPEND',)))
            return HDS駆動選択結果("SUSPEND", None, reasons, references, selected, world)
        if request.作用 == HDS作用種別.停止:
            if selected is None:
                selected = _保留選択(request.理由)
            return HDS駆動選択結果("SUSPEND", None, tuple(request.理由), references, selected, world)
        raise RuntimeError(f"未知のHDS作用要求: {request.作用}")


__all__ = ["HDS駆動実行環境", "HDS参照実行関数", "HDS評価実行関数", "HDS駆動選択結果", "HDS駆動選択実行"]
