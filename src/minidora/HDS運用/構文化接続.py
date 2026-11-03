"""現行構文化器と局所能力のLegacy責任境界。

Core-first正本は HDSコア入力束。ここは既存能力経路のためにLegacy意味IRへ射影し、
局所能力の対応範囲だけを検査する。利用者文の条件・関係・残差は免除しない。
"""
from __future__ import annotations
from dataclasses import replace
import re
from ..HDS構文化器_v1 import 公開HDSコンパイラ
from ..HDS中間表現 import HDSIR, 値状態

接続版 = "HDS-現行構文化局所接続-v1"
_生成元 = "公開HDS 構文化器 構造 v1"
_委譲 = "HDS Native/Kernelの導出規則と最終採否は公開構文化器へ含めない"
_座標役割 = frozenset(("発話主体", "作用主体", "対象", "時間", "空間", "目的", "機構"))
_運用要求 = {
    "座標固定要求": "目的計画・能力の入力役割。未観測の世界座標を確定したとはしない",
    "未定義・未解参照要求": "局所解釈の残差照合と資料束縛。未解消なら実行を保留",
    "閉包要求": "各作用の実成果とHDS要求状態の閉包",
    "保持要求": "原IR・原資料・未解釈を改変せず保持",
    "意味損失要求": "この委譲記録以外の損失は局所解釈側でも検査",
    "暫定性要求": "資料版と依存を保持し変更時に再開放",
    "最終採否委譲": "HDS実行主体の通常循環と最終検証",
}


def 局所接続を構成(ir):
    if type(ir) is not HDSIR or len(ir.座標辞書()) != len(ir.座標):
        raise ValueError("構文化IRの型・座標重複")
    local, delegated, records = [], set(), []
    for point in ir.座標:
        responsibility = None
        if point.由来 == _生成元 and re.fullmatch(r"archv1:[0-9]+", point.座標ID) and point.原文範囲 is None:
            kind, value, status = point.種別, point.内容, point.値状態
            if kind == "監査.構造" and value == "v1" and status == 値状態.確定:
                responsibility = "構文化器の構造版。利用者要求ではない"
            elif kind == "監査.座標未固定" and value in _座標役割 and status == 値状態.未観測:
                responsibility = "未観測のまま保持。有限な依頼の必要役割は目的計画が別途束縛する"
            elif kind == "監査.要求" and value in _運用要求 and status == 値状態.留保:
                responsibility = _運用要求[value]
            elif kind == "監査.原理段階" and value == "未形成" and status == 値状態.未観測:
                responsibility = "原理形成は未実施。既存作用での依頼達成と区別"
            elif (kind == "保持.契約" and value == "全座標・全関係・不確実性・残差・由来・旧解釈を保持し、不可逆剪定しない"
                  and status == 値状態.確定):
                responsibility = "原IRを運用成果内に保持。局所ビューで原IRを上書きしない"
            elif kind == "暫定性.既定" and value == "原則暫定" and status == 値状態.確定:
                responsibility = "成立結果も資料・契約変更による再検査対象"
        elif (point.由来 == "公開HDS 構文化器 v1.1" and re.fullmatch(r"archv11:history:[0-9]+", point.座標ID)
              and point.種別 == "帰還.現行CognitiveWorld" and point.値状態 == 値状態.推定
              and type(point.内容) is str and re.fullmatch(r"cw:[0-9a-f]+", point.内容)):
            responsibility = "構文化世界の追跡ID。世界事実の確定座標ではない"
        if responsibility is None:
            local.append(point)
        else:
            delegated.add(point.座標ID)
            records.append({"座標": point.座標ID, "種別": point.種別, "状態": point.値状態.value,
                            "責任": responsibility, "解消済み認定": False})
    # 実際の意味関係が追加監査座標を参照する場合は、この局所ビューに降ろさない。
    if any(delegated.intersection((*rel.始点, *rel.終点)) for rel in ir.関係):
        raise ValueError("拡張座標を用いる意味関係は局所作用へ未接続")
    history = []
    for row in ir.意味作用履歴:
        losses = tuple(x for x in row.損失 if not (row.種別 == "開放多層監査射影" and x == _委譲))
        history.append(replace(row, 損失=losses))
    return replace(ir, 座標=tuple(local), 意味作用履歴=tuple(history)), tuple(records)


class 運用構文化器:
    def コンパイル(self, text, *, 文脈=None):
        構文化器 = 公開HDSコンパイラ()
        束 = 構文化器.コンパイル束(text, 文脈=文脈)
        self.コア入力 = 束.正本
        self.原IR = 束.意味IR
        local, self.責任対応 = 局所接続を構成(self.原IR)
        return local
