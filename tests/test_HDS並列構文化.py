from __future__ import annotations

from threading import Lock, get_ident
import time
import unittest

from minidora import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態, ミニドラ, 参照記録, 実行状態, 要求


def _ir(text: str, coords: tuple[HDS座標, ...], relations: tuple[HDS関係, ...] = (), *, refs: bool = False) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID="compile-parallel-test",
        座標=coords,
        関係=relations,
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        参照必須=refs,
        種別='knowledge_選択肢',
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
        手順=None,
    )


def _question() -> HDSIR:
    return _ir(
        "What does Alpha use?",
        (
            HDS座標("alpha", "対象.実体", "Alpha"),
            HDS座標('選択肢:A', "目的.候補", "engine"),
            HDS座標('選択肢:B', "目的.候補", "stone"),
            HDS座標('未知', "目的.未知終点", "entity", 値状態.未観測),
        ),
        (HDS関係(
            "question-use", ("alpha",), ('未知',), "使用",
            条件=("検索述語=use", "不足位置=終点", "英日意味射影=v0.5"),
            値状態=値状態.未観測,
        ),),
        refs=True,
    )


def _候補(text: str) -> HDSIR:
    return _ir(text, (HDS座標('候補', "対象.実体", text),))


def _資料() -> HDSIR:
    return _ir(
        "Alpha uses engine.",
        (HDS座標("alpha", "対象.実体", "Alpha"), HDS座標("engine", "対象.実体", "engine")),
        (HDS関係("use", ("alpha",), ("engine",), "使用", 条件=("検索述語=use",)),),
    )


class _Provider:
    名称 = "fixture"
    並列安全 = True

    def 検索(self, query: str, limit: int = 8):
        return tuple(
            参照記録(f"doc:{i}", "Alpha", "Alpha uses engine.", f"fixture://{i}", self.名称)
            for i in range(min(limit, 4))
        )


class _構文化器:
    def __init__(self, parallel: bool) -> None:
        self.並列安全 = parallel
        self._lock = Lock()
        self.calls: list[tuple[str, int]] = []

    def コンパイル(self, 入力: str, **kwargs) -> HDSIR:
        with self._lock:
            self.calls.append((入力, get_ident()))
        if 入力 != "What does Alpha use?":
            time.sleep(0.02)
        if 入力 == "What does Alpha use?":
            return _question()
        if 入力 in {"engine", "stone"}:
            return _候補(入力)
        if 入力 == "Alpha uses engine.":
            return _資料()
        raise ValueError(入力)


class HDS構文化器並列試験(unittest.TestCase):
    def test_並列安全構文化器は選択肢_資料を複数threadで処理する(self) -> None:
        構文化器 = _構文化器(True)
        実行系 = ミニドラ(_Provider(), HDSコンパイラ_=構文化器)
        結果 = 実行系.実行(要求("What does Alpha use?"))

        self.assertEqual(結果.採否.状態, 実行状態.合格, 結果.採否.理由)
        worker_threads = {
            thread_id
            for text, thread_id in 構文化器.calls
            if text != "What does Alpha use?"
        }
        self.assertGreaterEqual(len(worker_threads), 2)

    def test_未宣言構文化器は従来どおり逐次処理する(self) -> None:
        構文化器 = _構文化器(False)
        実行系 = ミニドラ(_Provider(), HDSコンパイラ_=構文化器)
        結果 = 実行系.実行(要求("What does Alpha use?"))

        self.assertEqual(結果.採否.状態, 実行状態.合格, 結果.採否.理由)
        worker_threads = {
            thread_id
            for text, thread_id in 構文化器.calls
            if text != "What does Alpha use?"
        }
        self.assertEqual(len(worker_threads), 1)


if __name__ == "__main__":
    unittest.main()
