"""新HTMLの明示見出しだけを既存ATX節へ渡す。実取得/旧保存の再変換はしない。"""
from dataclasses import fields
from hashlib import sha256
import unittest

from minidora.公開本文取得 import 本文を復号, 取得本文
from minidora.HDS運用.意味資料 import 資料を区分


URL = "https://example.test/heading"


def 復号(HTML, MIME="text/html; charset=utf-8"):
    return 本文を復号(URL, (URL,), {"content-type": MIME}, HTML.encode("utf-8"))


class HTML見出し境界試験(unittest.TestCase):
    def test_原h1からh6を同じ深さのATXに保持(self):
        HTML = "前文" + "".join(f"<h{i}>範囲{i}</h{i}><p>本文{i}。</p>" for i in range(1, 7))
        資料 = 復号(HTML)
        self.assertEqual(資料.本文, "前文\n" + "\n".join(
            f"{'#' * i} 範囲{i}\n本文{i}。" for i in range(1, 7)))

    def test_階層と兄弟節の原本文sliceを既存区分へ渡す(self):
        HTML = ("<h1>概要</h1><p>全体。</p><h2>条件</h2><p>限定。</p>"
                "<h3><b>詳細</b></h3><p>詳細本文。</p><h2>別条件</h2><p>別本文。</p>")
        資料 = 復号(HTML)
        節 = 資料を区分(資料.本文)
        self.assertEqual([[x["見出し"] for x in s["階層"]] for s in 節],
                         [["概要"], ["概要", "条件"], ["概要", "条件", "詳細"], ["概要", "別条件"]])
        self.assertEqual([資料.本文[s["開始"]:s["終了"]].strip() for s in 節],
                         ["# 概要\n全体。", "## 条件\n限定。", "### 詳細\n詳細本文。", "## 別条件\n別本文。"])

    def test_正しく閉じたinlineと文字参照を原順で保持(self):
        HTML = '<h2>材料<span>と<strong>性質</strong></span> &amp; <a href="/x">条件</a></h2><p>本文。</p>'
        資料 = 復号(HTML)
        self.assertEqual(資料.本文, "## 材料と性質 & 条件\n本文。")
        self.assertEqual(資料.転送SHA256, sha256(HTML.encode()).hexdigest())
        self.assertEqual(資料.転送バイト数, len(HTML.encode()))
        self.assertEqual(資料.本文SHA256, sha256(資料.本文.encode()).hexdigest())
        self.assertNotEqual(資料.転送SHA256, 資料.本文SHA256)

    def test_同名の普通本文へ見出し型を付さない(self):
        資料 = 復号("<p>条件</p><div>条件</div><h2>条件</h2><p>条件</p>")
        self.assertEqual(資料.本文, "条件\n条件\n## 条件\n条件")

    def test_未閉鎖の見出しとinlineは原文字に留める(self):
        for HTML, 期待 in [("<h2>条件", "条件"), ("<h2><span>重要条件</span>", "重要条件"),
                           ("<h2>前<span>未閉</h2><p>後文</p>", "前未閉\n後文")]:
            with self.subTest(HTML=HTML):
                self.assertEqual(復号(HTML).本文, 期待)

    def test_タグ不整合と外側閉鎖を見出しの証明にしない(self):
        for HTML in ("<h2>条件</h3>", "<h2><span><b>条件</span></b></h2>",
                     "<section><h2>条件</section></h2>"):
            with self.subTest(HTML=HTML):
                self.assertEqual(復号(HTML).本文, "条件")

    def test_入れ子見出しを複数の節へ分割しない(self):
        資料 = 復号("<h2>外側<h3>内部</h3>末尾</h2><p>後文</p>")
        self.assertEqual(資料.本文, "外側\n内部\n末尾\n後文")
        self.assertEqual(資料を区分(資料.本文), [{"開始": 0, "終了": len(資料.本文), "階層": []}])

    def test_ブロックと複雑多行の原文字を見出し化しない(self):
        for HTML, 期待 in [("<h2>甲<br>乙</h2>", "甲\n乙"),
                           ("<h2>甲\n乙</h2>", "甲\n乙"),
                           ("<h2>甲<div>乙</div>丙</h2>", "甲\n乙\n丙")]:
            with self.subTest(HTML=HTML):
                self.assertEqual(復号(HTML).本文, 期待)

    def test_抑止内の見出しと抑止を含む見出しに型を付さない(self):
        資料 = 復号("<template><h2>除外域</h2></template><h2>甲<script>code()</script>乙</h2><p>後文</p>")
        self.assertEqual(資料.本文, "甲乙\n後文")
        self.assertEqual(set(資料.除外要素), {"template", "script"})

    def test_空alt画像も見出しの単一文字行証明へ補完しない(self):
        for 属性 in ('alt=""', 'alt="図"', ""):
            with self.subTest(属性=属性):
                資料 = 復号(f"<h2>甲<img {属性}>乙</h2><p>後文</p>")
                self.assertEqual(資料.本文, "甲乙\n後文")
                self.assertEqual("img" in 資料.除外要素, 属性 != 'alt=""')

    def test_ルビを含む見出しは既存文字処理を保持し型を補わない(self):
        for 注記, 期待 in [("<rt>よみ</rt>", "字よみ"),
                           ('<rt style="font-size:0px">（</rt>', "字")]:
            with self.subTest(注記=注記):
                資料 = 復号(f"<h2><ruby>字{注記}</ruby></h2><p>後文</p>")
                self.assertEqual(資料.本文, 期待 + "\n後文")

    def test_空見出しは作らず閉じた後続見出しは保持(self):
        資料 = 復号("<h2> </h2><h3/>前文<h4>後続</h4><p>本文</p>")
        self.assertEqual(資料.本文, "前文\n#### 後続\n本文")

    def test_plainとmarkdownを新HTML型へ無言変換しない(self):
        原文 = "<h2>条件</h2>\n条件\n## 既存見出し\n旧本文"
        for MIME in ("text/plain; charset=utf-8", "text/markdown; charset=utf-8"):
            with self.subTest(MIME=MIME):
                資料 = 復号(原文, MIME)
                self.assertEqual(資料.本文, 原文)
                self.assertEqual(資料.本文SHA256, sha256(原文.encode()).hexdigest())

    def test_取得本文の既存API欄を増減しない(self):
        self.assertEqual([項.name for 項 in fields(取得本文)],
                         ["要求URL", "最終URL", "題名", "本文", "取得時刻", "内容種別", "文字コード",
                          "転送バイト数", "転送SHA256", "本文SHA256", "経路", "除外要素"])


if __name__ == "__main__":
    unittest.main()
