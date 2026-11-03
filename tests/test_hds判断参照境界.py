from __future__ import annotations
import unittest
from minidora.HDS中間表現 import HDSIR,HDS実行核
from minidora.参照 import 参照記録
from minidora.hds判断参照境界 import HDS判断資料整列,HDS判断出典ID

def ir(text):
    return HDSIR(原文=text,正規化文=text,認知世界ID="ref-test",座標=(),関係=(),残差=(),意味作用履歴=(),実行核=HDS実行核())

class HDS判断参照境界試験(unittest.TestCase):
    def test_識別子を出典同一性としてそのまま保持する(self):
        r=参照記録("DOI:AbC","x","x","fixture://x","p")
        self.assertEqual(HDS判断出典ID(r),"DOI:AbC")

    def test_空識別子だけprovider由来へ縮退する(self):
        r=参照記録("","x","x","fixture://x","fixture")
        self.assertEqual(HDS判断出典ID(r),"fixture:fixture://x")

    def test_コンパイル失敗を除外してID信頼IRの添字を揃える(self):
        refs=(
            参照記録("bad","bad","bad","fixture://bad","fixture",信頼=0.9),
            参照記録("good","good","good","fixture://good","fixture",信頼=0.25),
        )
        bundle=HDS判断資料整列(refs,(ValueError("bad"),ir("ok")),lambda x:x)
        self.assertEqual(bundle.失敗数,1)
        self.assertEqual(tuple(x.原文 for x in bundle.IR群),("ok",))
        self.assertEqual(bundle.出典ID群,("good",))
        self.assertEqual(bundle.信頼群,(0.25,))
        self.assertEqual(tuple(x.識別子 for x in bundle.成功記録群),("good",))

if __name__=="__main__":
    unittest.main()
