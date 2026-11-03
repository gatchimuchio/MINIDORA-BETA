from __future__ import annotations
from hashlib import sha256
import json
from typing import Protocol
from urllib.parse import quote
from urllib.request import Request, urlopen
from .型 import 参照資料, 能力結果
from .要約 import 汎用要約モジュール

知識版 = '参照-knowledge-v1'

class 知識供給器(Protocol):
    def 検索(self, query: str, limit: int = 3) -> tuple[参照資料, ...]: ...

class Wikipedia知識供給器:
    def __init__(self, timeout: float = 8.0): self.timeout=timeout
    def _get(self,url: str) -> dict:
        req=Request(url,headers={"User-Agent":"MINIDORA/0.5 product-demo"})
        with urlopen(req,timeout=self.timeout) as r: return json.loads(r.read(1_000_000).decode("utf-8"))
    def 検索(self, query: str, limit: int = 3) -> tuple[参照資料, ...]:
        api="https://ja.wikipedia.org/w/api.php?action=query&list=search&format=json&utf8=1&srlimit="+str(limit)+"&srsearch="+quote(query)
        資料=self._get(api); out=[]
        for item in 資料.get("query",{}).get("search",[]):
            title=item.get("title","")
            if not title: continue
            detail=self._get("https://ja.wikipedia.org/api/rest_v1/page/summary/"+quote(title.replace(" ","_")))
            extract=str(detail.get("extract","")).strip(); page=str(detail.get("content_urls",{}).get("desktop",{}).get("page", ""))
            key=sha256((title+page).encode()).hexdigest()[:20]
            out.append(参照資料(key,title,"Wikipedia",page,None,extract))
        return tuple(out)

class 固定知識供給器:
    def __init__(self, items: tuple[参照資料,...]): self.items=items
    def 検索(self, query: str, limit: int = 3) -> tuple[参照資料,...]: return self.items[:limit]

class 知識参照モジュール本体:
    版=知識版
    def __init__(self,provider:知識供給器): self.provider=provider; self.summarizer=汎用要約モジュール()
    def 実行(self, query:str)->能力結果:
        try: refs=self.provider.検索(query,3)
        except Exception as exc: return 能力結果(False,"",保留理由=f"知識参照失敗:{type(exc).__name__}")
        if not refs: return 能力結果(False,"",保留理由="参照知識が見つからない")
        情報源="\n".join(f"{r.題名}: {r.本文}" for r in refs if r.本文)
        if not 情報源: return 能力結果(False,"",保留理由="参照本文が空")
        要約=self.summarizer.実行(情報源,行数=4,参照=refs)
        return 能力結果(True,要約.本文,根拠=tuple(r.識別子 for r in refs),参照=refs,データ={"検索語":query,"参照数":len(refs)})
