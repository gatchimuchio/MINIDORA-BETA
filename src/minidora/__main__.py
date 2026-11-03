from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Sequence

from .実行系 import ミニドラ, 要求
from .標準構成 import 標準ミニドラ


def _標準入出力をUTF8化() -> None:
    """日本語基底CLIの標準入出力をOSロケールから分離する。"""
    for stream in (sys.stdin, sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is not None:
            reconfigure(encoding="utf-8", errors="strict")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="minidora",
        description='MINIDORA v0.5 日本語基底・非ニューラルネットワークLLM 実行系',
    )
    parser.add_argument("query", nargs="?", help="MINIDORAへ渡す言語入力")
    parser.add_argument(
        "--json",
        action="store_true",
        dest="json_mode",
        help="値・採否・理由・計画をJSONで出力する",
    )
    return parser


def _run_once(body: ミニドラ, query: str, *, json_mode: bool) -> None:
    if json_mode:
        結果 = body.実行(要求(query))
        構文化器 = body.HDSコンパイラ
        payload = {
            "query": query,
            "value": 結果.値,
            "status": 結果.採否.状態.value,
            "reasons": list(結果.採否.理由),
            "plan": 結果.言語計画,
            '参照_count': len(結果.参照),
            "HDS中間表現": 結果.HDS_IR is not None,
            '構文化器': "公開HDSコンパイラ",
            '構文化器_構造': getattr(構文化器, '構造版', None),
            '構文化器_処理系列': getattr(構文化器, '処理系列版', None),
            "実行系": "MINIDORA v0.5",
            '模型_模型核': "MINIDORA模型核",
        }
        print(json.dumps(payload, ensure_ascii=False, default=str))
        return
    print(body.応答(query))


def main(argv: Sequence[str] | None = None) -> int:
    _標準入出力をUTF8化()
    args = _parser().parse_args(argv)
    body = 標準ミニドラ()

    if args.query is not None:
        query = args.query.strip()
        if not query:
            raise SystemExit("入力が空です。")
        _run_once(body, query, json_mode=args.json_mode)
        return 0

    try:
        while True:
            query = input("MINIDORA> ").strip()
            if not query:
                continue
            _run_once(body, query, json_mode=args.json_mode)
    except (EOFError, KeyboardInterrupt):
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
