from __future__ import annotations

import unittest
from urllib.parse import urlparse

from minidora.HTTP参照 import Wikipedia参照供給器


class _LongWikipediaHTTP:
    def __call__(self, url: str, headers, timeout: float):
        parsed = urlparse(url)
        if parsed.path.endswith("/search/page"):
            return {
                "pages": [
                    {
                        "id": 999,
                        "key": "LongTopic",
                        "title": "LongTopic",
                        "description": "A long technical article",
                        "excerpt": "General overview",
                    }
                ]
            }
        if parsed.path.endswith("/page/LongTopic/with_html"):
            noise = "".join(
                f"<p>Background paragraph {i} discusses unrelated history and chronology.</p>"
                for i in range(40)
            )
            return {
                "id": 999,
                "title": "LongTopic",
                "html": (
                    "<h1>LongTopic</h1>"
                    + noise
                    + "<p>The rare catalyst marker directly activates the target pathway under hypoxia.</p>"
                ),
            }
        raise AssertionError(url)


class Wikipedia関連段落試験(unittest.TestCase):
    def test_記事後半のquery適合段落を先頭切捨てで失わない(self) -> None:
        provider = Wikipedia参照供給器(
            JSON取得=_LongWikipediaHTTP(),
            最大本文文字数=220,
        )
        records = provider.検索("rare catalyst marker hypoxia", 1)

        self.assertEqual(len(records), 1)
        content = records[0].内容
        self.assertIn("rare catalyst marker", content)
        self.assertIn("hypoxia", content)
        self.assertLessEqual(len(content), 220)
        self.assertNotIn("Background paragraph 39", content)


if __name__ == "__main__":
    unittest.main()
