from __future__ import annotations

from dataclasses import dataclass

from .模型 import 模型結果


@dataclass(frozen=True, slots=True)
class MINIDORA出力:
    """MINIDORA計算主体Cが後段HDSへ渡す唯一の判断入力。"""

    状態: str
    候補ID: str | None
    候補差: tuple[tuple[str, int], ...]
    参照候補差: tuple[tuple[str, int], ...]
    参照同率候補ID: tuple[str, ...]
    検査点数: int
    再作用回数: int
    終端遍歴数: int


@dataclass(frozen=True, slots=True)
class HDS判定門結果:
    判定門: str
    状態: str
    理由: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS判断結果:
    """後段HDSの局所判断結果。

    APPROVEだけが外へ出る。HOLD / REJECTはいずれも外部出力を持たずSILENTで閉じる。
    """

    状態: str
    選択候補ID: str | None
    外部出力状態: str
    運用状態: str
    判定門: tuple[HDS判定門結果, ...]
    理由: tuple[str, ...]



def MINIDORA出力化(結果: 模型結果) -> MINIDORA出力:
    '模型結果を後段HDSの入力へ固定する。\n\n    knowledge 選択肢の正式出力は参照由来の一意な正差だけとし、一般表層差へ代替経路しない。\n    '

    候補 = 結果.参照最有力候補ID
    状態 = "OUTPUT" if 候補 is not None else "NO_OUTPUT"
    return MINIDORA出力(
        状態=状態,
        候補ID=候補,
        候補差=tuple(sorted(結果.候補辞書().items())),
        参照候補差=tuple(sorted(結果.参照候補辞書().items())),
        参照同率候補ID=tuple(結果.参照同率候補ID),
        検査点数=len(結果.検査点),
        再作用回数=int(結果.統計.再作用回数),
        終端遍歴数=int(結果.統計.終端遍歴数),
    )


class HDS判断主体:
    'MINIDORA出力だけを採用・留保・拒否する後段HDS。\n\n    Question / 候補 / 資料 / 参照を受け取らない。再検索・再計算・差し戻しも行わない。\n    それらはMINIDORA単体LLMの責務外であり、必要なら上位AGI全体HDSが担う。\n    '

    版 = "v2-output-only"

    def 判断(self, 出力: MINIDORA出力) -> HDS判断結果:
        gates: list[HDS判定門結果] = [
            HDS判定門結果("入力境界", "PASS", ("MINIDORA_OUTPUT_ONLY",)),
        ]

        if 出力.状態 != "OUTPUT" or 出力.候補ID is None:
            gates.append(HDS判定門結果("出力存在", "HOLD", ("MINIDORA_OUTPUT_ABSENT",)))
            gates.append(HDS判定門結果("終端", "SILENT", ("NO_FEEDBACK_LOOP",)))
            return HDS判断結果(
                "HOLD",
                None,
                "SILENT",
                "HOLD",
                tuple(gates),
                ("HDS_OUTPUT_HOLD", "MINIDORA_OUTPUT_ABSENT", "SILENT", "NO_FEEDBACK_LOOP"),
            )

        ref_scores = dict(出力.参照候補差)
        候補 = 出力.候補ID
        if len(ref_scores) != len(出力.参照候補差) or 候補 not in ref_scores:
            gates.append(HDS判定門結果("出力整合", "REJECT", ("MALFORMED_OUTPUT",)))
            gates.append(HDS判定門結果("終端", "SILENT", ("NO_FEEDBACK_LOOP",)))
            return HDS判断結果(
                "REJECT",
                None,
                "SILENT",
                "REJECT",
                tuple(gates),
                ("HDS_OUTPUT_REJECTED", "MINIDORA_OUTPUT_MALFORMED", "SILENT", "NO_FEEDBACK_LOOP"),
            )

        maximum = max(ref_scores.values(), default=0)
        top = tuple(sorted(cid for cid, score in ref_scores.items() if score == maximum))
        if maximum <= 0 or len(top) != 1 or top[0] != 候補 or 出力.参照同率候補ID:
            gates.append(HDS判定門結果("出力整合", "REJECT", ("OUTPUT_NOT_UNIQUE_POSITIVE",)))
            gates.append(HDS判定門結果("終端", "SILENT", ("NO_FEEDBACK_LOOP",)))
            return HDS判断結果(
                "REJECT",
                None,
                "SILENT",
                "REJECT",
                tuple(gates),
                ("HDS_OUTPUT_REJECTED", "MINIDORA_OUTPUT_INCONSISTENT", "SILENT", "NO_FEEDBACK_LOOP"),
            )

        gates.append(HDS判定門結果("出力整合", "PASS", ("UNIQUE_POSITIVE_OUTPUT",)))
        gates.append(HDS判定門結果("終端", "OUTPUT", ("NO_FEEDBACK_LOOP",)))
        return HDS判断結果(
            "APPROVE",
            候補,
            "OUTPUT",
            "COMMIT",
            tuple(gates),
            ("HDS_OUTPUT_APPROVED", "MINIDORA_OUTPUT_ACCEPTED", "NO_FEEDBACK_LOOP"),
        )


__all__ = [
    "MINIDORA出力",
    "MINIDORA出力化",
    "HDS判定門結果",
    "HDS判断結果",
    "HDS判断主体",
]
