from __future__ import annotations

import unittest

from minidora.hds参照計画 import (
    HDS参照索引圧縮,
    HDS参照計画作成,
    HDS参照計画再利用可能,
    HDS参照計画消費,
    HDS参照計画無効化,
    HDS参照計画適用,
)
from minidora.hds並列作業状態 import (
    HDS並列作業状態生成,
    HDS並列状態読書混合,
    HDS制約混合行列,
)
from minidora.hds多時間尺度 import (
    HDS大域再照合判断,
    HDS先行草案検証,
    HDS異種入力射影,
    HDS阻害回復方針,
)
from minidora.構文化由来 import 構文化還元検索
from minidora.参照 import 参照記録


class GLM構文化還元試験(unittest.TestCase):
    def _refs(self, count: int = 8) -> tuple[参照記録, ...]:
        return tuple(
            参照記録(
                識別子=f"R-{i}",
                対象="GLM還元試験",
                内容=f"source {i} evidence retrieval state index local global {i}",
                由来="fixture",
                供給器="test",
                信頼=0.9,
            )
            for i in range(count)
        )

    def test_索引圧縮は正本証拠を変更しない(self) -> None:
        refs = self._refs(8)
        before = tuple((r.識別子, r.内容) for r in refs)
        index = HDS参照索引圧縮(refs, bucket幅=4)
        self.assertEqual(index.正本参照数, 8)
        self.assertEqual(len(index.bucket群), 2)
        self.assertEqual(index.bucket群[0].参照ID群, ("R-0", "R-1", "R-2", "R-3"))
        self.assertEqual(tuple((r.識別子, r.内容) for r in refs), before)

    def test_参照計画は有限leaseで再利用し正本を再読する(self) -> None:
        refs = self._refs(5)
        plan = HDS参照計画作成("retrieval state", refs, 利用上限=4)
        self.assertTrue(HDS参照計画再利用可能(plan, "retrieval state", refs))
        loaded = HDS参照計画適用(plan, refs)
        self.assertEqual({r.識別子 for r in loaded}, {r.識別子 for r in refs})
        for _ in range(4):
            plan = HDS参照計画消費(plan)
        self.assertFalse(plan.有効)
        self.assertFalse(HDS参照計画再利用可能(plan, "retrieval state", refs))

    def test_参照正本変更と明示無効化で再利用不可(self) -> None:
        refs = self._refs(4)
        plan = HDS参照計画作成('証拠', refs)
        changed = list(refs)
        changed[0] = 参照記録(
            識別子="R-0",
            対象="GLM還元試験",
            内容="changed evidence",
            由来="fixture",
            供給器="test",
            信頼=0.9,
        )
        self.assertFalse(HDS参照計画再利用可能(plan, '証拠', tuple(changed)))
        invalid = HDS参照計画無効化(plan, "evidence changed")
        self.assertFalse(invalid.有効)
        self.assertEqual(invalid.無効理由, "evidence changed")

    def test_4lane制約混合は決定論的で行列を正規化する(self) -> None:
        raw = (
            (4.0, 1.0, 0.0, 0.0),
            (1.0, 3.0, 1.0, 0.0),
            (0.0, 1.0, 3.0, 1.0),
            (0.0, 0.0, 1.0, 4.0),
        )
        matrix = HDS制約混合行列(raw, 反復回数=40)
        self.assertEqual(matrix, HDS制約混合行列(raw, 反復回数=40))
        for row in matrix:
            self.assertAlmostEqual(sum(row), 1.0, places=6)
        for j in range(4):
            self.assertAlmostEqual(sum(matrix[i][j] for i in range(4)), 1.0, places=6)

        状態 = HDS並列作業状態生成((
            {"a": 1.0},
            {"b": 1.0},
            {"c": 1.0},
            {"d": 1.0},
        ))
        mixed = HDS並列状態読書混合(状態, raw, raw, 反復回数=40)
        self.assertEqual(mixed.lane数, 4)
        self.assertEqual(mixed.revision, 2)
        self.assertTrue(all({"a", "b", "c", "d"}.issubset(lane.辞書()) for lane in mixed.lane群))

    def test_局所大域再照合の分離(self) -> None:
        refs = self._refs(2)
        plan = HDS参照計画作成('状態', refs)
        self.assertFalse(HDS大域再照合判断(1, 参照計画=plan).大域再照合)
        self.assertTrue(HDS大域再照合判断(4, 参照計画=plan).大域再照合)
        self.assertTrue(HDS大域再照合判断(1, 参照計画=plan, 証拠不足=True).大域再照合)
        self.assertTrue(HDS大域再照合判断(1, 参照計画=plan, 矛盾数=1).大域再照合)

    def test_先行草案は不成立prefixからrollbackする(self) -> None:
        結果 = HDS先行草案検証(("A", "B", "C"), lambda prefix: "C" not in prefix)
        self.assertEqual(結果.採用prefix, ("A", "B"))
        self.assertEqual(結果.却下位置, 2)
        self.assertTrue(結果.rollback)
        self.assertEqual(結果.検証回数, 3)

    def test_阻害回復は証拠状態変化で参照計画再構築を選ぶ(self) -> None:
        recovery = HDS阻害回復方針(("NO_PROVENANCE_PROOF", '証拠_GAP'))
        self.assertEqual(recovery.作用, 'REBUILD_取得_PLAN')
        self.assertTrue(recovery.参照計画無効化)
        self.assertFalse(recovery.Jへ留保)

    def test_異種入力は適合器後の表象だけを共通境界へ渡す(self) -> None:
        value = HDS異種入力射影("image", {"objects": 3}, 出典ID="vision:1")
        self.assertEqual(value.種別, "image")
        self.assertEqual(value.出典ID, "vision:1")
        self.assertEqual(value.表象, {"objects": 3})

    def test_K3とGLMを同一由来台帳で検索できる(self) -> None:
        k3 = 構文化還元検索(模型="K3")
        glm = 構文化還元検索(模型="GLM")
        self.assertGreaterEqual(len(k3), 6)
        self.assertGreaterEqual(len(glm), 10)
        self.assertTrue(any("参照計画" in row.MINIDORA還元 for row in glm))
        self.assertTrue(any("専門作用" in row.MINIDORA還元 for row in k3))


if __name__ == "__main__":
    unittest.main()
