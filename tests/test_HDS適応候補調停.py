from __future__ import annotations

import unittest

from minidora.HDS選択実行系 import HDS選択実行結果
from minidora.hds適応候補調停 import HDS適応候補調停


def _結果(
    状態: str,
    label: str | None,
    content: str | None,
    *,
    cross_updates: int = 0,
    specialist: int = 0,
    reasons: tuple[str, ...] = ("TEST",),
) -> HDS選択実行結果:
    return HDS選択実行結果(
        状態,
        label,
        content,
        reasons,
        None,
        2,
        1,
        0,
        0,
        0,
        0,
        候補横断更新数=cross_updates,
        専門作用起動数=specialist,
    )


class HDS適応候補調停試験(unittest.TestCase):
    def test_raw候補横断更新だけでは能力提案を優先しない(self) -> None:
        primary = _結果("PROPOSE", "A", "能力", cross_updates=3, reasons=('状態_DELTA_CROSS_UPDATE',))
        base = _結果("APPROVE", "B", "基礎", reasons=("BASE",))

        selected = HDS適応候補調停(primary, base)

        self.assertEqual(selected.状態, "PROPOSE")
        self.assertEqual(selected.回答ラベル, "B")
        self.assertIn("HDS_ADAPTIVE_BASE_SELECTED", selected.理由)
        self.assertNotIn("HDS_ADAPTIVE_PRIMARY_SELECTED", selected.理由)

    def test_local_view実観測変化なら能力提案を優先する(self) -> None:
        primary = _結果("PROPOSE", "A", "能力", reasons=("C_LOCAL_VIEW_RECHECK_SELECTED",))
        base = _結果("APPROVE", "B", "基礎", reasons=("BASE",))

        selected = HDS適応候補調停(primary, base)

        self.assertEqual(selected.状態, "PROPOSE")
        self.assertEqual(selected.回答ラベル, "A")
        self.assertIn('OBSERVATION_状態_CHANGE_SUPPORTED', selected.理由)
        self.assertIn("HDS_ADAPTIVE_PRIMARY_SELECTED", selected.理由)

    def test_専門作用実消費なら能力提案を優先する(self) -> None:
        primary = _結果(
            "PROPOSE",
            "A",
            "能力",
            specialist=1,
            reasons=('HDS_作用_DELTA_CONSUMED',),
        )
        base = _結果("APPROVE", "B", "基礎", reasons=("BASE",))

        selected = HDS適応候補調停(primary, base)

        self.assertEqual(selected.回答ラベル, "A")
        self.assertIn("HDS_ADAPTIVE_PRIMARY_SELECTED", selected.理由)

    def test_実観測変化が無ければ閉じた基礎経路を提案へ落とす(self) -> None:
        primary = _結果("PROPOSE", "A", "能力", reasons=("PRIMARY",))
        base = _結果("APPROVE", "B", "基礎", reasons=("BASE",))

        selected = HDS適応候補調停(primary, base)

        self.assertEqual(selected.状態, "PROPOSE")
        self.assertEqual(selected.回答ラベル, "B")
        self.assertIn("HDS_ADAPTIVE_BASE_SELECTED", selected.理由)
        self.assertIn('候補_GENERATION_HAS_NO_COMMIT_AUTHORITY', selected.理由)

    def test_実観測変化も基礎閉包も無ければ単独primaryを救済しない(self) -> None:
        primary = _結果("PROPOSE", "A", "能力", reasons=("PRIMARY",))
        base = _結果("SUSPEND", None, None, reasons=("BASE_SUSPEND",))

        selected = HDS適応候補調停(primary, base)

        self.assertEqual(selected.状態, "SUSPEND")
        self.assertIsNone(selected.回答ラベル)
        self.assertIsNone(selected.回答内容)
        self.assertIn("PRIMARY_WITHOUT_OBSERVATION_CHANGE_NOT_COMMITTED", selected.理由)

    def test_能力経路がSUSPENDでも基礎閉包は提案として残す(self) -> None:
        primary = _結果("SUSPEND", None, None, reasons=("PRIMARY_SUSPEND",))
        base = _結果("APPROVE", "C", "基礎", reasons=("BASE",))

        selected = HDS適応候補調停(primary, base)

        self.assertEqual(selected.状態, "PROPOSE")
        self.assertEqual(selected.回答ラベル, "C")


if __name__ == "__main__":
    unittest.main()
