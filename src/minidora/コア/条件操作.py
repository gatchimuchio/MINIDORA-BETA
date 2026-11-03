"""明示条件を三値で扱う共通操作。未記載を否定へ変換しない。"""
from __future__ import annotations
from dataclasses import dataclass
from enum import StrEnum
from .値 import 文字, 署名


class 条件状態(StrEnum):
    成立 = "成立"
    不成立 = "不成立"
    未確定 = "未確定"


@dataclass(frozen=True, slots=True)
class 条件項:
    ID: str
    命題: object
    期待: bool = True
    種別: str = "前提"
    範囲: str = "未指定"

    def __post_init__(self):
        文字(self.ID, "条件ID")
        文字(self.種別, "条件種別")
        文字(self.範囲, "条件範囲")
        if type(self.期待) is not bool:
            raise TypeError("条件期待はbool")
        署名(self.命題)


@dataclass(frozen=True, slots=True)
class 条件判定:
    状態: 条件状態
    成立ID: tuple[str, ...] = ()
    不成立ID: tuple[str, ...] = ()
    未確定ID: tuple[str, ...] = ()

    @property
    def 採用可能(self) -> bool:
        return self.状態 == 条件状態.成立


def 条件集合を判定(条件群: tuple[条件項, ...], 真集合, 偽集合=()) -> 条件判定:
    """真集合/偽集合の明示値だけで条件を判定する。閉世界仮定を使わない。"""
    if not isinstance(条件群, tuple) or any(not isinstance(x, 条件項) for x in 条件群):
        raise TypeError("条件群は条件項tuple")
    真署名 = {署名(x) for x in 真集合}
    偽署名 = {署名(x) for x in 偽集合}
    if 真署名 & 偽署名:
        raise ValueError("同じ命題を真偽同時に与えられない")
    成立ID列, 不成立ID列, 未確定ID列 = [], [], []
    for 条件 in 条件群:
        命題署名 = 署名(条件.命題)
        if 条件.期待:
            if 命題署名 in 真署名: 成立ID列.append(条件.ID)
            elif 命題署名 in 偽署名: 不成立ID列.append(条件.ID)
            else: 未確定ID列.append(条件.ID)
        else:
            if 命題署名 in 偽署名: 成立ID列.append(条件.ID)
            elif 命題署名 in 真署名: 不成立ID列.append(条件.ID)
            else: 未確定ID列.append(条件.ID)
    判定状態 = 条件状態.不成立 if 不成立ID列 else 条件状態.未確定 if 未確定ID列 else 条件状態.成立
    return 条件判定(判定状態, tuple(成立ID列), tuple(不成立ID列), tuple(未確定ID列))
