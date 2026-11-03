from __future__ import annotations

import unittest
from urllib.parse import parse_qs, urlparse

from minidora.HTTP参照 import OpenAlex参照供給器, Wikipedia参照供給器
from minidora.参照 import 複合参照供給器


class _FakeHTTP:
    def __init__(self) -> None:
        self.urls: list[str] = []

    def __call__(self, url: str, headers, timeout: float):
        self.urls.append(url)
        parsed = urlparse(url)
        if parsed.netloc == "api.openalex.org":
            params = parse_qs(parsed.query)
            if params.get("api_key") != ["test-key"]:
                raise AssertionError("api_key missing")
            return {
                "results": [
                    {
                        "id": "https://openalex.org/W1",
                        "doi": "https://doi.org/10.1/example",
                        "display_name": "Catalysis by ProteinX",
                        "publication_year": 2024,
                        "abstract_inverted_index": {"ProteinX": [0], "supports": [1], "catalysis": [2]},
                        "is_retracted": False,
                        "relevance_score": 12.3,
                    },
                    {
                        "id": "https://openalex.org/W2",
                        "display_name": "Retracted claim",
                        "abstract_inverted_index": {"wrong": [0]},
                        "is_retracted": True,
                    },
                ]
            }
        if parsed.path.endswith("/search/page"):
            return {
                "pages": [
                    {
                        "id": 123,
                        "key": "ProteinX",
                        "title": "ProteinX",
                        "description": "Protein family",
                        "excerpt": "ProteinX <span class=\"searchmatch\">catalysis</span>",
                    }
                ]
            }
        if parsed.path.endswith("/page/ProteinX/with_html"):
            return {
                "id": 123,
                "title": "ProteinX",
                "html": "<h1>ProteinX</h1><p>ProteinX supports <b>catalysis</b>.</p><script>ignored()</script>",
            }
        raise AssertionError(f"unexpected URL: {url}")


class HTTP参照供給器試験(unittest.TestCase):
    def test_OpenAlex検索を参照記録へ変換する(self) -> None:
        fake = _FakeHTTP()
        provider = OpenAlex参照供給器("test-key", JSON取得=fake)
        records = provider.検索("ProteinX catalysis", 4)

        self.assertEqual(len(records), 1)
        record = records[0]
        self.assertEqual(record.識別子, "openalex:W1")
        self.assertIn("ProteinX supports catalysis", record.内容)
        self.assertEqual(record.時点, "2024")
        self.assertEqual(record.由来, "https://doi.org/10.1/example")
        self.assertTrue(any("search=ProteinX+catalysis" in url for url in fake.urls))
        self.assertTrue(any("select=" in url for url in fake.urls))

    def test_OpenAlexはAPI_keyなしを拒否する(self) -> None:
        with self.assertRaises(ValueError):
            OpenAlex参照供給器("")

    def test_Wikipedia検索後に本文HTMLを取得してtext化する(self) -> None:
        fake = _FakeHTTP()
        provider = Wikipedia参照供給器(言語="en", JSON取得=fake)
        records = provider.検索("ProteinX catalysis", 2)

        self.assertEqual(len(records), 1)
        record = records[0]
        self.assertEqual(record.識別子, "wikipedia:en:123")
        self.assertIn("ProteinX supports catalysis.", record.内容)
        self.assertNotIn("ignored", record.内容)
        self.assertTrue(any("/search/page?" in url for url in fake.urls))
        self.assertTrue(any("/page/ProteinX/with_html" in url for url in fake.urls))

    def test_Wikipedia本文は複数query間で再取得しない(self) -> None:
        fake = _FakeHTTP()
        provider = Wikipedia参照供給器(言語="en", JSON取得=fake)
        first = provider.検索("ProteinX catalysis", 2)
        second = provider.検索("ProteinX transport", 2)

        self.assertTrue(first and second)
        detail_calls = [url for url in fake.urls if "/page/ProteinX/with_html" in url]
        search_calls = [url for url in fake.urls if "/search/page?" in url]
        self.assertEqual(len(detail_calls), 1)
        self.assertEqual(len(search_calls), 2)
        self.assertEqual(provider.本文cache件数, 1)

    def test_Wikipedia本文取得失敗はcacheせず次回再取得する(self) -> None:
        class _FlakyWikipedia(_FakeHTTP):
            def __init__(self):
                super().__init__()
                self.detail_attempts = 0

            def __call__(self, url: str, headers, timeout: float):
                if "/page/ProteinX/with_html" in url:
                    self.detail_attempts += 1
                    if self.detail_attempts == 1:
                        raise OSError("temporary detail failure")
                return super().__call__(url, headers, timeout)

        fake = _FlakyWikipedia()
        provider = Wikipedia参照供給器(言語="en", JSON取得=fake)
        first = provider.検索("ProteinX catalysis", 2)
        self.assertTrue(first)
        self.assertIsNotNone(provider.最後のエラー)
        second = provider.検索("ProteinX catalysis", 2)
        self.assertTrue(second)
        self.assertEqual(fake.detail_attempts, 2)
        self.assertEqual(provider.本文cache件数, 1)
        self.assertIsNone(provider.最後のエラー)

    def test_Wikipedia本文取得だけ失敗なら一覧資料を縮退保持する(self) -> None:
        class _部分失敗HTTP(_FakeHTTP):
            def __call__(self, url: str, headers, timeout: float):
                if "/page/ProteinX/with_html" in url:
                    raise OSError("detail down")
                return super().__call__(url, headers, timeout)

        provider = Wikipedia参照供給器(言語="en", JSON取得=_部分失敗HTTP())
        records, diagnosis = provider.検索診断("ProteinX catalysis", 2)

        self.assertEqual(diagnosis.状態, "縮退")
        self.assertTrue(records)
        self.assertIn("ProteinX", records[0].内容)
        self.assertIn(("wikipedia_detail", "degraded"), records[0].条件)
        self.assertLess(records[0].信頼, 1.0)

    def test_一Provider障害でも複合Rは他Providerを返す(self) -> None:
        def failing(url, headers, timeout):
            raise OSError("network down")

        wiki = Wikipedia参照供給器(JSON取得=_FakeHTTP())
        openalex = OpenAlex参照供給器("test-key", JSON取得=failing)
        combined = 複合参照供給器(openalex, wiki)
        records = combined.検索("ProteinX catalysis", 4)

        self.assertTrue(records)
        self.assertTrue(all(record.供給器.startswith("Wikipedia") for record in records))
        self.assertIsNotNone(openalex.最後のエラー)


if __name__ == "__main__":
    unittest.main()
