from __future__ import annotations

from typing import Any, Mapping

from .HDS中間表現 import (
    HDSIR,
    HDS実行核,
    HDS座標,
    HDS残差,
    HDS意味作用,
    HDS関係,
    値状態,
)


def _tuple(value: Any) -> tuple:
    if value is None:
        return ()
    return tuple(value)


def HDSIR辞書化(ir: HDSIR) -> dict[str, Any]:
    '構文化器内部を含めず、公開HDS-IRを再生可能なJSON形へ変換する。\n\n    `手順` はK/J性能再生では使用しないため保存対象外とする。Layer-0実行手順を\n    再現する用途ではなく、問題/候補/資料の意味構造を固定して実行系性能差だけを\n    比較することが目的である。\n    '
    return {
        "契約形式": 'minidora.hds-ir.再生.v1',
        "原文": ir.原文,
        "正規化文": ir.正規化文,
        "認知世界ID": ir.認知世界ID,
        "座標": [
            {
                "座標ID": c.座標ID,
                "種別": c.種別,
                "内容": c.内容,
                "値状態": c.値状態.value,
                "原文範囲": list(c.原文範囲) if c.原文範囲 is not None else None,
                "由来": c.由来,
                "暫定性": c.暫定性,
                "再開放条件": list(c.再開放条件),
            }
            for c in ir.座標
        ],
        "関係": [
            {
                "関係ID": r.関係ID,
                "始点": list(r.始点),
                "終点": list(r.終点),
                "種別": r.種別,
                "条件": list(r.条件),
                "値状態": r.値状態.value,
                "由来": r.由来,
                "暫定性": r.暫定性,
            }
            for r in ir.関係
        ],
        "残差": [
            {
                "残差ID": r.残差ID,
                "種別": r.種別,
                "原文": r.原文,
                "理由": r.理由,
                "影響座標": list(r.影響座標),
                "解消条件": list(r.解消条件),
            }
            for r in ir.残差
        ],
        "意味作用履歴": [
            {
                "作用ID": a.作用ID,
                "種別": a.種別,
                "入力参照": list(a.入力参照),
                "出力参照": list(a.出力参照),
                "変換": a.変換,
                "保持構造": list(a.保持構造),
                "損失": list(a.損失),
                "検証": list(a.検証),
            }
            for a in ir.意味作用履歴
        ],
        "実行核": {
            "作用": ir.実行核.作用,
            "入力座標": list(ir.実行核.入力座標),
            "出力座標": ir.実行核.出力座標,
            "境界": list(ir.実行核.境界),
            "検証": list(ir.実行核.検証),
        },
        "初期状態": ir.初期状態,
        "参照必須": ir.参照必須,
        "種別": ir.種別,
        "閉包状態": ir.閉包状態,
        "表現状態": ir.表現状態,
        "保持状態": ir.保持状態,
        "暫定性状態": ir.暫定性状態,
        "入力言語": ir.入力言語,
        "出力言語": ir.出力言語,
        "文脈引用": list(ir.文脈引用),
        "手順省略": True,
    }


def HDSIR復元(資料: Mapping[str, Any]) -> HDSIR:
    schema = 資料.get("契約形式")
    if schema not in {None, 'minidora.hds-ir.再生.v1'}:
        raise ValueError(f"未対応HDS-IR replay schema: {schema}")

    coords = tuple(
        HDS座標(
            座標ID=str(c["座標ID"]),
            種別=str(c["種別"]),
            内容=c.get("内容"),
            値状態=値状態(str(c.get("値状態", 値状態.確定.value))),
            原文範囲=(tuple(c["原文範囲"]) if c.get("原文範囲") is not None else None),
            由来=str(c.get("由来", "自然言語入力")),
            暫定性=str(c.get("暫定性", "原則暫定")),
            再開放条件=_tuple(c.get("再開放条件")),
        )
        for c in 資料.get("座標", ())
    )
    relations = tuple(
        HDS関係(
            関係ID=str(r["関係ID"]),
            始点=tuple(str(x) for x in r.get("始点", ())),
            終点=tuple(str(x) for x in r.get("終点", ())),
            種別=str(r["種別"]),
            条件=tuple(str(x) for x in r.get("条件", ())),
            値状態=値状態(str(r.get("値状態", 値状態.確定.value))),
            由来=str(r.get("由来", "自然言語入力")),
            暫定性=str(r.get("暫定性", "原則暫定")),
        )
        for r in 資料.get("関係", ())
    )
    residuals = tuple(
        HDS残差(
            残差ID=str(r["残差ID"]),
            種別=str(r["種別"]),
            原文=str(r.get("原文", "")),
            理由=str(r.get("理由", "")),
            影響座標=tuple(str(x) for x in r.get("影響座標", ())),
            解消条件=tuple(str(x) for x in r.get("解消条件", ())),
        )
        for r in 資料.get("残差", ())
    )
    effects = tuple(
        HDS意味作用(
            作用ID=str(a["作用ID"]),
            種別=str(a["種別"]),
            入力参照=tuple(str(x) for x in a.get("入力参照", ())),
            出力参照=tuple(str(x) for x in a.get("出力参照", ())),
            変換=str(a.get("変換", "")),
            保持構造=tuple(str(x) for x in a.get("保持構造", ())),
            損失=tuple(str(x) for x in a.get("損失", ())),
            検証=tuple(str(x) for x in a.get("検証", ())),
        )
        for a in 資料.get("意味作用履歴", ())
    )
    模型核_資料 = 資料.get("実行核", {})
    execution_模型核 = HDS実行核(
        作用=模型核_資料.get("作用"),
        入力座標=tuple(str(x) for x in 模型核_資料.get("入力座標", ())),
        出力座標=str(模型核_資料.get("出力座標", "結果")),
        境界=tuple(str(x) for x in 模型核_資料.get("境界", ())),
        検証=tuple(str(x) for x in 模型核_資料.get("検証", ())),
    )

    return HDSIR(
        原文=str(資料.get("原文", "")),
        正規化文=str(資料.get("正規化文", 資料.get("原文", ""))),
        認知世界ID=str(資料.get("認知世界ID", '再生')),
        座標=coords,
        関係=relations,
        残差=residuals,
        意味作用履歴=effects,
        実行核=execution_模型核,
        初期状態=dict(資料.get("初期状態", {})),
        参照必須=bool(資料.get("参照必須", False)),
        種別=str(資料.get("種別", "一般")),
        閉包状態=str(資料.get("閉包状態", "OPEN")),
        表現状態=str(資料.get("表現状態", "部分構文化")),
        保持状態=str(資料.get("保持状態", "全領域有効")),
        暫定性状態=str(資料.get("暫定性状態", "原則暫定")),
        手順=None,
        入力言語=str(資料.get("入力言語", "ja")),
        出力言語=(str(資料["出力言語"]) if 資料.get("出力言語") is not None else None),
        文脈引用=tuple(str(x) for x in 資料.get("文脈引用", ())),
    )


__all__ = ["HDSIR辞書化", "HDSIR復元"]
