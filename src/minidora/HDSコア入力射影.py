"""HDS意味IRからMINIDORA Core消費情報だけを射影する。

監査座標、計算手順、能力名、作用実行結果、最終採否は正本入力へ混入させない。
"""
from __future__ import annotations

from dataclasses import replace

from .コア.値 import 署名
from .HDSコア要求抽出 import 明示作用要求を抽出
from .HDSコア入力 import (
    HDSコア意味項目, HDSコア条件, HDSコア関係, HDSコア目的,
    HDSコア作用要求, HDSコア残差, HDSコア検証要求,
    HDSコア実行制約, HDSコア表現制約, HDSコア入力束,
)

_監査接頭辞 = ("監査.", "保持.", "暫定性.", "帰還.")
_関係メタ鍵群 = frozenset({
    "検索述語", "不足位置", "英日意味射影", "受動態", "選択意図", "選択問題閉包", "由来",
})


def _条件内容キー(値: object) -> str:
    return " ".join(str(値).split()).casefold()


def _状態名(値) -> str:
    return str(getattr(値, "value", 値))


def _監査項目か(座標) -> bool:
    種別 = str(getattr(座標, "種別", ""))
    return 種別.startswith(_監査接頭辞)


def HDSコア入力へ(IR) -> HDSコア入力束:
    """意味IRをCore入力正本へ射影する。

    実行計画や計算Pを再利用しない。IRに明示された意味だけを移す。
    """
    必須 = ("原文", "認知世界ID", "座標", "関係", "残差", "入力言語", "出力言語", "文脈引用", "参照必須")
    if any(not hasattr(IR, 名) for 名 in 必須):
        raise TypeError("HDS意味IR契約が不足")
    if not isinstance(IR.原文, str) or not IR.原文.strip():
        raise ValueError("Core入力には空でない原文が必要")

    意味項目 = []
    除外ID = set()
    for 座標 in tuple(IR.座標):
        ID = str(getattr(座標, "座標ID"))
        if _監査項目か(座標):
            除外ID.add(ID)
            continue
        意味項目.append(HDSコア意味項目(
            ID=ID,
            種別=str(getattr(座標, "種別")),
            内容=getattr(座標, "内容"),
            状態=_状態名(getattr(座標, "値状態")),
            由来=str(getattr(座標, "由来", "自然言語入力")),
            原文範囲=getattr(座標, "原文範囲", None),
        ))
    意味ID = {x.ID for x in 意味項目}

    条件 = []
    for 項目 in 意味項目:
        if not 項目.種別.startswith("条件."):
            continue
        条件.append(HDSコア条件(
            ID=項目.ID,
            種別=項目.種別.split(".", 1)[1] or "条件",
            内容=項目.内容,
            状態=項目.状態,
            由来=項目.由来,
            原文範囲=項目.原文範囲,
        ))
    条件内容索引: dict[str, list[str]] = {}
    for 項目 in 条件:
        条件内容索引.setdefault(_条件内容キー(項目.内容), []).append(項目.ID)

    関係 = []
    追加残差 = []
    for 関係元 in tuple(IR.関係):
        関係ID = str(getattr(関係元, "関係ID"))
        始点 = tuple(str(x) for x in getattr(関係元, "始点"))
        終点 = tuple(str(x) for x in getattr(関係元, "終点"))
        参照 = set((*始点, *終点))
        if 参照 and 参照 <= 除外ID:
            continue
        if 参照 & 除外ID:
            追加残差.append(HDSコア残差(
                ID="Core入力:監査依存:" + 関係ID,
                種別="監査副産物依存",
                理由="意味関係が監査副産物を参照しておりCore入力へ安全に分離できない",
                影響参照=tuple(sorted(参照 - 除外ID)),
                解消条件=("監査副産物参照を意味関係から分離する",),
            ))
            continue
        if not 参照 <= 意味ID:
            追加残差.append(HDSコア残差(
                ID="Core入力:参照欠落:" + 関係ID,
                種別="関係参照欠落",
                理由="意味関係の参照先がCore入力意味項目に存在しない",
                影響参照=tuple(sorted(参照)),
                解消条件=("関係参照先を構文化する",),
            ))
            continue
        # HDS関係.条件には検索述語・不足位置など構文化/照合用メタデータが含まれる。
        # メタは制約として保持し、既に構文化済みの条件座標と一致する意味条件だけを明示bindingする。
        制約 = tuple(str(x) for x in getattr(関係元, "条件", ()))
        関係条件ID: list[str] = []
        for raw in 制約:
            鍵, 区切り, 値 = str(raw).partition("=")
            鍵, 値 = 鍵.strip(), 値.strip()
            if not 区切り or not 値 or 鍵 in _関係メタ鍵群:
                continue
            for 条件ID in 条件内容索引.get(_条件内容キー(値), ()):
                if 条件ID not in 関係条件ID:
                    関係条件ID.append(条件ID)
        関係.append(HDSコア関係(
            ID=関係ID,
            始点=始点,
            終点=終点,
            種別=str(getattr(関係元, "種別")),
            条件ID=tuple(関係条件ID),
            制約=制約,
            状態=_状態名(getattr(関係元, "値状態")),
            由来=str(getattr(関係元, "由来", "自然言語入力")),
        ))

    条件適用先: dict[str, list[str]] = {x.ID: [] for x in 条件}
    for 関係項 in 関係:
        for 条件ID in 関係項.条件ID:
            if 条件ID in 条件適用先 and 関係項.ID not in 条件適用先[条件ID]:
                条件適用先[条件ID].append(関係項.ID)
    条件 = [
        replace(項目, 適用先=tuple(条件適用先[項目.ID]))
        for 項目 in 条件
    ]

    目的 = []
    for 項目 in 意味項目:
        if not 項目.種別.startswith("目的."):
            continue
        種別 = 項目.種別.split(".", 1)[1] or "目的"
        関連参照 = []
        for 関係項 in 関係:
            端点 = (*関係項.始点, *関係項.終点)
            if 項目.ID in 端点:
                関連参照.extend(x for x in 端点 if x != 項目.ID)
        目的.append(HDSコア目的(
            項目.ID, 種別, 項目.内容, tuple(dict.fromkeys(関連参照)), 項目.原文範囲
        ))

    # 目的.*を能力名へ読み替えず、原文に明示された作用意味だけを別経路で抽出する。
    提示座標 = [項目 for 項目 in 意味項目
                if 項目.種別 == "情報源.会話提示本文" and 項目.由来 == "公開HDS 会話射影"]
    制御文 = IR.原文
    if 提示座標:
        if len(提示座標) != 1:
            raise ValueError("提示本文のKernel原文範囲が不整合")
        if 提示座標[0].原文範囲 is not None:
            if IR.原文[slice(*提示座標[0].原文範囲)] != 提示座標[0].内容:
                raise ValueError("提示本文のKernel原文範囲が不整合")
            制御文 = IR.原文[:提示座標[0].原文範囲[0]].rstrip("\r\n")
    if any(座標.種別 == "目的.会話操作" for 座標 in IR.座標):
        from .HDS会話意味射影 import _引用を隠す
        # 会話意味の引用した対象はデータであり、内部の命令語を再昇格しない。
        # 同じ長さで隠すため、引用外の実要求は元の原文範囲を維持する。
        制御文 = _引用を隠す(制御文)
    要求抽出 = 明示作用要求を抽出(制御文)
    作用要求 = list(要求抽出.作用要求)
    表現要求 = list(要求抽出.表現要求)
    実行制約 = list(要求抽出.実行制約)
    if bool(IR.参照必須):
        実行制約.append(HDSコア実行制約(
            ID="実行制約:参照必須",
            種別="参照",
            値="必須",
        ))

    残差 = list(要求抽出.残差)
    for 項目 in tuple(IR.残差):
        解消条件 = tuple(str(x) for x in getattr(項目, "解消条件", ()))
        残差.append(HDSコア残差(
            ID=str(getattr(項目, "残差ID")),
            種別=str(getattr(項目, "種別")),
            理由=str(getattr(項目, "理由")),
            原文=str(getattr(項目, "原文", "")),
            影響参照=tuple(str(x) for x in getattr(項目, "影響座標", ())),
            解消条件=解消条件,
        ))
    残差.extend(追加残差)

    # 完了条件は明示作用要求の期待成果だけを供給し、目的座標からは捏造しない。
    要求成果 = tuple(dict.fromkeys(成果 for 要求 in 作用要求 for 成果 in 要求.要求成果))

    明示出力言語 = tuple(dict.fromkeys(x.値 for x in 表現要求 if x.種別 == "出力言語"))
    if len(明示出力言語) > 1:
        残差.append(HDSコア残差(
            ID="Core入力:出力言語競合",
            種別="表現要求競合",
            原文=IR.原文,
            理由="複数の出力言語要求が競合している",
            解消条件=("出力言語を一つに固定する",),
        ))
        出力言語 = None if IR.出力言語 is None else str(IR.出力言語)
    elif 明示出力言語:
        出力言語 = 明示出力言語[0]
    else:
        出力言語 = None if IR.出力言語 is None else str(IR.出力言語)

    表現制約 = HDSコア表現制約(
        str(IR.入力言語),
        出力言語,
        tuple(表現要求),
    )

    # 表現要求の競合を含め、最終的に残った全残差へ検証要求を対応付ける。
    検証要求 = tuple(
        HDSコア検証要求(
            ID="検証:" + 項目.ID,
            種別="残差解消確認",
            対象参照=項目.影響参照,
            条件=項目.解消条件 or ("残差が解消されること",),
        )
        for 項目 in 残差
    )
    由来署名 = 署名((
        IR.原文, IR.認知世界ID,
        tuple((x.ID, x.種別, x.内容, x.状態, x.由来, x.原文範囲) for x in 意味項目),
        tuple((x.ID, x.始点, x.終点, x.種別, x.条件ID, x.制約, x.状態, x.由来) for x in 関係),
        tuple((x.ID, x.種別, x.内容, x.適用先) for x in 条件),
        tuple((x.ID, x.種別, x.理由, x.影響参照, x.解消条件) for x in 残差),
        tuple((x.ID, x.種別, x.値, x.原文範囲) for x in 実行制約),
        tuple((x.ID, x.種別, x.値, x.原文範囲) for x in 表現要求),
    ))
    結果 = HDSコア入力束(
        原文=IR.原文,
        認知世界ID=str(IR.認知世界ID),
        意味項目=tuple(意味項目),
        関係=tuple(関係),
        条件=tuple(条件),
        目的=tuple(目的),
        作用要求=tuple(作用要求),
        要求成果=要求成果,
        残差=tuple(残差),
        検証要求=検証要求,
        実行制約=tuple(実行制約),
        表現制約=表現制約,
        文脈引用=tuple(str(x) for x in IR.文脈引用),
        射影由来署名=由来署名,
    )
    結果.コア需要を検査()
    return 結果


__all__ = ["HDSコア入力へ"]
