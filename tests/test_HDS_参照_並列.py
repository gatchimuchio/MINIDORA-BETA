from __future__ import annotations

from threading import Barrier, Lock, get_ident
import unittest

from minidora import HDSIR, HDS実行核, HDS座標, 参照記録
from minidora.HDS参照 import HDS参照検索


def _ir() -> HDSIR:
    return HDSIR(
        原文="What does Alpha use?",
        正規化文="What does Alpha use?",
        認知世界ID="parallel-query-test",
        座標=(
            HDS座標("alpha", "対象.実体", "Alpha"),
            HDS座標("use", "関係.述語表層", "use"),
            HDS座標('選択肢:A', "目的.候補", "engine"),
            HDS座標('選択肢:B', "目的.候補", "stone"),
        ),
        関係=(),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("参照回答"),
        参照必須=True,
        種別="knowledge_query",
    )


class _ParallelSafeProvider:
    並列安全 = True
    名称 = "safe"

    def __init__(self, barrier: Barrier) -> None:
        self.barrier = barrier
        self.thread_ids: set[int] = set()
        self.calls: list[str] = []
        self._lock = Lock()
        self._call_count = 0

    def 検索(self, query: str, limit: int = 8):
        with self._lock:
            self._call_count += 1
            call_no = self._call_count
            self.thread_ids.add(get_ident())
            self.calls.append(query)
        # 最初の2 callだけ同期し、奇数個の後続queryを単独Barrier待ちにしない。
        if call_no <= 2:
            self.barrier.wait(timeout=1.0)
        字句 = str(abs(hash(query)))
        return (参照記録(字句, query, query, "fixture://" + 字句, self.名称),)


class _SequentialOnlyProvider:
    名称 = "legacy-custom"

    def __init__(self) -> None:
        self.thread_ids: set[int] = set()

    def 検索(self, query: str, limit: int = 8):
        self.thread_ids.add(get_ident())
        return (参照記録(query, query, query, "fixture://" + query, self.名称),)


class HDS参照Query並列試験(unittest.TestCase):
    def test_並列安全Providerではqueryを同時開始する(self) -> None:
        provider = _ParallelSafeProvider(Barrier(2))
        records = HDS参照検索(provider, _ir(), 上限=4, 最大問合せ並列=2)
        self.assertTrue(records)
        self.assertGreaterEqual(len(provider.thread_ids), 2)

    def test_並列安全宣言のないProviderは逐次互換経路を使う(self) -> None:
        provider = _SequentialOnlyProvider()
        records = HDS参照検索(provider, _ir(), 上限=4, 最大問合せ並列=4)
        self.assertTrue(records)
        self.assertEqual(len(provider.thread_ids), 1)


if __name__ == "__main__":
    unittest.main()
