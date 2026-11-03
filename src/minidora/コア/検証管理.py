"""局所検証と全体採用を分離する共通検証境界。

検証結果の永続台帳はここに新設しない。HDS側の検証票・依存署名・失効伝播を正本とし、
コアは検証器の読取専用実行だけを所有する。
"""
from __future__ import annotations
from .値 import 文字, 署名


def 検証器群を実行(状態, 検証器群: tuple[object, ...], 対象=None) -> tuple[str, ...]:
    """検証器を読取専用で実行し、不合格IDだけを返す。

    検証器自身は全体状態・対象を書換えられない。bool以外の返却も契約違反。
    検証記録の保存・版管理・再利用判断はHDSの検証票と依存署名が所有する。
    """
    from copy import deepcopy
    if not isinstance(検証器群, tuple):
        raise TypeError("検証器群はtuple")
    不合格ID列 = []
    for 検証器 in 検証器群:
        ID = getattr(検証器, "ID", None)
        版 = getattr(検証器, "版", None)
        検証関数 = getattr(検証器, "検証", None)
        文字(ID, "検証器ID")
        文字(版, "検証器版")
        if not callable(検証関数):
            raise TypeError("検証器に検証関数が必要")
        状態写し = deepcopy(状態)
        対象写し = deepcopy(対象)
        前状態署名 = 署名(状態写し)
        前対象署名 = 署名(対象写し)
        判定結果 = 検証関数(状態写し, 対象写し)
        if type(判定結果) is not bool:
            raise TypeError("検証器はboolを返す必要がある")
        if 署名(状態写し) != 前状態署名 or 署名(対象写し) != 前対象署名:
            raise ValueError("検証器が全体状態または対象を書換えた")
        if not 判定結果:
            不合格ID列.append(ID)
    return tuple(不合格ID列)
