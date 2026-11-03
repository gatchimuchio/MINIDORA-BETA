from __future__ import annotations

import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.HDS観測計画 import HDS参照観測要求群, HDS追加観測要求群
from minidora.HDS参照 import HDS参照問合せ候補, HDS参照検索
from minidora.参照 import 参照記録


def _選択肢(*values: str) -> tuple[HDS座標, ...]:
    return tuple(HDS座標(f"選択肢:{chr(ord('A') + index)}", "目的.候補", value) for index, value in enumerate(values))


def _関係質問() -> HDSIR:
    return HDSIR(
        原文="Which molecule causes apoptosis under hypoxia?",
        正規化文="Which molecule causes apoptosis under hypoxia?",
        認知世界ID="observation-plan-test",
        座標=(
            HDS座標("u", "目的.未知始点", "molecule", 値状態.未観測),
            HDS座標("k", "対象.終点", "apoptosis"),
            HDS座標("search", "検索.英語正規化", "molecule causes apoptosis under hypoxia"),
            *_選択肢("Protein A", "Protein B", "Protein C", "Protein D"),
        ),
        関係=(
            HDS関係(
                "question",
                ("u",),
                ("k",),
                "因果",
                条件=("検索述語=causes", "不足位置=始点", "条件範囲=under hypoxia"),
                値状態=値状態.未観測,
            ),
        ),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核(),
        参照必須=True,
        種別="knowledge_query",
        入力言語="en",
    )


class _一般HitProvider:
    名称 = "general-hit"
    並列安全 = False

    def __init__(self) -> None:
        self.calls: list[str] = []

    def 検索(self, query: str, limit: int = 8):
        query = " ".join(query.split())
        self.calls.append(query)
        low = query.casefold()
        if low == "molecule causes apoptosis under hypoxia":
            return (参照記録("general", "apoptosis", "general hit", "fixture://general", self.名称),)
        if low in {"protein a", "protein b", "protein c", "protein d"}:
            label = low[-1]
            return (参照記録(f"fallback:{label}", low, low, f"fixture://{label}", self.名称),)
        return ()


class HDS参照観測要求試験(unittest.TestCase):
    def test_外部検索文脈は構文化器が明示アンカーとして保持する(self) -> None:
        ir = _関係質問()
        requests = HDS参照観測要求群(ir)
        関係要求群 = [x for x in requests if x.関係ID == "question" and x.段階 == "primary"]
        self.assertTrue(関係要求群)
        self.assertTrue(all("molecule causes apoptosis under hypoxia" in x.外部文脈アンカー for x in 関係要求群))
        self.assertTrue(all("molecule causes apoptosis under hypoxia" in x.外部検索表層.casefold() for x in 関係要求群))
        self.assertTrue(any(x.ID.startswith("検索表層:") and x.外部検索表層 == "molecule causes apoptosis under hypoxia" for x in requests))

    def test_複数関係を全保持し条件範囲を関係局所化する(self) -> None:
        ir = HDSIR(
            原文="synthetic",
            正規化文="synthetic",
            認知世界ID="multi-relation",
            座標=(
                HDS座標("u1", "目的.未知始点", "candidate", 値状態.未観測),
                HDS座標("k1", "対象.終点", "K1"),
                HDS座標("u2", "目的.未知始点", "candidate", 値状態.未観測),
                HDS座標("k2", "対象.終点", "K2"),
                *_選択肢("A", "B"),
            ),
            関係=(
                HDS関係("r1", ("u1",), ("k1",), "阻害", 条件=("検索述語=inhibit", "不足位置=始点", "条件範囲=under cold")),
                HDS関係("r2", ("u2",), ("k2",), "活性化", 条件=("検索述語=activate", "不足位置=始点", "条件範囲=under heat")),
            ),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核(),
            入力言語="en",
        )
        requests = [x for x in HDS参照観測要求群(ir) if x.候補ラベル and x.段階 == "primary"]
        self.assertEqual({x.関係ID for x in requests}, {"r1", "r2"})
        self.assertEqual(len(requests), 4)
        for request in requests:
            if request.関係ID == "r1":
                self.assertIn("under cold", request.外部検索表層)
                self.assertNotIn("under heat", request.外部検索表層)
            else:
                self.assertIn("under heat", request.外部検索表層)
                self.assertNotIn("under cold", request.外部検索表層)

    def test_generic問い適合はprimaryへ昇格しない(self) -> None:
        ir = HDSIR(
            原文="Which is correct?",
            正規化文="Which is correct?",
            認知世界ID="generic",
            座標=(
                HDS座標("u", "目的.未知始点", "選択肢", 値状態.未観測),
                HDS座標("k", "対象.問い本文", "entropy"),
                *_選択肢("A", "B"),
            ),
            関係=(
                HDS関係("generic", ("u",), ("k",), "問い適合", 条件=("検索述語=match", "不足位置=始点", "選択問題閉包=v0.1")),
            ),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核(),
            参照必須=True,
            種別="knowledge_query",
            入力言語="en",
        )
        requests = [x for x in HDS参照観測要求群(ir) if x.関係ID == "generic"]
        self.assertTrue(requests)
        self.assertTrue(all(x.段階 == "fallback" for x in requests))
        self.assertFalse(any(" match " in f" {x.外部検索表層.casefold()} " for x in requests))

    def test_一般1件hitでも必須観測未被覆なら縮退する(self) -> None:
        provider = _一般HitProvider()
        ir = _関係質問()
        requests = HDS参照観測要求群(ir)
        records = HDS参照検索(provider, ir, 観測要求=requests)
        self.assertIn("Protein A", provider.calls)
        self.assertIn("Protein B", provider.calls)
        conditions = {record.識別子: set(record.条件) for record in records}
        self.assertIn(("hds_observation_id", "関係:question:候補:A"), conditions["fallback:a"])
        self.assertIn(("hds_observation_id", "関係:question:候補:B"), conditions["fallback:b"])

    def test_primary必須観測未被覆なら同一実行で局所検証へ降下する(self) -> None:
        provider = _一般HitProvider()
        ir = _関係質問()
        requests = HDS参照観測要求群(ir)
        HDS参照検索(provider, ir, 観測要求=requests)
        局所表層 = {
            x.外部検索表層
            for x in requests
            if x.段階 == "fallback" and "局所検証" in x.provenance
        }
        self.assertTrue(局所表層)
        self.assertTrue(局所表層.intersection(provider.calls))

    def test_追加観測は局所検証から始めprimaryを初期には再発行しない(self) -> None:
        ir = _関係質問()
        requests = HDS参照観測要求群(ir)
        local = HDS追加観測要求群(
            requests,
            残差群=("HDS選択:観測不足",),
            世代=1,
        )
        self.assertTrue(local)
        self.assertTrue(all(x.段階 == "fallback" for x in local))
        self.assertTrue(all("局所検証" in x.provenance for x in local))
        self.assertFalse(any(x.段階 == "primary" for x in local))
        surfaces = tuple(x.外部検索表層.casefold() for x in local)
        self.assertIn("protein a causes apoptosis under hypoxia", surfaces)
        self.assertNotIn(
            "molecule causes apoptosis under hypoxia protein a causes apoptosis under hypoxia",
            surfaces,
        )

    def test_候補競合の第二世代は監査観測を先行する(self) -> None:
        ir = _関係質問()
        requests = list(HDS参照観測要求群(ir))
        requests.append(type(requests[0])(
            ID="監査表層:test",
            関係ID=None,
            関係種別=None,
            未知位置=None,
            既知端点=(),
            条件範囲=(),
            候補ラベル=None,
            候補表層=None,
            外部言語="en",
            外部検索表層="apoptosis failure conditions",
            必須被覆=False,
            外部文脈アンカー=(),
            段階="fallback",
            優先度=80,
            provenance=("監査.R_query",),
        ))
        planned = HDS追加観測要求群(
            tuple(requests),
            残差群=("HDS選択:候補競合",),
            世代=2,
        )
        self.assertTrue(planned)
        self.assertTrue(planned[0].ID.startswith("監査表層:"))

    def test_追加観測は各層を一度だけ通り最後にprimaryを一回だけ再観測する(self) -> None:
        requests = HDS参照観測要求群(_関係質問())
        計画群 = [
            HDS追加観測要求群(requests, 残差群=("HDS選択:観測不足",), 世代=世代)
            for 世代 in range(1, 8)
        ]
        有効 = [群 for 群 in 計画群 if 群]
        self.assertGreaterEqual(len(有効), 3)
        self.assertTrue(all(all(x.段階 == "fallback" for x in 群) for 群 in 有効[:-1]))
        self.assertTrue(all(x.段階 == "primary" for x in 有効[-1]))
        self.assertEqual(計画群[len(有効)], ())
        層署名 = [
            {(x.ID, x.段階, x.外部検索表層.casefold()) for x in 群}
            for 群 in 有効
        ]
        for index, 左 in enumerate(層署名):
            for 右 in 層署名[index+1:]:
                self.assertFalse(左 & 右)

    def test_既存単関係六query契約を維持する(self) -> None:
        queries = HDS参照問合せ候補(_関係質問())
        self.assertEqual(len(queries), 6)
        lowered = tuple(x.casefold() for x in queries)
        for 候補 in ("protein a", "protein b", "protein c", "protein d"):
            self.assertTrue(any(候補 in query and "causes apoptosis under hypoxia" in query for query in lowered))
        self.assertIn("molecule causes apoptosis under hypoxia", lowered)


if __name__ == "__main__":
    unittest.main()
