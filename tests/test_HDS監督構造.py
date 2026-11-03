from __future__ import annotations

import inspect
import unittest

import minidora.hds介入制御 as control
import minidora.HDS模型射影 as projection
import minidora.HDS監督選択実行系 as supervised
import minidora.実行系 as 実行系


class HDS監督構造Test(unittest.TestCase):
    def test_active_実行系は旧outer_HDS_wrapperをimportしない(self):
        text = inspect.getsource(実行系)
        self.assertNotIn("実行系_HDS_v1", text)
        self.assertNotIn("実行系_hds_v1", text)
        self.assertNotIn("HDS駆動選択実行", text)
        self.assertNotIn("MINIDORAHDS判断主体", text)

    def test_supervised_実行系は通常MINIDORAを再構成しない(self):
        text = inspect.getsource(supervised)
        self.assertNotIn("HDS判断主体", text)
        self.assertNotIn("HDSMINIDORA模型評価", text)
        self.assertNotIn("HDS適応候補提案実行", text)
        self.assertNotIn("HDS能力経路V2候補提案実行", text)
        self.assertNotIn("hds既存能力resolver", text)
        self.assertNotIn("既存MINIDORA提案解決", text)

    def test_通常能力評価内部に後段HDS判断主体を置かない(self):
        text = inspect.getsource(projection)
        self.assertNotIn("from .hds判断主体", text)
        self.assertNotIn("HDS判断主体()", text)
        self.assertNotIn("MINIDORA出力化", text)
        self.assertNotIn('HDS_OUTPUT_ONLY_境界', text)

    def test_HDS制御は回答を生成しない(self):
        text = inspect.getsource(control.標準HDS介入制御)
        self.assertNotIn("回答ラベル", text)
        self.assertNotIn("候補得点", text)
        self.assertNotIn("COMMIT", text)

    def test_通常MINIDORA閉包は完全透過と明記される(self):
        text = inspect.getsource(supervised.HDS監督選択実行)
        self.assertIn("完全透過", text)
        self.assertIn("監督介入", text)
        self.assertNotIn("安全弁", text)
        self.assertNotIn("HDS_FEEDBACK_SAFETY_VALVE", inspect.getsource(supervised))


if __name__ == "__main__":
    unittest.main()
