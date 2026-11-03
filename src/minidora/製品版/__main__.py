from __future__ import annotations
import argparse, os
from .製品チャット import 製品ミニドラ
from .監査 import 監査台帳
from .api import serve
from ..標準構成 import 標準ミニドラ

def main() -> int:
    import sys
    p = argparse.ArgumentParser()
    p.add_argument("--serve", action="store_true")
    p.add_argument("--HDS", action="store_true", help="HDS単一主体の全体運用を利用")
    p.add_argument("--ADK", action="store_true", help="ADK 2.xの実行境界から既存HDSを駆動")
    p.add_argument("--CloudRun", action="store_true", help="HTTP Basicで保護するCloud Run用のHTTP入口")
    p.add_argument("--基本チャット", action="store_true", help="管理者の方針に従いHDSが必要な公開資料を自動取得")
    p.add_argument("--汎用", action="store_true", help="目的・複数資料・確認継続の会話入口を利用")
    p.add_argument("--外部読取", action="store_true", help="汎用入口で要求された公開資料取得を許可")
    p.add_argument("--検索しない", action="store_true", help="基本チャットの今回の質問では外部取得を禁止")
    p.add_argument("--session", default="cli")
    p.add_argument("message", nargs="*")
    a = p.parse_args()
    if a.HDS and a.汎用:
        p.error("--HDSと--汎用は別の実行入口です")
    if a.ADK and not a.HDS:
        p.error("--ADKは--HDSと共に指定してください")
    if a.CloudRun and not (a.HDS and a.ADK and a.serve):
        p.error("--CloudRunは--HDS --ADK --serveと共に指定してください")
    if a.基本チャット and not a.HDS:
        p.error("--基本チャットは--HDSと共に指定してください")
    if a.検索しない and (not a.基本チャット or a.外部読取 or a.serve):
        p.error("--検索しないは基本チャットの質問にのみ指定でき、--外部読取とは併用できません")
    if a.CloudRun and a.外部読取 and not a.基本チャット:
        p.error("Cloud Run検証環境では外部読取を許可しません")
    if a.汎用 or a.HDS:
        for stream in (sys.stdin,sys.stdout,sys.stderr):
            if hasattr(stream,"reconfigure"):
                stream.reconfigure(encoding="utf-8",errors="strict")
    audit = 監査台帳(os.getenv("MINIDORA_AUDIT_LOG") or None)
    if a.外部読取 and not (a.汎用 or a.HDS):
        p.error("--外部読取は--HDS又は--汎用と共に指定してください")
    if a.HDS:
        from ..HDS運用.製品 import HDS製品ミニドラ
        取得器 = None
        if a.CloudRun and a.基本チャット:
            from .Google起動構成 import GoogleCloud取得器を構成, Google起動設定不正
            try:
                取得器 = GoogleCloud取得器を構成(os.environ)
            except Google起動設定不正 as 例外:
                p.error(str(例外))
        app = HDS製品ミニドラ(外部読取許可=a.外部読取, 監査台帳_=audit,
                             基本チャット=a.基本チャット, 取得器=取得器)
    else:
        app = 製品ミニドラ(基礎ミニドラ=None if a.汎用 else 標準ミニドラ(), 監査台帳_=audit,
                          汎用会話=a.汎用, 汎用外部読取許可=a.外部読取)
    if a.ADK:
        from .ADK接続 import ADK製品ミニドラ
        app = ADK製品ミニドラ(app)
    if a.serve:
        if a.CloudRun: serve(app, host="0.0.0.0", CloudRun境界=True)
        elif a.汎用 or a.HDS: serve(app, host="127.0.0.1", 同一生成元限定=True)
        else: serve(app)
        return 0
    if a.message:
        条件 = {"セッションID": a.session}
        if a.基本チャット and (a.外部読取 or a.検索しない):
            条件["外部読取許可"] = not a.検索しない
        r=app.応答(" ".join(a.message), **条件)
        print(r.本文); print(f"trace_id={r.追跡ID}\ntrace_hash={r.監査ハッシュ}")
        return 2 if a.HDS and r.状態 != "APPROVE" else 0
    print("MINIDORA Product Chat / exit で終了")
    while True:
        try: text=input("> ").strip()
        except EOFError: return 0
        except UnicodeError:
            if not (a.汎用 or a.HDS): raise
            print("標準入力はUTF-8で指定してください。", file=sys.stderr)
            return 2
        if text.casefold() in {"exit","quit"}: return 0
        条件 = {"セッションID": a.session}
        if a.基本チャット and (a.外部読取 or a.検索しない):
            条件["外部読取許可"] = not a.検索しない
        r=app.応答(text, **条件); print(r.本文); print(f"[trace:{r.追跡ID}]")
if __name__ == "__main__": raise SystemExit(main())
