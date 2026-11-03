from __future__ import annotations

from types import SimpleNamespace
import unittest
from unittest.mock import patch

from minidora.HDS既存能力継承 import HDS既存能力選択評価, HDS既存能力直接反証評価
from minidora.HDS選択実行系 import HDS選択実行結果


class _模型結果:
    def __init__(self, label: str, scores: dict[str, int]):
        self.参照最有力候補ID = label
        self._得点 = dict(scores)

    def 参照候補辞書(self):
        return dict(self._得点)


def _結果(
    状態: str,
    label: str | None = None,
    *,
    理由=(),
    K証拠=0,
    K3根拠=0,
    模型=None,
):
    return HDS選択実行結果(
        状態=状態,
        回答ラベル=label,
        回答内容=(label.lower() if label is not None else None),
        理由=tuple(理由),
        K3結果=(SimpleNamespace(根拠事実数=K3根拠) if K3根拠 else None),
        候補コンパイル数=0,
        資料コンパイル数=0,
        資料コンパイル失敗数=0,
        K追加事実数=0,
        K証拠事実数=K証拠,
        K証拠阻害事実数=0,
        MINIDORA模型結果=模型,
    )


class HDS既存能力継承試験(unittest.TestCase):
    def test_正本formal閉包済みは旧能力を起動せず完全透過(self):
        正本 = _結果("APPROVE", "A", 模型=_模型結果("A", {"A": 2, "B": 0}))
        with patch(
            "minidora.HDS既存能力継承.HDS選択推論実行",
            return_value=正本,
        ) as 選択, patch(
            "minidora.HDS既存能力継承.HDS適応候補提案実行"
        ) as 能力:
            out = HDS既存能力選択評価(
                object(), (), コンパイル=lambda x: x, 模型核=object(), 基礎能力核=object()
            )
        self.assertIs(out, 正本)
        self.assertEqual(選択.call_count, 1)
        能力.assert_not_called()

    def test_formal未閉包ならK3既存能力を継承(self):
        正本 = _結果("SUSPEND", 理由=("NO_GUESS",))
        旧補助 = _結果("APPROVE", "A", K3根拠=2)
        能力 = _結果("SUSPEND")
        with patch(
            "minidora.HDS既存能力継承.HDS選択推論実行",
            side_effect=(正本, 旧補助),
        ), patch(
            "minidora.HDS既存能力継承.HDS適応候補提案実行",
            return_value=能力,
        ):
            out = HDS既存能力選択評価(
                object(), (), コンパイル=lambda x: x, 模型核=object(), 基礎能力核=object()
            )
        self.assertEqual(out.状態, "APPROVE")
        self.assertEqual(out.回答ラベル, "A")
        self.assertIn("HDS_EXISTING_CAPABILITY_INHERITED", out.理由)
        self.assertIn("HDS_EXISTING_SOURCE:K3", out.理由)

    def test_既存能力同士が競合したらHDSに勝者を選ばせない(self):
        正本 = _結果("SUSPEND")
        旧補助 = _結果("APPROVE", "A", K3根拠=1)
        能力 = _結果(
            "PROPOSE",
            "B",
            模型=_模型結果("B", {"A": 0, "B": 2}),
        )
        with patch(
            "minidora.HDS既存能力継承.HDS選択推論実行",
            side_effect=(正本, 旧補助),
        ), patch(
            "minidora.HDS既存能力継承.HDS適応候補提案実行",
            return_value=能力,
        ):
            out = HDS既存能力選択評価(
                object(), (), コンパイル=lambda x: x, 模型核=object(), 基礎能力核=object()
            )
        self.assertEqual(out.状態, "SUSPEND")
        self.assertIsNone(out.回答ラベル)
        self.assertIn("HDS_EXISTING_CAPABILITY_CONFLICT_OR_UNCLOSED", out.理由)

    def test_直接反証は2独立proof以上だけ返す(self):
        強反証 = _結果("APPROVE", "B", 理由=("DIRECTED_関係_VERIFIED",), K3根拠=2)
        with patch("minidora.HDS既存能力継承.HDS選択推論実行", return_value=強反証):
            out = HDS既存能力直接反証評価(
                object(), (), コンパイル=lambda x: x, 基礎能力核=object(), 基準ラベル="A"
            )
        self.assertIs(out, 強反証)

        弱反証 = _結果("APPROVE", "B", 理由=("DIRECTED_関係_VERIFIED",), K3根拠=1)
        with patch("minidora.HDS既存能力継承.HDS選択推論実行", return_value=弱反証):
            out = HDS既存能力直接反証評価(
                object(), (), コンパイル=lambda x: x, 基礎能力核=object(), 基準ラベル="A"
            )
        self.assertIsNone(out)

    def test_直接反証は同じ基準ラベルを上書き候補にしない(self):
        同一 = _結果("APPROVE", "A", 理由=("DIRECTED_関係_VERIFIED",), K3根拠=3)
        with patch("minidora.HDS既存能力継承.HDS選択推論実行", return_value=同一):
            out = HDS既存能力直接反証評価(
                object(), (), コンパイル=lambda x: x, 基礎能力核=object(), 基準ラベル="A"
            )
        self.assertIsNone(out)

    def test_直接関係検証は能力模型競合より既存強証拠として優先(self):
        正本 = _結果("SUSPEND")
        旧補助 = _結果(
            "APPROVE",
            "C",
            理由=("DIRECTED_関係_VERIFIED",),
            K3根拠=1,
        )
        能力 = _結果(
            "PROPOSE",
            "B",
            模型=_模型結果("B", {"B": 3, "C": 0}),
        )
        with patch(
            "minidora.HDS既存能力継承.HDS選択推論実行",
            side_effect=(正本, 旧補助),
        ), patch(
            "minidora.HDS既存能力継承.HDS適応候補提案実行",
            return_value=能力,
        ):
            out = HDS既存能力選択評価(
                object(), (), コンパイル=lambda x: x, 模型核=object(), 基礎能力核=object()
            )
        self.assertEqual(out.状態, "APPROVE")
        self.assertEqual(out.回答ラベル, "C")
        self.assertIn("HDS_EXISTING_SOURCE:DIRECT_関係", out.理由)


if __name__ == "__main__":
    unittest.main()
