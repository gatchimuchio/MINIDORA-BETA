from __future__ import annotations

from dataclasses import replace

from .HDS中間表現 import HDSIR, HDS座標, HDS関係, HDS残差, HDS意味作用, 値状態
from .言語基底_英日意味 import 英語質問境界解析
from .言語基底_英日意味強化 import 英日意味フレーム抽出


_VERSION = "v0.5"


def _norm(value: object) -> str:
    return " ".join(str(value).split()).strip()


def _条件値(関係: HDS関係, key: str) -> str:
    prefix = key + "="
    for raw in 関係.条件:
        value = str(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _同じ表層(a: object, b: object) -> bool:
    return _norm(a).casefold() == _norm(b).casefold()


def HDS英日意味射影(ir: HDSIR) -> HDSIR:
    """英語表層を日本語正本の意味フレームへ有限射影する。

    v0.5では有限関係語彙外の**明示述語**を開放述語として保持し、命題選択・説明選択も
    「世界事実」へ誤変換せず問い関係として保持する。未知の意味を世界知識で補わない。
    """
    言語 = str(getattr(ir, "入力言語", "") or "").casefold()
    if not 言語.startswith("en"):
        return ir

    text = str(ir.正規化文 or ir.原文)
    frame = 英日意味フレーム抽出(text)
    境界 = 英語質問境界解析(text)
    質問表示 = 境界.質問表示
    if 境界.境界状態 == "when前置句境界未確定" and not 質問表示:
        # 疑問と条件主節の境界が閉じない場合、質問でないことを事実の成立へ読み替えない。
        if not any(残差.残差ID == 'lang-sem:question-境界-unresolved' for 残差 in ir.残差):
            ir = replace(ir, 残差=(*ir.残差, HDS残差(
                'lang-sem:question-境界-unresolved', '意味_loss', 境界.焦点,
                "when節の前置詞句と後続主節の境界を確定できない",
                解消条件=("疑問の主節または条件の主節を構造として保持する",),
            )))
    if not frame.正本意味 and not frame.外部検索語 and frame.関係質問 is None:
        if 質問表示 and not any(残差.種別 == '意味_loss' for 残差 in ir.残差):
            残差 = HDS残差(
                "lang-sem:question-loss",
                '意味_loss',
                text,
                "質問の未知関係を意味IRへ保持できない",
                解消条件=("開放述語または問い適合関係へ射影する",),
            )
            return replace(ir, 残差=(*ir.残差, 残差))
        return ir

    coords = list(ir.座標)
    existing_ids = {coord.座標ID for coord in coords}
    relations = list(ir.関係)
    existing_関係_ids = {関係.関係ID for 関係 in relations}
    operations = list(ir.意味作用履歴)
    residuals = list(ir.残差)

    def add_coord(base: str, kind: str, content: str, 状態: 値状態 = 値状態.確定) -> str:
        value = _norm(content)
        候補 = base
        serial = 1
        while 候補 in existing_ids:
            候補 = f"{base}:{serial}"
            serial += 1
        existing_ids.add(候補)
        coords.append(
            HDS座標(
                候補,
                kind,
                value,
                状態,
                由来="共有言語基底P",
                暫定性='EN_TO_JA_意味_射影',
            )
        )
        return 候補

    if frame.正本意味:
        operations.append(
            HDS意味作用(
                "lang-sem:canonicalize",
                "英日意味正本化",
                ("normalized",),
                (),
                " / ".join(frame.正本意味),
                保持構造=("原文", "外部英語表層", "日本語意味正本", "質問関係修飾", "開放述語"),
                損失=(),
                検証=("世界知識非追加", "外部検索表層分離", "背景制御非伝染", "有限語彙外明示述語保持"),
            )
        )

    if frame.外部検索語:
        external = " ".join(字句 for 字句 in frame.外部検索語 if not 字句.startswith("rel:"))
        if external:
            add_coord("lang-sem:search", "検索.英語正規化", external)

    question = frame.関係質問
    if question is not None:
        known = question.既知端点 or "問い対象"
        coord_map = {coord.座標ID: coord for coord in coords}
        existing_question_index = None
        existing_question = None
        for index, 関係候補 in enumerate(relations):
            if str(関係候補.種別) != str(question.種別):
                continue
            if _条件値(関係候補, "不足位置") != question.未知位置:
                continue
            known_ids = 関係候補.終点 if question.未知位置 == "始点" else 関係候補.始点
            known_values = [
                coord_map[cid].内容
                for cid in known_ids
                if cid in coord_map and coord_map[cid].値状態 not in {値状態.矛盾, 値状態.留保}
            ]
            if any(_同じ表層(value, known) for value in known_values):
                existing_question_index = index
                existing_question = 関係候補
                break

        if existing_question is not None and question.未知位置 == "始点":
            # 旧基礎Compilerの未知ノードだけを再利用し、既知端点は英日正本のclean surfaceへ置換する。
            # これにより未知座標を二重化せず、条件句を端点へ抱き込んだ旧表層をKernel正本へ残さない。
            start_id = existing_question.始点[0]
            end_id = add_coord("lang-sem:known:end", "対象.終点", known)
        elif existing_question is not None:
            start_id = add_coord("lang-sem:known:start", "対象.始点", known)
            end_id = existing_question.終点[0]
        elif question.未知位置 == "始点":
            start_id = add_coord('lang-sem:未知:start', "目的.未知始点", question.要求型 or "未特定", 値状態.未観測)
            end_id = add_coord("lang-sem:known:end", "対象.終点", known)
        else:
            start_id = add_coord("lang-sem:known:start", "対象.始点", known)
            end_id = add_coord('lang-sem:未知:end', "目的.未知終点", question.要求型 or "未特定", 値状態.未観測)

        if not any(str(coord.種別) == "目的.不足位置" and _同じ表層(coord.内容, question.未知位置) for coord in coords):
            add_coord("lang-sem:missing", "目的.不足位置", question.未知位置)
        if question.要求型 and not any(str(coord.種別) == "目的.要求型" and _同じ表層(coord.内容, question.要求型) for coord in coords):
            add_coord("lang-sem:type", "目的.要求型", question.要求型)

        関係_conditions = [
            f"検索述語={question.検索述語}",
            f"不足位置={question.未知位置}",
            f"英日意味射影={_VERSION}",
            f"受動態={str(question.受動).lower()}",
            f"選択意図={'反転' if question.反転 else '通常'}",
        ]
        関係_conditions.extend(f"{key}={value}" for key, value in question.修飾)

        if existing_question is not None and existing_question_index is not None:
            canonical_keys = {str(raw).partition("=")[0].strip() for raw in 関係_conditions}
            inherited = tuple(
                str(raw)
                for raw in existing_question.条件
                if str(raw).partition("=")[0].strip() not in canonical_keys
            )
            relations[existing_question_index] = replace(
                existing_question,
                条件=tuple(dict.fromkeys((*関係_conditions, *inherited))),
                値状態=値状態.未観測,
                由来="共有言語基底P",
                暫定性='EN_TO_JA_意味_射影',
            )
        else:
            rid = 'lang-sem:関係-question'
            serial = 1
            while rid in existing_関係_ids:
                rid = f"lang-sem:relation-question:{serial}"
                serial += 1
            relations.insert(
                0,
                HDS関係(
                    rid,
                    (start_id,),
                    (end_id,),
                    question.種別,
                    条件=tuple(dict.fromkeys(関係_conditions)),
                    値状態=値状態.未観測,
                    由来="共有言語基底P",
                    暫定性='EN_TO_JA_意味_射影',
                ),
            )

        if question.反転 and not any(
            str(coord.種別) == "制御.選択意図" and str(coord.内容) == "反転" for coord in coords
        ):
            add_coord("lang-sem:selection", "制御.選択意図", "反転")
    elif 質問表示 and not any(残差.種別 == '意味_loss' for 残差 in residuals):
        residuals.append(
            HDS残差(
                "lang-sem:question-loss",
                '意味_loss',
                text,
                "質問の未知関係を意味IRへ保持できない",
                解消条件=("開放述語または問い適合関係へ射影する",),
            )
        )

    return replace(ir, 座標=tuple(coords), 関係=tuple(relations), 残差=tuple(residuals), 意味作用履歴=tuple(operations))


__all__ = ["HDS英日意味射影"]
