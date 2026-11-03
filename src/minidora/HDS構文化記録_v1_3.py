from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class HDS作用記録:
    """入力から観測できた一つの状態作用。採否・実行済みを意味しない。"""

    作用ID: str
    種別: str
    入力状態: str | None
    出力状態: str | None
    条件: tuple[str, ...] = ()
    可逆: bool | None = None
    巻戻し先: str | None = None
    由来遷移ID: str = ""


@dataclass(frozen=True, slots=True)
class HDS状態差記録:
    '作用前後の状態差。状態名が同一なら現行射影では無変化として保持する。'

    差分ID: str
    原因作用ID: str
    前状態: str
    後状態: str
    変化有無: bool
    由来遷移ID: str = ""


@dataclass(frozen=True, slots=True)
class HDS後続利用記録:
    """状態差の後状態が別作用の入力状態条件を満たす構造的接続。

    追加条件の充足や実際の発火は確定しない。
    """

    利用ID: str
    原因差分ID: str
    成立状態: str
    後続作用ID: str
    追加条件: tuple[str, ...] = ()
    状態条件充足: bool = True


@dataclass(frozen=True, slots=True)
class HDS作用差分構造:
    作用: tuple[HDS作用記録, ...] = ()
    状態差: tuple[HDS状態差記録, ...] = ()
    後続利用: tuple[HDS後続利用記録, ...] = ()
    未閉包: tuple[str, ...] = ()

    @property
    def 作用数(self) -> int:
        return len(self.作用)

    @property
    def 状態差数(self) -> int:
        return len(self.状態差)

    @property
    def 後続利用数(self) -> int:
        return len(self.後続利用)


__all__ = [
    "HDS作用記録",
    "HDS状態差記録",
    "HDS後続利用記録",
    "HDS作用差分構造",
]
