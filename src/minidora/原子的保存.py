"""同一ディレクトリの一時ファイルから、完成したUTF-8ファイルだけを公開する。

各ファイル単独の原子的置換であり、複数ファイルの一括トランザクションではない。
fsyncするのはファイル本体。停電時のディレクトリエントリの永続性は保証しない。
"""
from __future__ import annotations

from contextlib import contextmanager
import os
from pathlib import Path
import tempfile
from typing import Iterator, TextIO


@contextmanager
def 原子的テキスト出力(path: Path, *, 上書き: bool = False) -> Iterator[TextIO]:
    if type(上書き) is not bool:
        raise ValueError('上書き指定はbool')
    path = Path(path)
    if path.is_symlink():
        raise ValueError('出力先のシンボリックリンクは使用しない')
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.minidora-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8', errors='strict', newline='\n') as stream:
            yield stream
            stream.flush()
            os.fsync(stream.fileno())
        # 事前のexists確認だけでは競合する。公開時にも上書き可否を強制する。
        if 上書き:
            os.replace(temporary, path)
        else:
            os.link(temporary, path)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def 同じファイル(a: Path, b: Path) -> bool:
    """正規化パスだけでなく、異名のハードリンクも照合する。"""
    if a.resolve() == b.resolve():
        return True
    try:
        return a.samefile(b)
    except FileNotFoundError:
        return False
