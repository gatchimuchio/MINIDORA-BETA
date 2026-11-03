from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, Mapping

from .HDS中間表現 import HDSIR, HDS関係, 値状態
from .意味字句 import 意味語

_BLOCKING = {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}
_関係メタ鍵群 = frozenset({
    "検索述語", "不足位置", "英日意味射影", "受動態", "選択意図", "選択問題閉包",
})
_汎用関係種別群 = frozenset({"問い適合", "命題適合", "説明適合"})


@dataclass(frozen=True, slots=True)
class HDS参照観測要求:
    """Compiler KernelからRへ渡す完成済み観測要求。

    意味関係と外部検索文脈を別フィールドで保持する。Rは原文・意味IRを再解釈せず、
    ``外部検索表層`` をproviderへ降下するだけでよい。
    """

    ID: str
    関係ID: str | None
    関係種別: str | None
    未知位置: str | None
    既知端点: tuple[str, ...]
    条件範囲: tuple[tuple[str, str], ...]
    候補ラベル: str | None
    候補表層: str | None
    外部言語: str
    外部検索表層: str
    必須被覆: bool
    外部文脈アンカー: tuple[str, ...] = ()
    段階: str = "primary"
    優先度: int = 50
    provenance: tuple[str, ...] = ()


def _unique(parts: Iterable[str]) -> tuple[str, ...]:
    out: list[str] = []
    seen: set[str] = set()
    for raw in parts:
        value = " ".join(str(raw).split()).strip()
        if not value:
            continue
        key = value.casefold()
        if key in seen:
            continue
        seen.add(key)
        out.append(value)
    return tuple(out)


def _意味集合(values: Iterable[str]) -> frozenset[str]:
    out: set[str] = set()
    for value in values:
        out.update(str(x).casefold() for x in 意味語(value) if str(x))
    return frozenset(out)


def _条件値(関係: HDS関係, key: str) -> str:
    prefix = key + "="
    for raw in 関係.条件:
        value = str(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _局所条件範囲(関係: HDS関係) -> tuple[tuple[str, str], ...]:
    out: list[tuple[str, str]] = []
    seen: set[tuple[str, str]] = set()
    for raw in 関係.条件:
        key, sep, payload = str(raw).partition("=")
        key, payload = key.strip(), payload.strip()
        if not sep or not key or not payload or key in _関係メタ鍵群:
            continue
        item = (key, payload)
        if item in seen:
            continue
        seen.add(item)
        out.append(item)
    return tuple(out)


def _選択肢(ir: HDSIR) -> tuple[tuple[str, str], ...]:
    out: list[tuple[str, str]] = []
    for coord in ir.座標:
        if not str(coord.座標ID).startswith("選択肢:"):
            continue
        label = str(coord.座標ID).split(":", 1)[1]
        value = " ".join(str(coord.内容).split()).strip()
        if label and value:
            out.append((label, value))
    return tuple(sorted(out))


def _検索表層(ir: HDSIR) -> tuple[str, ...]:
    return _unique(
        str(coord.内容)
        for coord in ir.座標
        if str(coord.種別).startswith("検索.")
        and coord.値状態 not in {値状態.矛盾, 値状態.留保}
    )


def _縮退対象(ir: HDSIR) -> tuple[str, ...]:
    return _unique(
        str(coord.内容)
        for coord in ir.座標
        if str(coord.種別).startswith(("対象.", "実体."))
        and coord.値状態 not in _BLOCKING
        and str(coord.内容).strip()
    )


def _汎用外部文脈(ir: HDSIR) -> tuple[str, ...]:
    """関係未閉包時にもKernelが保持する外部観測用の意味文脈。

    Rで原文を読み直さず、Compilerが既に構文化した対象・関係表層・状態・属性・値・条件だけを使う。
    制御・監査・選択肢・未知位置などの実行メタは混ぜない。
    """
    prefixes = ("対象.", "実体.", "関係.", "状態.", "属性.", "値.", "条件.", "時刻.", "時間.", "範囲.")
    return _unique(
        str(coord.内容)
        for coord in ir.座標
        if not str(coord.座標ID).startswith("選択肢:")
        and str(coord.種別).startswith(prefixes)
        and coord.値状態 not in _BLOCKING
        and str(coord.内容).strip()
    )


def _監査表層(ir: HDSIR) -> tuple[str, ...]:
    return _unique(
        str(coord.内容)
        for coord in ir.座標
        if str(coord.種別) == "監査.R_query"
        and coord.値状態 != 値状態.矛盾
    )


def _既知端点(ir: HDSIR, 関係: HDS関係, position: str) -> tuple[str, ...]:
    coords = ir.座標辞書()
    ids = 関係.終点 if position == "始点" else 関係.始点
    return _unique(
        str(coords[cid].内容)
        for cid in ids
        if cid in coords and coords[cid].値状態 not in _BLOCKING
    )


def _query_surface(
    *,
    predicate: str,
    position: str,
    known: tuple[str, ...],
    条件範囲: tuple[tuple[str, str], ...],
    候補: str | None,
) -> str:
    scoped = tuple(value for _key, value in 条件範囲)
    if 候補 is None:
        if position == "始点":
            return " ".join(_unique((predicate, *known, *scoped)))
        return " ".join(_unique((*known, predicate, *scoped)))
    if position == "始点":
        parts = (候補, predicate, *known, *scoped)
    else:
        parts = (*known, predicate, 候補, *scoped)
    return " ".join(_unique(parts))


def _関連検索文脈(
    search_surfaces: tuple[str, ...],
    *,
    predicate: str,
    known: tuple[str, ...],
    条件範囲: tuple[tuple[str, str], ...],
    最大: int = 2,
) -> tuple[str, ...]:
    """Compilerが既に抽出した外部検索表層から、関係観測の文脈anchorを選ぶ。
    意味分類をRでやり直さないため、ここでCompiler Kernel内で選ぶ。

    """
    if not search_surfaces:
        return ()
    needles = _unique((predicate, *known, *(value for _key, value in 条件範囲)))
    scored: list[tuple[int, int, str]] = []
    for index, surface in enumerate(search_surfaces):
        low = surface.casefold()
        score = sum(1 for needle in needles if needle.casefold() in low)
        scored.append((score, -index, surface))
    scored.sort(reverse=True)
    related = [surface for score, _order, surface in scored if score > 0]
    if not related:
        related = list(search_surfaces)
    return tuple(related[:max(1, int(最大))])


def _外部表層(local_surface: str, 外部文脈: tuple[str, ...]) -> str:
    # 文脈anchorと局所意味を両方保持する。重複した完全表層だけを除き、語句の削除はしない。
    return " ".join(_unique((*外部文脈, local_surface)))


def HDS参照観測要求群(ir: HDSIR) -> tuple[HDS参照観測要求, ...]:
    """Kernel意味正本を再解析せず、外部観測へ降下可能な要求群を構成する。"""

    choices = _選択肢(ir)
    search_surfaces = _検索表層(ir)
    audit_surfaces = _監査表層(ir)
    縮退対象群 = _縮退対象(ir)
    外部言語名 = str(ir.入力言語 or "ja")
    requests: list[HDS参照観測要求] = []
    concrete = 0
    計画済み関係数 = 0
    関係署名群: set[tuple[object, ...]] = set()
    正本関係群: list[tuple[str, str, frozenset[str]]] = []

    for 検査関係 in ir.関係:
        検査位置 = _条件値(検査関係, "不足位置")
        検査述語 = _条件値(検査関係, "検索述語")
        if 検査位置 not in {"始点", "終点"} or not 検査述語:
            continue
        if not _条件値(検査関係, "英日意味射影"):
            continue
        検査既知端点 = _既知端点(ir, 検査関係, 検査位置)
        検査条件範囲 = _局所条件範囲(検査関係)
        terms = _意味集合((*検査既知端点, *(value for _key, value in 検査条件範囲)))
        正本関係群.append((検査位置, str(検査関係.種別), terms))

    for 関係 in ir.関係:
        position = _条件値(関係, "不足位置")
        predicate = _条件値(関係, "検索述語")
        if position not in {"始点", "終点"} or not predicate:
            continue
        計画済み関係数 += 1
        known = _既知端点(ir, 関係, position)
        条件範囲 = _局所条件範囲(関係)
        generic = str(関係.種別) in _汎用関係種別群 or bool(_条件値(関係, "選択問題閉包"))
        関係種別 = str(関係.種別)
        if not _条件値(関係, "英日意味射影"):
            local_terms = _意味集合((*known, *(value for _key, value in 条件範囲)))
            dominated = any(
                c_position == position
                and c_kind == 関係種別
                and local_terms
                and c_terms
                and (local_terms <= c_terms or c_terms <= local_terms)
                for c_position, c_kind, c_terms in 正本関係群
            )
            if dominated:
                continue
        predicate_key = (
            str(predicate).casefold()
            if 関係種別 in {"開放述語", "問い適合"}
            else 関係種別
        )
        signature = (
            position,
            predicate_key,
            tuple(str(x).casefold() for x in known),
            tuple((str(k), str(v).casefold()) for k, v in 条件範囲),
            generic,
        )
        if signature in 関係署名群:
            continue
        関係署名群.add(signature)
        stage = "fallback" if generic else "primary"
        基礎優先度 = 90 if generic else 10
        if not generic:
            concrete += 1

        外部文脈 = _関連検索文脈(
            search_surfaces,
            predicate=predicate,
            known=known,
            条件範囲=条件範囲,
        )
        local_anchor = (
            " ".join(_unique((*known, *(value for _key, value in 条件範囲))))
            if generic
            else _query_surface(
                predicate=predicate,
                position=position,
                known=known,
                条件範囲=条件範囲,
                候補=None,
            )
        )
        anchor = _外部表層(local_anchor, 外部文脈) if local_anchor else " ".join(外部文脈)
        if anchor:
            requests.append(HDS参照観測要求(
                ID=f"関係:{関係.関係ID}:anchor",
                関係ID=str(関係.関係ID),
                関係種別=str(関係.種別),
                未知位置=position,
                既知端点=known,
                条件範囲=条件範囲,
                候補ラベル=None,
                候補表層=None,
                外部言語=外部言語名,
                外部検索表層=anchor,
                必須被覆=False,
                外部文脈アンカー=外部文脈,
                段階=stage,
                優先度=基礎優先度,
                provenance=(f"関係:{関係.関係ID}", "Compiler外部文脈"),
            ))

        bound_choices = choices or (("_", ""),)
        for label, 候補 in bound_choices:
            local_surface = (
                " ".join(_unique((候補, *known, *(value for _key, value in 条件範囲))))
                if generic and 候補
                else _query_surface(
                    predicate=predicate,
                    position=position,
                    known=known,
                    条件範囲=条件範囲,
                    候補=候補 or None,
                )
            )
            surface = _外部表層(local_surface, 外部文脈) if local_surface else " ".join(外部文脈)
            if not surface:
                continue
            observation_id = f"関係:{関係.関係ID}:候補:{label}" if choices else f"関係:{関係.関係ID}:観測"
            requests.append(HDS参照観測要求(
                ID=observation_id,
                関係ID=str(関係.関係ID),
                関係種別=str(関係.種別),
                未知位置=position,
                既知端点=known,
                条件範囲=条件範囲,
                候補ラベル=None if label == "_" else label,
                候補表層=候補 or None,
                外部言語=外部言語名,
                外部検索表層=surface,
                必須被覆=True,
                外部文脈アンカー=外部文脈,
                段階=stage,
                優先度=基礎優先度 + 1,
                provenance=(f"関係:{関係.関係ID}", f"候補:{label}", "Compiler外部文脈"),
            ))
            if 候補 and local_surface and local_surface.casefold() != surface.casefold():
                requests.append(HDS参照観測要求(
                    ID=observation_id,
                    関係ID=str(関係.関係ID),
                    関係種別=str(関係.種別),
                    未知位置=position,
                    既知端点=known,
                    条件範囲=条件範囲,
                    候補ラベル=label,
                    候補表層=候補,
                    外部言語=外部言語名,
                    外部検索表層=local_surface,
                    必須被覆=True,
                    外部文脈アンカー=(),
                    段階="fallback",
                    優先度=基礎優先度 + 10,
                    provenance=(f"関係:{関係.関係ID}", f"候補:{label}", "局所検証"),
                ))
            if 候補:
                縮退表層群 = (
                    " ".join(_unique((*known, 候補))),
                    候補,
                )
                for offset, 縮退表層 in enumerate(_unique(縮退表層群), start=1):
                    if 縮退表層.casefold() == surface.casefold():
                        continue
                    requests.append(HDS参照観測要求(
                        ID=observation_id,
                        関係ID=str(関係.関係ID),
                        関係種別=str(関係.種別),
                        未知位置=position,
                        既知端点=known,
                        条件範囲=条件範囲,
                        候補ラベル=label,
                        候補表層=候補,
                        外部言語=外部言語名,
                        外部検索表層=縮退表層,
                        必須被覆=True,
                        外部文脈アンカー=(),
                        段階="fallback",
                        優先度=基礎優先度 + 20 + offset,
                        provenance=(f"関係:{関係.関係ID}", f"候補:{label}", "縮退"),
                    ))

    # 外部検索表層そのものも独立観測として残す。関係queryへのanchor利用とは別責任。
    for index, surface in enumerate(search_surfaces):
        requests.append(HDS参照観測要求(
            ID=f"検索表層:{index}",
            関係ID=None,
            関係種別=None,
            未知位置=None,
            既知端点=(),
            条件範囲=(),
            候補ラベル=None,
            候補表層=None,
            外部言語=外部言語名,
            外部検索表層=surface,
            必須被覆=False,
            外部文脈アンカー=(surface,),
            段階="primary",
            優先度=20 if concrete else 40,
            provenance=("検索.外部表層",),
        ))

    for index, surface in enumerate(audit_surfaces):
        requests.append(HDS参照観測要求(
            ID=f"監査表層:{index}",
            関係ID=None,
            関係種別=None,
            未知位置=None,
            既知端点=(),
            条件範囲=(),
            候補ラベル=None,
            候補表層=None,
            外部言語=外部言語名,
            外部検索表層=surface,
            必須被覆=False,
            外部文脈アンカー=(),
            段階="fallback",
            優先度=80,
            provenance=("監査.R_query",),
        ))

    if choices and 計画済み関係数 == 0:
        汎用文脈 = _汎用外部文脈(ir)
        generic_primary = search_surfaces[0] if search_surfaces else " ".join(汎用文脈)
        if generic_primary:
            requests.append(HDS参照観測要求(
                ID="汎用文脈:0",
                関係ID=None,
                関係種別="未解決関係",
                未知位置=None,
                既知端点=縮退対象群,
                条件範囲=(),
                候補ラベル=None,
                候補表層=None,
                外部言語=外部言語名,
                外部検索表層=generic_primary,
                必須被覆=False,
                外部文脈アンカー=汎用文脈 or (generic_primary,),
                段階="primary",
                優先度=35,
                provenance=("Compiler汎用外部文脈",),
            ))
        anchor = search_surfaces[0] if search_surfaces else " ".join(縮退対象群)
        for label, 候補 in choices:
            observation_id = f"generic:候補:{label}"
            surfaces = _unique((" ".join(_unique((anchor, 候補))), 候補))
            for offset, surface in enumerate(surfaces):
                if not surface:
                    continue
                requests.append(HDS参照観測要求(
                    ID=observation_id,
                    関係ID=None,
                    関係種別="未解決関係",
                    未知位置=None,
                    既知端点=縮退対象群,
                    条件範囲=(),
                    候補ラベル=label,
                    候補表層=候補,
                    外部言語=外部言語名,
                    外部検索表層=surface,
                    必須被覆=True,
                    外部文脈アンカー=(anchor,) if anchor and offset == 0 else (),
                    段階="fallback",
                    優先度=95 + offset,
                    provenance=("未解決関係", f"候補:{label}"),
                ))

    out: list[HDS参照観測要求] = []
    seen: set[tuple[str, str, str]] = set()
    for request in sorted(requests, key=lambda x: (x.優先度, x.ID, x.外部検索表層.casefold())):
        key = (request.ID, request.段階, request.外部検索表層.casefold())
        if key in seen:
            continue
        seen.add(key)
        out.append(request)
    return tuple(out)


def HDS追加観測要求群(
    観測要求: Iterable[HDS参照観測要求],
    *,
    残差群: Iterable[str] = (),
    世代: int = 1,
) -> tuple[HDS参照観測要求, ...]:
    """形成済みKernel観測要求から、追加Rで次に見る観測だけを段階選択する。

    原文や意味IRを再解釈しない。初期primaryを世代ごとに反復せず、
    残差に応じて数量法則・局所検証・縮退・監査/反証・その他を一層ずつ観測し、最後にprimaryを一回だけ再観測する。
    候補競合・候補識別不足では第2世代から監査/反証を優先する。
    """

    requests = tuple(観測要求)
    縮退要求群 = tuple(x for x in requests if x.段階 == "fallback")
    主観測要求群 = tuple(x for x in requests if x.段階 == "primary")
    if not 縮退要求群 and not 主観測要求群:
        return ()

    数量法則 = tuple(x for x in 縮退要求群 if x.関係種別 == "数量計算法則")
    局所 = tuple(x for x in 縮退要求群 if "局所検証" in x.provenance)
    監査 = tuple(
        x for x in 縮退要求群
        if x.ID.startswith("監査表層:") or "監査.R_query" in x.provenance
    )
    縮退 = tuple(x for x in 縮退要求群 if "縮退" in x.provenance)
    その他 = tuple(
        x for x in 縮退要求群
        if x not in 数量法則 and x not in 局所 and x not in 監査 and x not in 縮退
    )

    generation = max(1, int(世代))
    residuals = tuple(str(x) for x in 残差群)
    conflict = any("候補競合" in x or "候補識別不足" in x for x in residuals)
    数量法則不足 = any("数量法則不足" in x for x in residuals)

    if 数量法則不足:
        優先層 = [数量法則, 局所, 縮退, 監査, その他, 主観測要求群]
    elif conflict:
        優先層 = [局所, 監査, 縮退, その他, 数量法則, 主観測要求群]
    else:
        優先層 = [局所, 縮退, 監査, その他, 数量法則, 主観測要求群]
    有効層 = [層 for 層 in 優先層 if 層]
    層番号 = generation - 1
    if 層番号 >= len(有効層):
        return ()
    selected = 有効層[層番号]

    out: list[HDS参照観測要求] = []
    seen: set[tuple[str, str]] = set()
    for request in selected:
        key = (request.ID, request.外部検索表層.casefold())
        if key in seen:
            continue
        seen.add(key)
        out.append(request)
    return tuple(out)




@dataclass(frozen=True, slots=True)
class HDS候補関係観測計画:
    観測要求: tuple[HDS参照観測要求, ...]
    未観測関係数: int
    矛盾関係数: int


def _関係条件範囲(関係: object) -> tuple[tuple[str, str], ...]:
    meta = {"検索述語", "不足位置", "英日意味射影", "受動態", "選択意図", "選択問題閉包", "由来"}
    out: list[tuple[str, str]] = []
    for raw in tuple(getattr(関係, "条件", ())):
        key, sep, value = str(raw).partition("=")
        key, value = key.strip(), value.strip()
        if sep and key and value and key not in meta:
            item = (key, value)
            if item not in out:
                out.append(item)
    return tuple(out)


def _関係端点表層(ir: object, ids: Iterable[str]) -> tuple[str, ...]:
    coords = ir.座標辞書()
    out: list[str] = []
    seen: set[str] = set()
    for cid in ids:
        coord = coords.get(cid)
        if coord is None:
            continue
        value = " ".join(str(getattr(coord, "内容", "")).split()).strip()
        if not value:
            continue
        key = value.casefold()
        if key in seen:
            continue
        seen.add(key)
        out.append(value)
    return tuple(out)


def _候補関係観測表層群(ir: object, 関係: object) -> tuple[str, ...]:
    left = _関係端点表層(ir, getattr(関係, "始点", ()))
    right = _関係端点表層(ir, getattr(関係, "終点", ()))
    predicate = _条件値(関係, "検索述語") or str(getattr(関係, "種別", ""))
    条件範囲 = tuple(v for _k, v in _関係条件範囲(関係))
    候補列 = (
        (*left, predicate, *right, *条件範囲),
        (predicate, *left, *right, *条件範囲),
        (*left, *right, *条件範囲),
    )
    out: list[str] = []
    seen: set[str] = set()
    for 候補 in 候補列:
        parts: list[str] = []
        local_seen: set[str] = set()
        for raw in 候補:
            value = " ".join(str(raw).split()).strip()
            key = value.casefold()
            if value and key not in local_seen:
                local_seen.add(key)
                parts.append(value)
        surface = " ".join(parts)
        key = surface.casefold()
        if surface and key not in seen:
            seen.add(key)
            out.append(surface)
    return tuple(out)


def HDS候補関係観測計画を構成(
    question_ir: HDSIR,
    候補意味IR: Mapping[str, HDSIR] | None,
    資料IR群: Iterable[HDSIR],
) -> HDS候補関係観測計画:
    """現在保持している資料だけから候補関係の未観測・矛盾を導出する。

    評価結果・回答ラベル・goldは入力にしない。導出した不足を同じ通常循環の
    次観測へ反映するための観測要求へ変換する。
    """
    if not 候補意味IR:
        return HDS候補関係観測計画((), 0, 0)

    from dataclasses import replace
    from .HDS模型射影 import HDS内部言語状態
    from .HDS選択仮説 import HDS候補代入仮説群
    from .能力作用則 import 証拠状態照合

    対象言語体系 = "自然言語:ja" if str(getattr(question_ir, "入力言語", "ja") or "ja").casefold().startswith("ja") else "自然言語:en"
    証拠関係群: list[object] = []
    for index, 資料IR in enumerate(tuple(資料IR群)):
        internal = HDS内部言語状態(
            資料IR,
            識別子=f"保持資料:{index}",
            言語体系=対象言語体系,
            証拠境界=True,
        )
        if internal.証拠利用可:
            証拠関係群.extend(tuple(internal.関係構造))

    hypotheses = HDS候補代入仮説群(question_ir, 候補意味IR)
    requests: list[HDS参照観測要求] = []
    seen: set[tuple[str, str]] = set()
    missing_count = 0
    conflict_count = 0

    for label, 候補IR in sorted(hypotheses.items()):
        for 関係 in tuple(getattr(候補IR, "関係", ())):
            if str(getattr(関係, "由来", "")) != "HDS候補代入仮説":
                continue
            projected = HDS内部言語状態(
                replace(候補IR, 関係=(関係,)),
                識別子=f"候補関係:{label}:{getattr(関係, '関係ID', '')}",
                言語体系=対象言語体系,
            )
            targets = tuple(projected.関係構造)
            if not targets:
                continue
            status = 証拠状態照合(targets, tuple(証拠関係群))
            missing = int(getattr(status, "未観測", 0))
            conflict = int(getattr(status, "矛盾", 0))
            if missing <= 0 and conflict <= 0:
                continue
            missing_count += missing
            conflict_count += conflict
            reason = "矛盾関係" if conflict > 0 else "未観測関係"
            for method, surface in enumerate(_候補関係観測表層群(候補IR, 関係), start=1):
                key = (str(label), surface.casefold())
                if key in seen:
                    continue
                seen.add(key)
                基礎優先度 = 0 if conflict > 0 else 5
                requests.append(HDS参照観測要求(
                    ID=f"候補関係:{label}:{getattr(関係, '関係ID', '')}:方法:{method}",
                    関係ID=str(getattr(関係, "関係ID", "")) or None,
                    関係種別=str(getattr(関係, "種別", "")) or None,
                    未知位置=None,
                    既知端点=(
                        *_関係端点表層(候補IR, getattr(関係, "始点", ())),
                        *_関係端点表層(候補IR, getattr(関係, "終点", ())),
                    ),
                    条件範囲=_関係条件範囲(関係),
                    候補ラベル=str(label),
                    候補表層=" ".join(str(getattr(候補意味IR[label], "正規化文", "") or getattr(候補意味IR[label], "原文", "")).split()).strip() or None,
                    外部言語=str(getattr(question_ir, "入力言語", "ja") or "ja"),
                    外部検索表層=surface,
                    必須被覆=True,
                    外部文脈アンカー=(),
                    段階="primary",
                    優先度=基礎優先度 + (method - 1) * 20,
                    provenance=("候補関係観測", reason, f"観測方法:{method}"),
                ))

    return HDS候補関係観測計画(
        tuple(sorted(requests, key=lambda x: (x.優先度, x.ID, x.外部検索表層.casefold()))),
        missing_count,
        conflict_count,
    )


__all__ = [
    "HDS参照観測要求",
    "HDS参照観測要求群",
    "HDS追加観測要求群",
    "HDS候補関係観測計画",
    "HDS候補関係観測計画を構成",
]
