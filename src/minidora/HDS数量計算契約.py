from __future__ import annotations

from dataclasses import dataclass
import re
from typing import Mapping

from .HDS中間表現 import HDSIR, 値状態
from .言語 import 言語計画
from .HDS観測計画 import HDS参照観測要求


_阻害状態 = frozenset({値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保})
_明示関係種別 = frozenset({
    "等価", "不同", "比較.大", "比較.小", "比較.以上", "比較.以下",
})
_数量候補表層 = re.compile(
    r"^\s*[-+]?(?:(?:\d+(?:\.\d+)?|\.\d+)(?:[eE][-+]?\d+)?|"
    r"(?:\d+(?:\.\d+)?|\.\d+)\s*(?:[x×*]\s*)?10\s*\^\s*\{?\s*[-+]?\d+\s*\}?)"
    r"(?:\s*/\s*\d+(?:\.\d+)?)?\s*(?:[%A-Za-zµμΩ°][A-Za-z0-9µμΩ°/%^+\-]*)?\s*$"
)


@dataclass(frozen=True, slots=True)
class HDS数量値:
    所属: str
    座標ID: str
    値: str
    単位: tuple[str, ...] = ()
    原文範囲: tuple[int, int] | None = None


@dataclass(frozen=True, slots=True)
class HDS明示数量関係:
    関係ID: str
    種別: str
    始点: tuple[str, ...]
    終点: tuple[str, ...]
    条件: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS数量計算契約:
    問い数量: tuple[HDS数量値, ...] = ()
    候補数量: tuple[HDS数量値, ...] = ()
    明示関係: tuple[HDS明示数量関係, ...] = ()
    数量問い関係ID: tuple[str, ...] = ()
    計算P実行可能: bool = False
    状態: str = "非数量"
    不足: tuple[str, ...] = ()

    @property
    def 数量問題(self) -> bool:
        return self.状態 != "非数量"

    @property
    def 候補被覆ラベル(self) -> tuple[str, ...]:
        return tuple(dict.fromkeys(
            x.所属.split(":", 1)[1]
            for x in self.候補数量
            if x.所属.startswith("候補:")
        ))


def _数量値群(ir: HDSIR, 所属: str) -> tuple[HDS数量値, ...]:
    coords = ir.座標辞書()
    単位索引: dict[str, list[str]] = {}
    for 関係 in ir.関係:
        if 関係.種別 != "数量単位" or 関係.値状態 in _阻害状態:
            continue
        for value_id in 関係.始点:
            if value_id not in coords:
                continue
            for unit_id in 関係.終点:
                unit = coords.get(unit_id)
                if unit is None or unit.値状態 in _阻害状態:
                    continue
                text = " ".join(str(unit.内容).split()).strip()
                if text and text not in 単位索引.setdefault(value_id, []):
                    単位索引[value_id].append(text)

    out: list[HDS数量値] = []
    for coord in ir.座標:
        if str(coord.種別) != "値.数量" or coord.値状態 in _阻害状態:
            continue
        value = " ".join(str(coord.内容).split()).strip()
        if not value:
            continue
        out.append(HDS数量値(
            所属=所属,
            座標ID=str(coord.座標ID),
            値=value,
            単位=tuple(単位索引.get(str(coord.座標ID), ())),
            原文範囲=coord.原文範囲,
        ))
    return tuple(out)


def _明示関係群(ir: HDSIR) -> tuple[HDS明示数量関係, ...]:
    coords = ir.座標辞書()
    out: list[HDS明示数量関係] = []
    for 関係 in ir.関係:
        if str(関係.種別) not in _明示関係種別 or 関係.値状態 in _阻害状態:
            continue
        starts = tuple(
            " ".join(str(coords[x].内容).split()).strip()
            for x in 関係.始点 if x in coords and coords[x].値状態 not in _阻害状態
        )
        ends = tuple(
            " ".join(str(coords[x].内容).split()).strip()
            for x in 関係.終点 if x in coords and coords[x].値状態 not in _阻害状態
        )
        if not starts or not ends:
            continue
        out.append(HDS明示数量関係(
            関係ID=str(関係.関係ID),
            種別=str(関係.種別),
            始点=starts,
            終点=ends,
            条件=tuple(str(x) for x in 関係.条件),
        ))
    return tuple(out)



def _候補が数量値主体(ir: HDSIR) -> bool:
    表層 = " ".join(str(ir.原文 or ir.正規化文).split()).strip()
    if 表層 and _数量候補表層.fullmatch(表層):
        return True
    数量あり = any(
        str(x.種別) == "値.数量" and x.値状態 not in _阻害状態
        for x in ir.座標
    )
    if not 数量あり:
        return False
    return not any(
        str(x.種別).startswith(("対象.", "実体.", "状態.", "関係."))
        and x.値状態 not in _阻害状態
        and str(x.内容).strip()
        for x in ir.座標
    )


def HDS数量法則観測要求群(
    問いIR: HDSIR,
    契約: HDS数量計算契約,
) -> tuple[HDS参照観測要求, ...]:
    """法則不足の数量問題だけ、問題文そのものを法則観測へ降下する。

    候補値・専門式・分野知識をquery側で推測しない。問題文はCompiler入力の保持表層として
    利用し、下流Rで再解釈しない。
    """

    if 契約.状態 != "法則不足":
        return ()
    surface = " ".join(str(問いIR.原文).split()).strip()
    if not surface:
        return ()
    known = tuple(dict.fromkeys(
        tuple(x.値 for x in 契約.問い数量)
        + tuple(unit for x in 契約.問い数量 for unit in x.単位)
    ))
    return (HDS参照観測要求(
        ID="数量法則:0",
        関係ID=None,
        関係種別="数量計算法則",
        未知位置=None,
        既知端点=known,
        条件範囲=(),
        候補ラベル=None,
        候補表層=None,
        外部言語=str(問いIR.入力言語 or "ja"),
        外部検索表層=surface,
        必須被覆=False,
        外部文脈アンカー=(surface,),
        段階="fallback",
        優先度=5,
        provenance=("数量計算契約", "計算法則不足"),
    ),)


def HDS数量計算契約を形成(
    問いIR: HDSIR,
    候補意味IR: Mapping[str, HDSIR] | None,
    計算計画: 言語計画,
) -> HDS数量計算契約:
    """形成済みKernel意味から数量・計算可能性だけを固定する。

    専門法則や式を推測しない。計算Pが既に閉じている場合だけ実行可能とし、
    数量はあるが明示関係も計算Pも無い場合は「法則不足」として残す。
    """

    問い数量 = _数量値群(問いIR, "問い")
    候補数量: list[HDS数量値] = []
    for label, ir in sorted((候補意味IR or {}).items()):
        候補数量.extend(_数量値群(ir, "候補:" + str(label)))

    明示関係 = _明示関係群(問いIR)
    数量問い関係 = tuple(
        str(x.関係ID)
        for x in 問いIR.関係
        if str(x.種別) == "数量同定" and x.値状態 not in _阻害状態
    )
    実行可能 = bool(
        not bool(getattr(計算計画, "参照必須", True))
        and tuple(getattr(getattr(計算計画, "手順", None), "命令列", ()))
    )

    候補辞書 = dict(候補意味IR or {})
    数量あり = bool(問い数量 or 候補数量 or 明示関係 or 数量問い関係)
    if not 数量あり:
        return HDS数量計算契約()
    数量回答対象 = bool(候補辞書) and all(_候補が数量値主体(ir) for ir in 候補辞書.values())
    数量推論要求 = bool(数量問い関係 or (問い数量 and 数量回答対象))
    if 実行可能:
        状態, 不足 = "実行可能", ()
    elif 明示関係:
        状態, 不足 = "明示関係あり・計画未閉包", ("計算P未閉包",)
    elif 数量推論要求:
        状態, 不足 = "法則不足", ("計算法則未形成",)
    else:
        状態, 不足 = "数量含有", ()

    return HDS数量計算契約(
        問い数量=問い数量,
        候補数量=tuple(候補数量),
        明示関係=明示関係,
        数量問い関係ID=数量問い関係,
        計算P実行可能=実行可能,
        状態=状態,
        不足=不足,
    )


__all__ = [
    "HDS数量値",
    "HDS明示数量関係",
    "HDS数量計算契約",
    "HDS数量法則観測要求群",
    "HDS数量計算契約を形成",
]
