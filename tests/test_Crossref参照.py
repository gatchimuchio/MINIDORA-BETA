from __future__ import annotations

import unittest
from urllib.parse import parse_qs, urlparse

from minidora.Crossref参照 import Crossref参照供給器


class _FakeCrossref:
    def __init__(self) -> None:
        self.urls: list[str] = []
        self.calls = 0

    def __call__(self, url: str, headers, timeout: float):
        self.calls += 1
        self.urls.append(url)
        parsed = urlparse(url)
        if parsed.netloc != "api.crossref.org" or parsed.path != "/works":
            raise AssertionError(url)
        params = parse_qs(parsed.query)
        if params.get("query") != ["Mott Gurney equation"]:
            raise AssertionError("query missing")
        if params.get("rows") != ["4"]:
            raise AssertionError("rows missing")
        return {
            "message": {
                "items": [
                    {
                        "DOI": "10.1000/example",
                        "title": ["Space-charge-limited current in semiconductors"],
                        "abstract": "<jats:p>The Mott-Gurney law assumes a trap-free single-carrier device.</jats:p>",
                        "container-title": ["Journal of Example Physics"],
                        "type": "journal-article",
                        "published-online": {"date-parts": [[2024, 7, 1]]},
                    },
                    {
                        "DOI": "10.1000/title-only",
                        "title": ["A title-only record"],
                        "type": "journal-article",
                        "published": {"date-parts": [[2023]]},
                    },
                ]
            }
        }


class Crossref参照供給器試験(unittest.TestCase):
    def test_abstractをmarkup除去して参照記録へ変換する(self) -> None:
        fake = _FakeCrossref()
        provider = Crossref参照供給器(JSON取得=fake)
        records = provider.検索("Mott Gurney equation", 4)

        self.assertEqual(len(records), 2)
        first = records[0]
        self.assertEqual(first.識別子, "doi:10.1000/example")
        self.assertIn("trap-free single-carrier", first.内容)
        self.assertNotIn("jats:p", first.内容)
        self.assertEqual(first.信頼, provider.ABSTRACT信頼)
        self.assertEqual(first.時点, "2024-7-1")
        self.assertIn(('canonical_情報源', "doi:10.1000/example"), first.条件)
        self.assertIn(('証拠_範囲', "abstract"), first.条件)

    def test_title_onlyは低信頼度(self) -> None:
        provider = Crossref参照供給器(JSON取得=_FakeCrossref())
        records = provider.検索("Mott Gurney equation", 4)
        self.assertEqual(records[1].識別子, "doi:10.1000/title-only")
        self.assertEqual(records[1].信頼, provider.TITLE_ONLY信頼)
        self.assertLess(records[1].信頼, records[0].信頼)

    def test_同一queryはcacheしてHTTPを再実行しない(self) -> None:
        fake = _FakeCrossref()
        provider = Crossref参照供給器(JSON取得=fake)
        first = provider.検索("Mott Gurney equation", 4)
        second = provider.検索("Mott Gurney equation", 4)
        self.assertEqual(first, second)
        self.assertEqual(fake.calls, 1)
        self.assertEqual(provider.cache件数, 1)

    def test_API障害は空集合へ閉じる(self) -> None:
        def failing(url, headers, timeout):
            raise OSError("network down")

        provider = Crossref参照供給器(JSON取得=failing)
        self.assertEqual(provider.検索("Mott Gurney equation", 4), ())
        self.assertIn("OSError", provider.最後のエラー or "")


if __name__ == "__main__":
    unittest.main()
