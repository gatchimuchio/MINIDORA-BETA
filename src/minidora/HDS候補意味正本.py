from __future__ import annotations

from collections.abc import Iterable, Mapping

from .HDS中間表現 import HDSIR


def HDS候補意味IR辞書(
    候補意味IR: Mapping[str, HDSIR] | Iterable[tuple[str, HDSIR]] | None,
    期待ラベル: Iterable[str],
) -> dict[str, HDSIR] | None:
    """Kernel形成済み候補意味IRを選択肢順へ固定する。

    None は旧入口だけの縮退契約とし、Kernelから候補意味IRが渡された場合は
    欠落・重複・余分な候補を黙って補完しない。
    """
    if 候補意味IR is None:
        return None
    if isinstance(候補意味IR, Mapping):
        行群 = tuple((str(ラベル), ir) for ラベル, ir in 候補意味IR.items())
    else:
        行群 = tuple((str(ラベル), ir) for ラベル, ir in 候補意味IR)
    期待 = tuple(str(x) for x in 期待ラベル)
    ラベル群 = tuple(ラベル for ラベル, _ in 行群)
    if len(ラベル群) != len(set(ラベル群)):
        raise ValueError("候補意味IRのラベルが重複")
    if any(not isinstance(ir, HDSIR) for _, ir in 行群):
        raise TypeError("候補意味IRはHDSIRである必要がある")
    辞書 = dict(行群)
    if set(辞書) != set(期待):
        raise ValueError("候補意味IRが選択肢集合と一致しない")
    return {ラベル: 辞書[ラベル] for ラベル in 期待}


__all__ = ["HDS候補意味IR辞書"]
