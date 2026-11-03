"""意味署名と境界検査。objectの住所・時計・乱数を署名へ入れない。"""
from __future__ import annotations

from dataclasses import fields, is_dataclass
from enum import Enum
from functools import lru_cache
from hashlib import sha256
import json
import math
from collections.abc import Mapping


def 文字(値: object, 名称: str = "識別子") -> str:
    if not isinstance(値, str) or not 値.strip():
        raise ValueError(f"{名称}は空でない文字列が必要")
    return 値


def 整数(値: object, 名称: str, 下限: int = 0, 上限: int = 1_000_000) -> int:
    if type(値) is not int or not 下限 <= 値 <= 上限:
        raise ValueError(f"{名称}は{下限}..{上限}の整数が必要")
    return 値


def 文字列組(値: object, 名称: str = "要素", *, 一意: bool = True) -> None:
    if not isinstance(値, tuple):
        raise TypeError(f"{名称}はtupleが必要")
    for 項 in 値:
        文字(項, 名称)
    if 一意 and len(値) != len(set(値)):
        raise ValueError(f"{名称}に重複がある")


@lru_cache(maxsize=2048)
def _文字列キーJSON(鍵: str) -> str:
    """短い不変文字列の並び順表現だけを再利用する。状態・本文は保存しない。"""
    return json.dumps(鍵, ensure_ascii=False, sort_keys=True)


def _写像キー順序(組: list[object]) -> str:
    鍵 = 組[0]
    if type(鍵) is str and len(鍵) <= 256:
        return _文字列キーJSON(鍵)
    return json.dumps(鍵, ensure_ascii=False, sort_keys=True)


def 正規化(値: object, _経路: frozenset[int] = frozenset()) -> object:
    """型と欠落を区別する正準表現。循環参照と未契約型は拒否する。

    外部型は引数なしのHDS署名値()で、決定論的な意味データを明示できる。
    これは署名契約であり、そのデータが真であることの証明ではない。
    """
    if isinstance(値, Enum):
        return {"列挙型": type(値).__module__ + "." + type(値).__qualname__, "値": 正規化(値.value)}
    if 値 is None or type(値) in (str, bool, int):
        return 値
    if type(値) is float:
        if not math.isfinite(値):
            raise ValueError("非有限数は意味署名へ入れられない")
        return {"実数": 値.hex()}
    if isinstance(値, bytes):
        return {"バイト列": 値.hex()}
    if id(値) in _経路:
        raise ValueError("循環参照を含む意味状態")
    経路 = _経路 | {id(値)}
    if isinstance(値, Mapping):
        組 = [[正規化(k, 経路), 正規化(v, 経路)] for k, v in 値.items()]
        return {"写像": sorted(組, key=_写像キー順序)}
    if isinstance(値, (tuple, list)):
        return {"組" if isinstance(値, tuple) else "列": [正規化(v, 経路) for v in 値]}
    if isinstance(値, (set, frozenset)):
        return {"集合": sorted((正規化(v, 経路) for v in 値), key=lambda x: json.dumps(x, ensure_ascii=False, sort_keys=True))}
    契約 = getattr(値, "HDS署名値", None)
    if callable(契約):
        return {"契約型": type(値).__module__ + "." + type(値).__qualname__, "値": 正規化(契約(), 経路)}
    if is_dataclass(値) and not isinstance(値, type):
        return {
            "構造型": type(値).__module__ + "." + type(値).__qualname__,
            "項目": {f.name: 正規化(getattr(値, f.name), 経路) for f in fields(値) if f.metadata.get("意味", True)},
        }
    raise TypeError(f"意味署名契約がない型: {type(値).__module__}.{type(値).__qualname__}")


def 署名(値: object) -> str:
    return sha256(json.dumps(正規化(値), ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")).hexdigest()


def 不変値(値: object) -> object:
    """認識の値は不変なJSON相当スカラー/tupleのみ。曖昧な暗黙変換はしない。"""
    if 値 is None or type(値) in (str, bool, int):
        return 値
    if type(値) is float and math.isfinite(値):
        return 値
    if isinstance(値, tuple):
        for v in 値:
            不変値(v)
        return 値
    raise TypeError("認識値は有限スカラーまたはそのtupleが必要。可変写像は明示構文化すること")
