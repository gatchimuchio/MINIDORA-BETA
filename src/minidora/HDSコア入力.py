"""MINIDORA Compiler KernelがCoreへ射影するHDS入力契約。

構文化器の監査履歴・計算手順・能力名・最終採否を含めない。
自然言語から観測できた意味、関係、条件、目的、残差、表現制約だけを保持する。
"""
from __future__ import annotations

from dataclasses import dataclass

from .コア.値 import 署名, 文字, 文字列組


@dataclass(frozen=True, slots=True)
class HDSコア意味項目:
    ID: str
    種別: str
    内容: object
    状態: str
    由来: str
    原文範囲: tuple[int, int] | None = None
    範囲: str = "未指定"
    時点: str = "未指定"

    def __post_init__(self) -> None:
        for 名 in ("ID", "種別", "状態", "由来", "範囲", "時点"):
            文字(getattr(self, 名), 名)
        署名(self.内容)
        if self.原文範囲 is not None:
            if (not isinstance(self.原文範囲, tuple) or len(self.原文範囲) != 2
                    or any(type(x) is not int for x in self.原文範囲)
                    or not 0 <= self.原文範囲[0] <= self.原文範囲[1]):
                raise ValueError("原文範囲不正")


@dataclass(frozen=True, slots=True)
class HDSコア条件:
    ID: str
    種別: str
    内容: object
    状態: str
    由来: str
    適用先: tuple[str, ...] = ()
    原文範囲: tuple[int, int] | None = None

    def __post_init__(self) -> None:
        for 名 in ("ID", "種別", "状態", "由来"):
            文字(getattr(self, 名), 名)
        署名(self.内容)
        文字列組(self.適用先, "条件適用先")
        if self.原文範囲 is not None:
            if (not isinstance(self.原文範囲, tuple) or len(self.原文範囲) != 2
                    or any(type(x) is not int for x in self.原文範囲)
                    or not 0 <= self.原文範囲[0] <= self.原文範囲[1]):
                raise ValueError("条件原文範囲不正")


@dataclass(frozen=True, slots=True)
class HDSコア関係:
    ID: str
    始点: tuple[str, ...]
    終点: tuple[str, ...]
    種別: str
    条件ID: tuple[str, ...] = ()
    制約: tuple[str, ...] = ()
    状態: str = "未確定"
    由来: str = "自然言語入力"

    def __post_init__(self) -> None:
        for 名 in ("ID", "種別", "状態", "由来"):
            文字(getattr(self, 名), 名)
        文字列組(self.始点, "関係始点")
        文字列組(self.終点, "関係終点")
        文字列組(self.条件ID, "関係条件ID")
        文字列組(self.制約, "関係制約", 一意=False)
        if not self.始点 or not self.終点:
            raise ValueError("Core入力の関係は始点・終点が必要")


@dataclass(frozen=True, slots=True)
class HDSコア目的:
    ID: str
    種別: str
    内容: object
    対象参照: tuple[str, ...] = ()
    原文範囲: tuple[int, int] | None = None

    def __post_init__(self) -> None:
        文字(self.ID, "目的ID")
        文字(self.種別, "目的種別")
        文字列組(self.対象参照, "目的対象参照")
        署名(self.内容)


@dataclass(frozen=True, slots=True)
class HDSコア作用要求:
    ID: str
    種別: str
    対象参照: tuple[str, ...] = ()
    要求成果: tuple[str, ...] = ()
    原文範囲: tuple[int, int] | None = None

    def __post_init__(self) -> None:
        文字(self.ID, "作用要求ID")
        文字(self.種別, "作用要求種別")
        文字列組(self.対象参照, "作用要求対象")
        文字列組(self.要求成果, "要求成果")


@dataclass(frozen=True, slots=True)
class HDSコア残差:
    ID: str
    種別: str
    理由: str
    原文: str = ""
    影響参照: tuple[str, ...] = ()
    解消条件: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        for 名 in ("ID", "種別", "理由"):
            文字(getattr(self, 名), 名)
        if not isinstance(self.原文, str):
            raise TypeError("残差原文は文字列")
        文字列組(self.影響参照, "残差影響参照")
        文字列組(self.解消条件, "残差解消条件", 一意=False)


@dataclass(frozen=True, slots=True)
class HDSコア検証要求:
    ID: str
    種別: str
    対象参照: tuple[str, ...] = ()
    条件: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        文字(self.ID, "検証要求ID")
        文字(self.種別, "検証要求種別")
        文字列組(self.対象参照, "検証対象参照")
        文字列組(self.条件, "検証条件", 一意=False)


@dataclass(frozen=True, slots=True)
class HDSコア表現要求:
    ID: str
    種別: str
    値: str
    原文範囲: tuple[int, int] | None = None

    def __post_init__(self) -> None:
        for 名 in ("ID", "種別", "値"):
            文字(getattr(self, 名), 名)
        if self.原文範囲 is not None:
            if (not isinstance(self.原文範囲, tuple) or len(self.原文範囲) != 2
                    or any(type(x) is not int for x in self.原文範囲)
                    or not 0 <= self.原文範囲[0] <= self.原文範囲[1]):
                raise ValueError("表現要求原文範囲不正")


@dataclass(frozen=True, slots=True)
class HDSコア実行制約:
    ID: str
    種別: str
    値: str
    原文範囲: tuple[int, int] | None = None

    def __post_init__(self) -> None:
        for 名 in ("ID", "種別", "値"):
            文字(getattr(self, 名), 名)
        if self.原文範囲 is not None:
            if (not isinstance(self.原文範囲, tuple) or len(self.原文範囲) != 2
                    or any(type(x) is not int for x in self.原文範囲)
                    or not 0 <= self.原文範囲[0] <= self.原文範囲[1]):
                raise ValueError("実行制約原文範囲不正")


@dataclass(frozen=True, slots=True)
class HDSコア表現制約:
    入力言語: str
    出力言語: str | None = None
    要求: tuple[HDSコア表現要求, ...] = ()
    保持条件: tuple[str, ...] = ("原文保持", "不確定保持", "残差保持")

    def __post_init__(self) -> None:
        文字(self.入力言語, "入力言語")
        if self.出力言語 is not None:
            文字(self.出力言語, "出力言語")
        if not isinstance(self.要求, tuple) or any(not isinstance(x, HDSコア表現要求) for x in self.要求):
            raise TypeError("表現要求型不正")
        IDs = tuple(x.ID for x in self.要求)
        if len(IDs) != len(set(IDs)):
            raise ValueError("表現要求ID重複")
        文字列組(self.保持条件, "表現保持条件")


@dataclass(frozen=True, slots=True)
class HDSコア入力束:
    原文: str
    認知世界ID: str
    意味項目: tuple[HDSコア意味項目, ...]
    関係: tuple[HDSコア関係, ...]
    条件: tuple[HDSコア条件, ...]
    目的: tuple[HDSコア目的, ...]
    作用要求: tuple[HDSコア作用要求, ...]
    要求成果: tuple[str, ...]
    残差: tuple[HDSコア残差, ...]
    検証要求: tuple[HDSコア検証要求, ...]
    実行制約: tuple[HDSコア実行制約, ...]
    表現制約: HDSコア表現制約
    文脈引用: tuple[str, ...] = ()
    射影由来署名: str = ""
    版: str = "HDS-コア入力-v1"

    def __post_init__(self) -> None:
        文字(self.原文, "原文")
        文字(self.認知世界ID, "認知世界ID")
        文字(self.版, "Core入力版")
        文字列組(self.要求成果, "要求成果")
        文字列組(self.文脈引用, "文脈引用")
        for 名, 型 in (
            ("意味項目", HDSコア意味項目), ("関係", HDSコア関係),
            ("条件", HDSコア条件), ("目的", HDSコア目的),
            ("作用要求", HDSコア作用要求), ("残差", HDSコア残差),
            ("検証要求", HDSコア検証要求), ("実行制約", HDSコア実行制約),
        ):
            群 = getattr(self, 名)
            if not isinstance(群, tuple) or any(not isinstance(x, 型) for x in 群):
                raise TypeError(名 + "の型不正")
            IDs = tuple(x.ID for x in 群)
            if len(IDs) != len(set(IDs)):
                raise ValueError(名 + "のID重複")
        if not isinstance(self.表現制約, HDSコア表現制約):
            raise TypeError("表現制約型不正")
        if self.射影由来署名:
            文字(self.射影由来署名, "射影由来署名")

    @property
    def 意味署名(self) -> str:
        return 署名(self)

    def コア需要を検査(self) -> bool:
        """正本情報の消費先が現行Core責任に存在し、C7へ入力責任を捏造していないことを確認する。"""
        from .コア.責任境界 import コア責任
        現行 = {x.ID for x in コア責任}
        使用 = {責任 for _, 責任群 in self.消費責任 for 責任 in 責任群}
        if not 使用 <= 現行:
            raise ValueError("Coreに存在しない消費責任:" + ",".join(sorted(使用 - 現行)))
        if "C7" in 使用:
            raise ValueError("C7実行内適応へコンパイラ入力責任を割り当ててはならない")
        if 使用 != (現行 - {"C7"}):
            raise ValueError("Core入力需要閉包不成立")
        return True

    @property
    def 消費責任(self) -> tuple[tuple[str, tuple[str, ...]], ...]:
        """Coreのどの責任が各正本情報を消費するかを固定する。C7はCore内部責任。"""
        return (
            ("意味項目", ("C1", "C3")),
            ("関係", ("C3", "C4")),
            ("条件", ("C1", "C4")),
            ("目的", ("C5",)),
            ("作用要求", ("C2", "C5")),
            ("要求成果", ("C5",)),
            ("実行制約", ("C2", "C6")),
            ("残差", ("C5", "C6")),
            ("検証要求", ("C6",)),
            ("表現制約", ("C8",)),
        )

    @property
    def 未解残差ID(self) -> tuple[str, ...]:
        return tuple(x.ID for x in self.残差)


__all__ = [
    "HDSコア意味項目", "HDSコア条件", "HDSコア関係", "HDSコア目的",
    "HDSコア作用要求", "HDSコア残差", "HDSコア検証要求",
    "HDSコア表現要求", "HDSコア実行制約", "HDSコア表現制約",
    "HDSコア入力束",
]
