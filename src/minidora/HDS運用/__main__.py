"""HDS通常運用の同期CLI。資料と実行命令を分け、全経路を同じ主体へ渡す。"""
from __future__ import annotations
import argparse
from pathlib import Path
import sys
from .セッション import HDS運用セッション
from .保存移行 import 旧保存を移行
from .製品 import HDS製品ミニドラ
from ..製品版.api import serve


def main():
    parser = argparse.ArgumentParser(description="HDS-MINIDORA 全体運用")
    parser.add_argument("--serve", action="store_true", help="既存画面・APIをローカル限定で起動")
    parser.add_argument("--外部読取", action="store_true")
    parser.add_argument("--session", default="cli")
    parser.add_argument("--資料", action="append", default=[], metavar="名前=ファイル")
    parser.add_argument("--知識", action="append", default=[], metavar="名前=ファイル", help="提供知識を出典付きの命題資産へ形成")
    parser.add_argument("--形成なし", action="store_true", help="純粋工程の追加再実行を行わない")
    復帰 = parser.add_mutually_exclusive_group()
    復帰.add_argument("--復元", type=Path)
    復帰.add_argument("--移行元", type=Path, help="v1〜v4を原本保持で移行。別の--保存先が必須")
    parser.add_argument("--保存", type=Path)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("message", nargs="*")
    args = parser.parse_args()
    for stream in (sys.stdin, sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="strict")
    if args.serve:
        if args.復元 or args.移行元 or args.保存 or args.資料 or args.知識 or args.message:
            parser.error("サーバ起動にCLI専用の資料・保存・入力指定は併用できません")
        serve(HDS製品ミニドラ(外部読取許可=args.外部読取, 手順形成=not args.形成なし), host="127.0.0.1", 同一生成元限定=True)
        return 0
    if args.移行元:
        if not args.保存 or args.保存.resolve() == args.移行元.resolve() or args.保存.exists():
            parser.error("移行では旧原本を上書きしません。未作成の別ファイルを--保存に指定してください")
        session = 旧保存を移行(args.移行元.read_text(encoding="utf-8"), 外部読取許可=args.外部読取)
    else:
        session = (HDS運用セッション.復元(args.復元.read_text(encoding="utf-8"), 外部読取許可=args.外部読取)
                   if args.復元 else HDS運用セッション(args.session, 外部読取許可=args.外部読取, 手順形成=not args.形成なし))
    if args.形成なし:
        session.手順形成 = False
    for item, 種類 in [(値, "資料") for 値 in args.資料] + [(値, "知識") for 値 in args.知識]:
        name, separator, path = item.partition("=")
        if not separator:
            parser.error("資料は名前=ファイルで指定してください")
        結果 = session.資料を登録(name, Path(path).read_text(encoding="utf-8"), 種類=種類)
        if not 結果.成立:
            print(結果.本文, file=sys.stderr)
            return 2
    def run(text):
        結果 = session.応答(text, 外部読取許可=args.外部読取)
        if args.json:
            import json
            print(json.dumps(結果.辞書化(), ensure_ascii=False))
        else:
            print(結果.本文)
        if args.保存:
            session.保存先へ書く(args.保存)
        return 0 if 結果.成立 else 2
    if args.message:
        return run(" ".join(args.message))
    if args.移行元:
        session.保存先へ書く(args.保存)
        if args.json:
            import json
            print(json.dumps({"状態": "移行完了", "版": session.状態()["版"], "旧成果再採用": False}, ensure_ascii=False))
        else:
            print("移行しました。原本と履歴・依存を保持し、旧成果と旧手順の再採用は行っていません。")
        return 0
    while True:
        try:
            text = input("HDS> ")
        except EOFError:
            return 0
        if text in ("exit", "quit"):
            return 0
        run(text)


if __name__ == "__main__":
    raise SystemExit(main())
