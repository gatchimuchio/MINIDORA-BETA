"""限定Wikipedia検索・JSON安全輸送・実本文来歴の契約。人工HTTPは性能証拠にしない。"""
from dataclasses import asdict
from hashlib import sha256
import json
import unittest
from unittest.mock import patch
from urllib.parse import urlsplit

from minidora.HTTP参照 import Wikipedia参照供給器
from minidora.公開本文取得 import 公開JSON取得器, JSON待機要求, 本文取得失敗, 本文を復号
from minidora.知識取得 import 知識取得要求
from minidora.製品版.検索 import Wikipedia検索供給器, Wikipedia知識取得器


API = "https://ja.wikipedia.org/w/rest.php/v1/search/page?q=%E4%B8%BB%E9%A1%8C&limit=1"
記事URL = "https://ja.wikipedia.org/wiki/%E5%88%9D%E8%A6%8B%E4%B8%BB%E9%A1%8C"


def 取得器(**条件):
    return 公開JSON取得器(許可ホスト=Wikipedia検索供給器.許可ホスト,
                          許可経路=Wikipedia検索供給器.許可経路, **条件)


def JSON応答(値):
    return 200, {"content-type": "application/json; charset=utf-8"}, json.dumps(値, ensure_ascii=False).encode()


def 検索応答():
    return JSON応答({"pages": [{"id": 903, "key": "初見主題", "title": "初見主題",
                               "excerpt": "スニペットの偽値999", "description": ""}]})


class JSON取得契約試験(unittest.TestCase):
    def test_実要求URLと転送元を本文に偽装しない(self):
        転送先 = API + "&redirect=1"
        with patch.object(公開JSON取得器, "_一回取得", side_effect=[
            (302, {"location": 転送先}, b""), JSON応答({"pages": []})]):
            結果 = 取得器().取得(API)
        self.assertEqual((結果.要求URL, 結果.最終URL), (API, 転送先))
        self.assertEqual(結果.経路, (API, 転送先))
        self.assertNotIn("本文", asdict(結果))

    def test_固定ホストと経路を接続前に検査する(self):
        禁止 = ("https://en.wikipedia.org.evil.test/w/rest.php/v1/search/page",
                "https://ja.wikipedia.org/wiki/A", "https://ja.wikipedia.org/w/api.php",
                "https://127.0.0.1/w/rest.php/v1/search/page")
        with patch.object(公開JSON取得器, "_一回取得") as 呼出:
            for url in 禁止:
                with self.subTest(url=url), self.assertRaises(本文取得失敗):
                    取得器().取得(url)
            呼出.assert_not_called()

    def test_許可ホスト同士でもJSONは別ホスト転送を禁止(self):
        for 転送先 in ("https://en.wikipedia.org/w/rest.php/v1/search/page?q=A",
                       "https://ja.wikipedia.org/wiki/A"):
            with self.subTest(転送先=転送先), patch.object(公開JSON取得器, "_一回取得", return_value=(
                302, {"location": 転送先}, b"")) as 呼出:
                with self.assertRaises(本文取得失敗):
                    取得器().取得(API)
                self.assertEqual(呼出.call_count, 1)

    def test_JSON復号故障は値として返さない(self):
        for 資料 in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":1e999}', b'[]', b'{"x":', b'\xff'):
            with self.subTest(資料=資料), patch.object(公開JSON取得器, "_一回取得", return_value=(
                200, {"content-type": "application/json"}, 資料)), self.assertRaises(本文取得失敗):
                取得器().取得(API)

    def test_JSON内容種別圧縮文字コードサイズを検査(self):
        for 見出し in ({"content-type": "text/html"},
                        {"content-type": "application/json", "content-encoding": "gzip"},
                        {"content-type": "application/json; charset=cp932"}):
            with self.subTest(見出し=見出し), patch.object(公開JSON取得器, "_一回取得", return_value=(
                200, 見出し, b'{}')), self.assertRaises(本文取得失敗):
                取得器().取得(API)
        with patch.object(公開JSON取得器, "_一回取得", return_value=JSON応答({"pages": []})), self.assertRaises(本文取得失敗):
            取得器(最大バイト数=2).取得(API)

    def test_JSONのDNS検査とTLSの失敗を迂回しない(self):
        for 故障 in (本文取得失敗("非公開アドレスへの接続禁止"), TimeoutError()):
            with self.subTest(故障=故障), patch("minidora.公開本文取得._公開アドレス", side_effect=故障), \
                 patch("minidora.公開本文取得.socket.create_connection") as 接続:
                with self.assertRaises(type(故障)):
                    取得器().取得(API)
                接続.assert_not_called()

    def test_RetryAfter中は別言語や次の検索でも再通信しない(self):
        供給器 = Wikipedia検索供給器()
        with patch.object(公開JSON取得器, "_一回取得", return_value=(429, {"retry-after": "600"}, b"")) as 呼出:
            最初の候補, 最初の来歴 = 供給器.検索来歴付き("初見主題", 1)
            次の候補, 次の来歴 = 供給器.検索来歴付き("別の主題", 1)
        self.assertEqual((最初の候補, 次の候補), ((), ()))
        self.assertEqual(呼出.call_count, 1)
        for 来歴 in (*最初の来歴, *次の来歴):
            self.assertEqual(来歴["通信"][0]["種類"], "JSON待機要求")
            self.assertGreater(来歴["通信"][0]["待機要求秒"], 590)


class Wikipedia接続契約試験(unittest.TestCase):
    def test_検索標題とURLだけを実APIから返し候補本文は空(self):
        with patch.object(公開JSON取得器, "_一回取得", return_value=検索応答()) as 呼出:
            候補, 来歴 = Wikipedia検索供給器().検索来歴付き("初見主題", 1)
        self.assertEqual(呼出.call_count, 1)
        self.assertEqual(候補[0].URL, 記事URL)
        self.assertEqual(候補[0].本文, "")
        通信 = 来歴[0]["通信"][0]
        self.assertEqual(urlsplit(通信["要求URL"]).path, "/w/rest.php/v1/search/page")
        self.assertNotIn("with_html", str(来歴))
        self.assertEqual(通信["経路"], (通信["要求URL"],))

    def test_空検索と通信故障を分けて保持(self):
        for 応答, 期待 in ((JSON応答({"pages": []}), "空"), ((403, {}, b""), "失敗")):
            with self.subTest(期待=期待), patch.object(公開JSON取得器, "_一回取得", return_value=応答):
                候補, 来歴 = Wikipedia検索供給器().検索来歴付き("未発見の主題", 1)
            self.assertEqual(候補, ())
            self.assertEqual([記録["状態"] for 記録 in 来歴], [期待, 期待])
            self.assertEqual(len(来歴), 2)

    def test_未許可なら検索JSONと記事取得を両方呼ばない(self):
        with patch.object(公開JSON取得器, "_一回取得") as 検索, \
             patch("minidora.公開本文取得.公開本文取得器._一回取得") as 記事:
            結果 = Wikipedia知識取得器().実行(知識取得要求("初見主題", ("値",)))
        self.assertFalse(結果.成立)
        検索.assert_not_called()
        記事.assert_not_called()

    def test_記事本文だけ採用し検索と本文の来歴を分離(self):
        本文 = 本文を復号(記事URL, (記事URL,), {"content-type": "text/html"},
                            "<title>初見主題</title><p>値は47である。</p>".encode())
        実行器 = Wikipedia知識取得器()
        with patch.object(公開JSON取得器, "_一回取得", return_value=検索応答()), \
             patch.object(実行器.本文, "取得", return_value=本文) as 読取:
            結果 = 実行器.実行(知識取得要求("初見主題", ("値",), 最大候補数=1), 外部読取許可=True)
        self.assertTrue(結果.成立, 結果.保留理由)
        self.assertEqual(結果.本文, "値は47である。")
        self.assertNotIn("999", str(結果))
        読取.assert_called_once_with(記事URL)
        self.assertEqual(結果.データ["資料"][0]["経路"], (記事URL,))
        self.assertIn("/search/page?", 結果.データ["検索来歴"][0]["取得"][0]["通信"][0]["要求URL"])
        self.assertEqual(結果.データ["意味的事実検証"], "未実施")
        記録 = dict(結果.データ)
        封印 = 記録.pop("記録SHA256")
        self.assertEqual(封印, sha256(json.dumps(記録, ensure_ascii=False, sort_keys=True, allow_nan=False).encode()).hexdigest())

    def test_API成功でも記事サイズ失敗なら本文成功にしない(self):
        実行器 = Wikipedia知識取得器()
        with patch.object(公開JSON取得器, "_一回取得", return_value=検索応答()), \
             patch.object(実行器.本文, "取得", side_effect=本文取得失敗("本文サイズ上限")):
            結果 = 実行器.実行(知識取得要求("初見主題", ("値",), 最大候補数=1, 最大検索回数=1), 外部読取許可=True)
        self.assertFalse(結果.成立)
        self.assertEqual(結果.本文, "")
        self.assertEqual(結果.データ["資料数"], 0)
        self.assertEqual(結果.データ["検索来歴"][0]["取得"][0]["状態"], "取得")

    def test_従来Wikipedia参照の本文取得は変更しない(self):
        要求URL = []

        def JSONを取得(url, 見出し, 時間):
            要求URL.append(url)
            if "/search/page?" in url:
                return {"pages": [{"id": 903, "key": "初見主題", "title": "初見主題", "excerpt": "資料"}]}
            return {"html": "<p>初見主題の既存本文。</p>"}

        候補 = Wikipedia参照供給器(言語="ja", JSON取得=JSONを取得).検索("初見主題", 1)
        self.assertEqual(len(要求URL), 2)
        self.assertIn("/with_html", 要求URL[-1])
        self.assertIn("既存本文", 候補[0].内容)


if __name__ == "__main__":
    unittest.main()
