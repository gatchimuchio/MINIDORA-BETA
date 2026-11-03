"""表現済みのバイト列だけを指定先へ送る。内容の生成・学習・再採否は行わない。"""
from __future__ import annotations
from pathlib import Path
from dataclasses import replace
import os
import stat
from hashlib import sha256
from ..共通契約.出力 import 送達要求, 受領票, 送達器, 送達記録


def 外部へ射影(要求: 送達要求, 器: 送達器, 試行回数: int) -> 送達記録:
    try:
        写し = replace(要求)
        票 = 器.送る(写し)
        if 写し != 要求:
            raise ValueError('送達器が送達要求を改変した')
        if not isinstance(票, 受領票):
            raise TypeError('受領票型が必要')
        票.__post_init__()
        if (票.送達ID, 票.本文署名) != (要求.送達ID, 要求.本文署名):
            raise ValueError('別出力の受領票')
        return 送達記録(要求.送達ID, 要求.表現署名, 器.ID,
            '送達済み' if 票.状態 == '受領' else '未受理', 試行回数,
            () if 票.状態 == '受領' else ('受信先が未受理を報告',))
    except Exception as 例外:
        # 送信後に例外が出た可能性を捨てず、単なる未送信失敗とは扱わない。
        return 送達記録(要求.送達ID, 要求.表現署名, 器.ID, '結果不明', 試行回数,
            ('送達結果不明:' + type(例外).__name__,))


def _関数受領(要求):
    return 受領票(要求.送達ID, 要求.本文署名, '受領')


関数返却先 = 送達器('関数返却', _関数受領, 重複排除対応=True)


def ファイル送達器(保存先: Path, *, ID: str) -> 送達器:
    """信頼する呼出側が指定した単一ファイルへ排他的作成。本文由来のパスは扱わない。

    既存ファイルは同内容なら受領、別内容なら未受理。上書き・親作成はしない。
    出力先ディレクトリは信頼された単一所有領域を前提とする。並行する悪意ある
    ホストによる親ディレクトリの差替えを隔離する機構ではない。
    """
    保存先 = Path(保存先).expanduser().absolute()
    def 検査():
        if not 保存先.parent.is_dir():
            raise ValueError('送達先の親ディレクトリがない')
        for 路 in (保存先, *保存先.parents):
            if 路.is_symlink():
                raise ValueError('送達先のリンクは不可')
        if 保存先.exists() and not stat.S_ISREG(保存先.stat().st_mode):
            raise ValueError('送達先の特殊ファイルは不可')
    検査()
    def 書込(要求):
        検査()
        if 保存先.exists():
            if 保存先.stat().st_size == len(要求.本文):
                # 任意に大きい既存ファイルを読み込まない。
                with 保存先.open('rb') as 入:
                    一致 = 入.read(len(要求.本文) + 1) == 要求.本文
                if 一致:
                    return 受領票(要求.送達ID, 要求.本文署名, '受領')
            return 受領票(要求.送達ID, 要求.本文署名, '未受理', '既存の別内容を上書きしない')
        if sha256(要求.本文).hexdigest() != 要求.本文署名:
            raise ValueError('送達内容署名不一致')
        fd = os.open(保存先, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, 'O_NOFOLLOW', 0), 0o600)
        with os.fdopen(fd, 'wb') as 出:
            出.write(要求.本文)
            出.flush()
            os.fsync(出.fileno())
        return 受領票(要求.送達ID, 要求.本文署名, '受領')
    return 送達器(ID, 書込, 重複排除対応=True)
