from __future__ import annotations

from .Crossref参照 import Crossref参照供給器
from .EuropePMC参照 import EuropePMC参照供給器
from .HTTP参照 import OpenAlex参照供給器, Wikipedia参照供給器
from .参照 import 参照供給器, 複合参照供給器


def 一般知識参照供給器(
    *,
    OpenAlex_API_key: str | None = None,
    EuropePMC有効: bool = True,
    EuropePMC同義語展開: bool = False,
    Crossref有効: bool = True,
    Crossref連絡先メール: str | None = None,
    Wikipedia言語: tuple[str, ...] = ("en",),
    timeout: float = 12.0,
    最大本文文字数: int = 12000,
    並列: bool = True,
    最大並列: int = 4,
) -> 参照供給器:
    """MINIDORA標準一般知識Rを構成する。

    OpenAlex keyは呼出側が明示した場合だけ利用する。Europe PMCは生命科学・医学を含む科学文献、
    Crossrefは分野横断の学術メタデータ、Wikipediaは百科事典参照として独立Providerにする。
    いずれも明示的に無効化可能で、複数Provider時は決定論的round-robin複合Rを返す。
    """
    providers: list[参照供給器] = []
    if OpenAlex_API_key is not None and str(OpenAlex_API_key).strip():
        providers.append(
            OpenAlex参照供給器(
                str(OpenAlex_API_key).strip(),
                timeout=timeout,
                最大本文文字数=最大本文文字数,
            )
        )

    if EuropePMC有効:
        providers.append(
            EuropePMC参照供給器(
                timeout=timeout,
                最大本文文字数=最大本文文字数,
                同義語展開=EuropePMC同義語展開,
            )
        )

    if Crossref有効:
        providers.append(
            Crossref参照供給器(
                timeout=timeout,
                最大本文文字数=最大本文文字数,
                連絡先メール=Crossref連絡先メール,
            )
        )

    seen_languages: set[str] = set()
    for raw_言語 in Wikipedia言語:
        言語 = str(raw_言語).strip().casefold()
        if not 言語 or 言語 in seen_languages:
            continue
        seen_languages.add(言語)
        providers.append(
            Wikipedia参照供給器(
                言語=言語,
                timeout=timeout,
                最大本文文字数=最大本文文字数,
            )
        )

    if not providers:
        raise ValueError("一般知識RにはOpenAlex・Europe PMC・Crossref・WikipediaのProviderが1つ以上必要")
    if len(providers) == 1:
        return providers[0]
    return 複合参照供給器(
        *providers,
        名称="MINIDORA一般知識R",
        並列=並列,
        最大並列=最大並列,
    )


__all__ = ["一般知識参照供給器"]
