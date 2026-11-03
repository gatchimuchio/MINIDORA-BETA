from __future__ import annotations

# v0.4の候補・関係評価実装は能力核の互換部品として再利用する。
from .模型 import *  # noqa: F401,F403
from .模型 import __all__ as _能力模型公開名
from .言語確率法則 import *  # noqa: F401,F403
from .言語確率法則 import __all__ as _厳密LM公開名
from .規定参照 import *  # noqa: F401,F403
from .規定参照 import __all__ as _規定公開名
from .能力状態差循環 import (
    MINIDORA能力状態差模型核,
    標準能力模型核,
    能力作用記録,
    能力状態差記録,
    能力後続利用記録,
    能力作用構造,
    能力候補状態差,
)

MINIDORA能力模型核 = MINIDORA能力状態差模型核
# v0.5の標準能力核は状態差起動型。旧 `.模型.標準模型核` は履歴互換実装として残す。
標準模型核 = 標準能力模型核

__all__ = list(dict.fromkeys((
    *_能力模型公開名,
    *_厳密LM公開名,
    *_規定公開名,
    "MINIDORA能力模型核",
    "MINIDORA能力状態差模型核",
    "標準能力模型核",
    "標準模型核",
    "能力作用記録",
    "能力状態差記録",
    "能力後続利用記録",
    "能力作用構造",
    "能力候補状態差",
)))
