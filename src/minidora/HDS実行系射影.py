from __future__ import annotations

from dataclasses import replace

from .HDS中間表現 import HDSIR, HDS座標, HDS関係, HDS残差, 値状態


_K_意味_PREFIXES = (
    "対象.",
    "実体.",
    "関係.述語",
    "属性.",
    "値.",
    "状態.",
)
_K_EXCLUDED_KINDS = frozenset({"状態.否定"})
_R_文脈_PREFIXES = (
    "検索.",
    "条件.",
    "文脈.",
    "時刻.",
    "時間.",
    "範囲.",
)
_R_代替経路_意味_PREFIXES = (
    "対象.",
    "実体.",
    "状態.",
    "属性.",
    "値.",
)
_R_CONTROL_CONDITION_KINDS = frozenset({"条件.検索極性"})
_BLOCKING = {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}
_K_修飾_範囲_KEYS = frozenset({"様相", "量化", '条件範囲', '範囲', "条件作用"})
_候補_ASSERTION_PREFIXES = (
    "状態.", "条件.", "動態.", "不確実性.", "前提.", "射程.", "論証.",
)


def _条件値(関係: HDS関係, key: str) -> str:
    prefix = key + "="
    for raw in 関係.条件:
        value = str(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _座標重複除去(*groups: tuple[HDS座標, ...]) -> tuple[HDS座標, ...]:
    out: list[HDS座標] = []
    seen: set[str] = set()
    for group in groups:
        for coord in group:
            cid = str(coord.座標ID)
            if cid in seen:
                continue
            seen.add(cid)
            out.append(coord)
    return tuple(out)


def _文字列重複除去(values: tuple[str, ...]) -> tuple[str, ...]:
    out: list[str] = []
    seen: set[str] = set()
    for raw in values:
        value = " ".join(str(raw).split()).strip()
        if not value:
            continue
        key = value.casefold()
        if key in seen:
            continue
        seen.add(key)
        out.append(value)
    return tuple(out)


def _R利用座標(coord: HDS座標) -> bool:
    return str(coord.種別) not in _R_CONTROL_CONDITION_KINDS


def _R検索表層(coords: tuple[HDS座標, ...], relations: tuple[HDS関係, ...] = ()) -> str:
    search = tuple(str(coord.内容) for coord in coords if str(coord.種別).startswith("検索."))
    predicates = tuple(_条件値(関係, "検索述語") for 関係 in relations)
    known_endpoints = tuple(
        str(coord.内容)
        for coord in coords
        if str(coord.種別).startswith(("対象.", "実体.", "状態.", "属性.", "値."))
        and coord.値状態 not in _BLOCKING
    )
    文脈 = tuple(
        str(coord.内容)
        for coord in coords
        if str(coord.種別).startswith(("条件.", "時刻.", "時間.", "範囲."))
        and _R利用座標(coord)
        and coord.値状態 not in {値状態.矛盾, 値状態.留保}
    )
    return " ".join(_文字列重複除去((*search, *predicates, *known_endpoints, *文脈)))


def _K意味座標(coord: HDS座標) -> bool:
    kind = str(coord.種別)
    if kind in _K_EXCLUDED_KINDS:
        return False
    return kind.startswith(_K_意味_PREFIXES)


def _候補は命題(ir: HDSIR) -> bool:
    if ir.関係:
        return True
    for coord in ir.座標:
        kind = str(coord.種別)
        if kind == "関係.述語" or kind.startswith(_候補_ASSERTION_PREFIXES):
            return True
    return False


def _質問関係(ir: HDSIR) -> tuple[HDS関係, ...]:
    canonical = tuple(
        関係
        for 関係 in ir.関係
        if _条件値(関係, "英日意味射影")
        and _条件値(関係, "不足位置") in {"始点", "終点"}
    )
    if canonical:
        return canonical
    return tuple(
        関係
        for 関係 in ir.関係
        if _条件値(関係, "不足位置") in {"始点", "終点"}
    )


def _関係座標ID(relations: tuple[HDS関係, ...]) -> set[str]:
    ids: set[str] = set()
    for 関係 in relations:
        ids.update(関係.始点)
        ids.update(関係.終点)
    return ids


def _関係を座標へ閉じる(relations: tuple[HDS関係, ...], coordinate_ids: set[str]) -> tuple[HDS関係, ...]:
    return tuple(
        関係
        for 関係 in relations
        if (*関係.始点, *関係.終点)
        and all(cid in coordinate_ids for cid in (*関係.始点, *関係.終点))
    )


def _K未対応範囲(
    関係: HDS関係,
    *,
    否定可: bool = False,
    修飾可: bool = False,
) -> bool:
    'K/模型境界が保持できない範囲だけを辺から除外する。'
    for raw in 関係.条件:
        value = str(raw)
        key, sep, payload = value.partition("=")
        if not sep:
            continue
        key = key.strip()
        payload = payload.strip()
        if key == "極性" and payload:
            if payload == "肯定":
                continue
            if payload == "否定" and 否定可:
                continue
            return True
        if key in _K_修飾_範囲_KEYS and payload:
            if 修飾可:
                continue
            return True
    return False


def _K関係射影(
    ir: HDSIR,
    coords: tuple[HDS座標, ...],
    *,
    否定可: bool = False,
    修飾可: bool = False,
) -> tuple[HDS関係, ...]:
    closed = _関係を座標へ閉じる(ir.関係, {coord.座標ID for coord in coords})
    return tuple(
        関係
        for 関係 in closed
        if not _K未対応範囲(関係, 否定可=否定可, 修飾可=修飾可)
    )


def HDSR質問射影(ir: HDSIR) -> HDSIR:
    """質問HDS-IRからRが検索query生成に必要な最小構造だけを返す。"""
    choices = tuple(coord for coord in ir.座標 if coord.座標ID.startswith('選択肢:'))
    question_relations = _質問関係(ir)
    coords_by_id = ir.座標辞書()
    文脈 = tuple(
        coord
        for coord in ir.座標
        if str(coord.種別).startswith(_R_文脈_PREFIXES)
        and _R利用座標(coord)
        and coord.値状態 not in {値状態.矛盾, 値状態.留保}
        and str(coord.内容).strip()
    )

    if question_relations:
        endpoint_ids = _関係座標ID(question_relations)
        endpoints = tuple(coords_by_id[cid] for cid in endpoint_ids if cid in coords_by_id)
        projected_coords = _座標重複除去(choices, endpoints, 文脈)
        surface = _R検索表層(projected_coords, question_relations)
        return replace(
            ir,
            原文=surface,
            正規化文=surface,
            座標=projected_coords,
            関係=question_relations,
            意味作用履歴=(),
        )

    search_focus = tuple(coord for coord in 文脈 if str(coord.種別).startswith("検索."))
    if search_focus:
        projected_coords = _座標重複除去(choices, search_focus)
        surface = _R検索表層(projected_coords)
        return replace(ir, 原文=surface, 正規化文=surface, 座標=projected_coords, 関係=(), 意味作用履歴=())

    代替経路 = tuple(
        coord
        for coord in ir.座標
        if str(coord.種別).startswith((*_R_代替経路_意味_PREFIXES, *_R_文脈_PREFIXES))
        and _R利用座標(coord)
        and coord.値状態 not in _BLOCKING
        and str(coord.内容).strip()
    )
    relations = _関係を座標へ閉じる(ir.関係, {coord.座標ID for coord in 代替経路})
    projected_coords = _座標重複除去(choices, 代替経路)
    surface = _R検索表層(projected_coords, relations)
    return replace(ir, 原文=surface, 正規化文=surface, 座標=projected_coords, 関係=relations, 意味作用履歴=())


def HDSK質問射影(ir: HDSIR) -> HDSIR:
    '質問HDS-IRから候補比較へ必要な意味核を、範囲を落とさず返す。'
    choices = tuple(coord for coord in ir.座標 if coord.座標ID.startswith('選択肢:'))
    question_relations = _質問関係(ir)
    coords_by_id = ir.座標辞書()

    if question_relations:
        endpoint_ids = _関係座標ID(question_relations)
        endpoints = tuple(coords_by_id[cid] for cid in endpoint_ids if cid in coords_by_id)
        projected_relations = tuple(
            replace(
                関係,
                値状態=値状態.確定,
                由来='HDS 実行系 K質問射影',
                暫定性='関係_TYPE_KNOWN_ENDPOINT_OPEN',
            )
            for 関係 in question_relations
            if not _K未対応範囲(関係, 否定可=True, 修飾可=True)
        )
        if projected_relations:
            return replace(
                ir,
                座標=_座標重複除去(choices, endpoints),
                関係=projected_relations,
                意味作用履歴=(),
            )

    topic = tuple(
        coord
        for coord in ir.座標
        if str(coord.種別) == "対象.主題語"
        and coord.値状態 not in _BLOCKING
        and str(coord.内容).strip()
    )

    # 知識選択問題を「関係不明のtopic bag」のまま正常処理しない。
    if ir.種別 == "knowledge_query" or choices:
        residuals = tuple(ir.残差)
        if not any(残差.種別 == '意味_loss' for 残差 in residuals):
            residuals = (
                *residuals,
                HDS残差(
                    "実行系:question-loss",
                    '意味_loss',
                    ir.原文,
                    "知識選択質問が問い関係を持たずtopic-onlyへ落ちることを禁止",
                    解消条件=("問い関係を開放述語・命題適合・説明適合・問い適合のいずれかへ射影する",),
                ),
            )
        return replace(
            ir,
            座標=_座標重複除去(choices, topic),
            関係=(),
            残差=residuals,
            意味作用履歴=(),
        )

    if topic:
        return replace(ir, 座標=_座標重複除去(choices, topic), 関係=(), 意味作用履歴=())

    意味_coords = tuple(coord for coord in ir.座標 if _K意味座標(coord))
    意味_relations = _K関係射影(ir, 意味_coords)
    return replace(ir, 座標=_座標重複除去(choices, 意味_coords), 関係=意味_relations, 意味作用履歴=())


def HDSK候補射影(ir: HDSIR) -> HDSIR:
    '候補IRから明示命題を、否定・様相・条件範囲を保ったまま残す。'
    意味_coords = tuple(coord for coord in ir.座標 if _K意味座標(coord))
    意味_relations = _K関係射影(ir, 意味_coords, 否定可=True, 修飾可=True)
    return replace(ir, 座標=意味_coords, 関係=意味_relations, 意味作用履歴=())


def HDSK候補代入可能(ir: HDSIR) -> bool:
    """完全候補IRが未知端点へ代入する実体句として扱える時だけTrue。"""
    if _候補は命題(ir):
        return False
    return any(
        str(coord.種別).startswith(("対象.", "実体."))
        and coord.値状態 not in _BLOCKING
        and str(coord.内容).strip()
        for coord in ir.座標
    )


def HDS模型候補代入可能(question_ir: HDSIR, 候補_ir: HDSIR) -> bool:
    '正式模型だけが使う、質問型を含めた候補代入可否。\n\n    候補内部の関係を世界事実としてcanonical Kへ昇格させる判定ではない。\n    問いの未知端点へ候補表層を比較専用仮説として置けるかだけを判定する。\n    旧v0.3 補助器は従来の ``HDSK候補代入可能`` を使い続ける。\n    '
    if any(str(残差.種別) == '意味_loss' for 残差 in 候補_ir.残差):
        return False
    surface = " ".join(str(候補_ir.正規化文 or 候補_ir.原文).split()).strip()
    if not surface:
        return False

    question_relations = _質問関係(question_ir)
    if not question_relations:
        return False
    kinds = {str(関係.種別) for 関係 in question_relations}

    # 命題・説明・一般選択は候補全体が回答payload。候補内部のrelation有無で拒否しない。
    if kinds.intersection({"問い適合", "命題適合", "説明適合"}):
        return True

    # 数量/同定では値表現そのものが未知端点となる。数量単位relationが存在しても、
    # それは候補payload内部構造であり、世界命題への昇格ではない。
    if kinds.intersection({"数量同定", "同定"}):
        return any(
            str(coord.種別).startswith(("値.", "属性."))
            and coord.値状態 not in _BLOCKING
            and str(coord.内容).strip()
            for coord in 候補_ir.座標
        ) or HDSK候補代入可能(候補_ir)

    # 世界関係の未知端点は従来どおり実体句だけに限定する。
    return HDSK候補代入可能(候補_ir)


def HDSK資料射影(ir: HDSIR) -> HDSIR:
    'R取得資料から世界事実意味を、極性・修飾を保って模型参照状態へ渡す。'
    意味_coords = tuple(coord for coord in ir.座標 if _K意味座標(coord))
    意味_relations = _K関係射影(ir, 意味_coords, 否定可=True, 修飾可=True)
    return replace(ir, 座標=意味_coords, 関係=意味_relations, 意味作用履歴=())


__all__ = [
    "HDSR質問射影",
    "HDSK質問射影",
    "HDSK候補射影",
    "HDSK候補代入可能",
    "HDS模型候補代入可能",
    'HDSK資料射影',
]
