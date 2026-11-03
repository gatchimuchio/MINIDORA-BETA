"""CLIの標準入出力契約。呼出し元のストリームの所有権は変更しない。

起動直後、引数解析や最初の読書きより前に呼ぶ。ライブラリのimportでは実行しない。
機械入出力はUTF-8/strictとし、置換文字で破損を隠さない。
StringIO等の既にUnicodeであるストリームはそのまま利用する。
"""
from __future__ import annotations

import sys


def 標準入出力をUTF8にする() -> None:
    for stream in (sys.stdin, sys.stdout, sys.stderr):
        configure = getattr(stream, 'reconfigure', None)
        if callable(configure):
            configure(encoding='utf-8', errors='strict')
