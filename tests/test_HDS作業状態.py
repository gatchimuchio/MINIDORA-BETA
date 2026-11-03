from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器, HDS証拠事実, HDS証拠状態複製
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.HDS作業状態 import HDS一時証拠統合, HDS作業状態構築, HDS寄与関門再照合
from minidora.K3機能 import K3相当能力核


def _弱関係(*, negative: bool = False) -> HDSIR:
    conditions = ("極性=否定",) if negative else ()
    return HDSIR(
        原文="Alpha does not use engine." if negative else "Alpha uses engine.",
        正規化文="Alpha does not use engine." if negative else "Alpha uses engine.",
        認知世界ID='作業-状態-test',
        座標=(
            HDS座標("alpha", "対象.実体", "Alpha", 値状態=値状態.留保),
            HDS座標("engine", "対象.実体", "engine", 値状態=値状態.留保),
        ),
        関係=(HDS関係("use", ("alpha",), ("engine",), "作用", 条件=conditions, 値状態=値状態.留保),),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        種別='knowledge_資料',
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
        手順=None,
    )


class HDS作業状態試験(unittest.TestCase):
    def _模型核(self, *rows: tuple[str, HDSIR]) -> K3相当能力核:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        for 情報源, ir in rows:
            適合器.投入(ir, provenance=("fixture", "fixture://" + 情報源, 情報源), 信頼係数=1.0)
        return 模型核

    def test_二独立出典の未解関係は一時証拠化するがcanonical_Kは増やさない(self) -> None:
        模型核 = self._模型核(("a", _弱関係()), ("b", _弱関係()))
        before_k = len(getattr(模型核.K, "_facts", {}))
        状態 = HDS作業状態構築(模型核)
        temporary = HDS寄与関門再照合(状態)

        self.assertGreaterEqual(len(temporary), 2)
        self.assertEqual(状態.統計.作業関係K昇格数, 0)
        self.assertGreater(状態.統計.作業関係再利用数, 0)
        self.assertEqual(len(getattr(模型核.K, "_facts", {})), before_k)

        clone = 模型核.clone()
        HDS証拠状態複製(模型核, clone)
        clone_before_k = len(getattr(clone.K, "_facts", {}))
        added = HDS一時証拠統合(clone, temporary)
        self.assertEqual(added, len(temporary))
        self.assertEqual(len(getattr(clone.K, "_facts", {})), clone_before_k)
        self.assertGreater(len(HDS証拠事実(clone)), len(HDS証拠事実(模型核)))

    def test_一出典だけでは一時証拠化しない(self) -> None:
        模型核 = self._模型核(("a", _弱関係()))
        状態 = HDS作業状態構築(模型核)
        temporary = HDS寄与関門再照合(状態)
        self.assertEqual(temporary, ())
        self.assertEqual(状態.統計.作業関係K昇格数, 0)

    def test_反対関係が存在すれば再照合で一時証拠化しない(self) -> None:
        模型核 = self._模型核(
            ("a", _弱関係()),
            ("b", _弱関係()),
            ("c", _弱関係(negative=True)),
        )
        状態 = HDS作業状態構築(模型核)
        temporary = HDS寄与関門再照合(状態)
        positive_relations = [fact for fact in temporary if fact.predicate == 'hds_関係_作用' and fact.極性]
        self.assertEqual(positive_relations, [])
        self.assertGreater(状態.統計.作業関係再検証後破棄数, 0)

    def test_検査点は同じ入力で決定論的(self) -> None:
        模型核1 = self._模型核(("a", _弱関係()), ("b", _弱関係()))
        模型核2 = self._模型核(("a", _弱関係()), ("b", _弱関係()))
        状態1 = HDS作業状態構築(模型核1)
        状態2 = HDS作業状態構築(模型核2)
        HDS寄与関門再照合(状態1)
        HDS寄与関門再照合(状態2)
        self.assertEqual(
            [(cp.検査点ID, cp.段階) for cp in 状態1.検査点],
            [(cp.検査点ID, cp.段階) for cp in 状態2.検査点],
        )


if __name__ == "__main__":
    unittest.main()
