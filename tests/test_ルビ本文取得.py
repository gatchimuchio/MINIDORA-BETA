"""ルビの代替表示を本文へ二重混入させず、通常の括弧や数学記号を保全する。"""
from hashlib import sha256
import unittest

from minidora.公開本文取得 import 本文を復号


def 復号(本文, 内容種別="text/html; charset=utf-8"):
    URL = "https://example.test/ruby"
    return 本文を復号(URL, (URL,), {"content-type": 内容種別}, 本文.encode())


class ルビ本文契約試験(unittest.TestCase):
    def test_標準rpの代替括弧を除外して実タグを記録(self):
        資料 = 復号("<p><ruby>山<rp>（</rp><rt>やま</rt><rp>）</rp></ruby>の説明。</p>")
        self.assertEqual(資料.本文, "山やまの説明。")
        self.assertIn("rp", 資料.除外要素)
        self.assertNotIn("rt", 資料.除外要素)

    def test_実型のspan括弧は残し零サイズrtだけを除外(self):
        HTML = ('<p><ruby class="yomigana"><rb>対象</rb><span class="rp">（</span>'
                '<rt style="display:inline;font-size:inherit">たいしょう</rt>、'
                '<rt style="display:inline;font-size:0px">（</rt></ruby>英語）とは記載。</p>')
        資料 = 復号(HTML)
        self.assertEqual(資料.本文, "対象（たいしょう、英語）とは記載。")
        self.assertIn("rt", 資料.除外要素)
        self.assertNotIn("rp", 資料.除外要素)
        self.assertEqual(資料.転送SHA256, sha256(HTML.encode()).hexdigest())
        self.assertEqual(資料.本文SHA256, sha256(資料.本文.encode()).hexdigest())
        self.assertNotEqual(資料.転送SHA256, 資料.本文SHA256)

    def test_本文の不整合括弧も勝手に補修しない(self):
        資料 = 復号("<p>対象（条件未閉。引用「甲」。対象(113年）。</p>")
        self.assertEqual(資料.本文, "対象（条件未閉。引用「甲」。対象(113年）。")
        self.assertEqual(資料.除外要素, ())

    def test_spanのrpクラスは標準rpタグへ読み替えない(self):
        資料 = 復号('<p><span class="rp">（</span>通常本文）</p>')
        self.assertEqual(資料.本文, "（通常本文）")
        self.assertEqual(資料.除外要素, ())

    def test_可視ルビと未対応スタイルは保持(self):
        for 形式 in ("font-size:12px", "font-size:inherit", "font-size:var(--size)", "font-size:calc(0px)", "font-size:0em", "font-size:0"):
            with self.subTest(形式=形式):
                資料 = 復号(f'<ruby>字<rt style="{形式}">読み</rt></ruby>')
                self.assertEqual(資料.本文, "字読み")
                self.assertNotIn("rt", 資料.除外要素)

    def test_単一零サイズ宣言の空白大小文字を扱う(self):
        for 形式 in ("font-size: 0px;", "FONT-SIZE:0PX", "color:red; font-size:0px", "font-size:0px !important"):
            with self.subTest(形式=形式):
                資料 = 復号(f'<ruby>字<rt style="{形式}">（</rt></ruby>本文。')
                self.assertEqual(資料.本文, "字本文。")
                self.assertIn("rt", 資料.除外要素)

    def test_重複宣言と字体全体上書きは推測せず保全(self):
        for 形式 in ("font-size:0px;font-size:12px", "font-size:12px;font-size:0px", "font-size:0px;font:12px serif", "font-size:0px;all:unset"):
            with self.subTest(形式=形式):
                資料 = 復号(f'<ruby>字<rt style="{形式}">読み</rt></ruby>')
                self.assertEqual(資料.本文, "字読み")
                self.assertNotIn("rt", 資料.除外要素)

    def test_ルビ外のrtは字体だけで除外しない(self):
        資料 = 復号('<p>本文<rt style="font-size:0px">実記載</rt>末尾。</p>')
        self.assertEqual(資料.本文, "本文実記載末尾。")
        self.assertNotIn("rt", 資料.除外要素)

    def test_入れ子ルビと子構造のある注記は保全(self):
        資料 = 復号('<ruby>甲<ruby>乙<rt style="font-size:0px"><span>代替</span></rt></ruby>'
                    '<rt style="font-size:0px">（</rt></ruby><p>後続</p><rt style="font-size:0px">外部</rt>')
        self.assertEqual(資料.本文, "甲乙代替\n後続\n外部")
        self.assertIn("rt", 資料.除外要素)

    def test_閉鎖を観測しない注記は保全(self):
        for 内部 in ("（未閉", "（<span>未閉", "（未閉</ruby>"):
            with self.subTest(内部=内部):
                資料 = 復号('<p>前文</p><ruby>甲<rt style="font-size:0px">' + 内部)
                self.assertEqual(資料.本文, "前文\n甲（未閉")
                self.assertNotIn("rt", 資料.除外要素)

    def test_子要素のサイズ上書きと全注記原文字を保全(self):
        資料 = 復号('<ruby>対象<rt style="font-size:0px">先行'
                    '<span style="font-size:12px">屋外では使用しない</span>後続</rt></ruby>本文。')
        self.assertEqual(資料.本文, "対象先行屋外では使用しない後続本文。")
        self.assertNotIn("rt", 資料.除外要素)

    def test_重複style属性は曖昧な原文字として保全(self):
        for 属性 in ('style="font-size:12px" style="font-size:0px"',
                     'style="font-size:0px" style="font-size:12px"',
                     'style="font-size:0px" style="font-size:0px"'):
            with self.subTest(属性=属性):
                資料 = 復号(f'<ruby>対象<rt {属性}>屋外では使用しない</rt></ruby>本文。')
                self.assertEqual(資料.本文, "対象屋外では使用しない本文。")
                self.assertNotIn("rt", 資料.除外要素)

    def test_子注記を入れ子の非表示へ再解釈しない(self):
        資料 = 復号('<ruby>対象<rt style="font-size:0px">先行'
                    '<rt style="font-size:0px">重要条件</rt>後続</rt></ruby>本文。')
        self.assertEqual(資料.本文, "対象先行重要条件後続本文。")
        self.assertNotIn("rt", 資料.除外要素)

    def test_MathMLの関数適用記号を削除しない(self):
        資料 = 復号('<p>数式<math><mi>sin</mi><mo>\u2061</mo><mi>x</mi></math>の記載。</p>')
        self.assertEqual(資料.本文, "数式sin\u2061xの記載。")
        self.assertEqual(資料.除外要素, ())

    def test_プレーン資料をHTMLルビへ無言変換しない(self):
        原文 = '<ruby>対象<rt style="font-size:0px">（</rt></ruby>'
        資料 = 復号(原文, "text/plain; charset=utf-8")
        self.assertEqual(資料.本文, 原文)
        self.assertEqual(資料.除外要素, ())


if __name__ == "__main__":
    unittest.main()
