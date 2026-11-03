"""既存能力の共通登録と、純粋な処理結果だけを再利用する統合側の調整。"""
from __future__ import annotations

from copy import deepcopy
from dataclasses import replace
from hashlib import sha256
import re

from .能力合成 import 登録能力, _文脈辞書, _結果辞書, _符号化
from .能力合成_局所接続 import 局所能力群, 文脈変換接続
from .製品版.型 import 能力結果

統合能力版 = "MINIDORA-統合能力-v0.1"


class 完全文脈変換(文脈変換接続):
    """旧入口を変えず、統合経路の箇条書きだけを全項目保持へ変更する。"""
    版 = 統合能力版 + "/全項目変換"

    def 実行(self, 文脈):
        settings = (文脈.補助 or {}).get("合成設定", {})
        if settings == {"形式": "箇条書き"}:
            items = [s.strip() for s in re.split(r"[。\n]+", 文脈.直前応答) if s.strip()]
            if not items:
                return 能力結果(False, "", 保留理由="変換対象がない")
            body = "\n".join("- " + item for item in items)
            if len(body) > 100000:
                return 能力結果(False, "", 保留理由="全項目変換の上限。切断せず保留")
            return 能力結果(True, body, 根拠=("全項目の箇条書き化",),
                            データ={"形式": "箇条書き", "項目数": len(items), "省略数": 0})
        return super().実行(文脈)


from .純粋結果庫 import 純粋結果庫


from .純粋再利用 import 純粋再利用能力 as _再利用能力


def 統合能力群(庫, 再利用庫, *, 外部読取許可=False, 取得器=None, 閲覧器=None):
    '全15領域の制御器と部品を接続する。領域数とモジュール数は別。'
    from .証拠統合接続 import 証拠統合モジュール, 記載値採用モジュール
    from .応答構成接続 import 応答構成モジュール
    from .関係制約接続 import 関係制約モジュール, 数値関係化モジュール, 関係判定採用モジュール
    from .コード能力接続 import コード能力群
    from .数学能力接続 import 数学能力群
    from .多言語接続 import 多言語変換モジュール
    from .構造化文書接続 import 構造化文書能力群
    from .文章能力接続 import 文章能力群
    from .多段解決接続 import 解決補助能力群, 多段解決モジュール
    from .長文脈接続 import 長文脈選択モジュール
    from .知識取得接続 import 知識取得モジュール
    from .知識取得 import 知識取得器
    from .製品版.検索 import SearXNG検索供給器
    from .ブラウザ閲覧 import ブラウザ閲覧器
    from .ブラウザ接続 import ブラウザ閲覧モジュール
    if type(外部読取許可) is not bool or not isinstance(再利用庫, 純粋結果庫):
        raise ValueError("登録条件不正")
    local = tuple(replace(r, モジュール=完全文脈変換()) if r.モジュール.名前 == "文脈変換" else r for r in 局所能力群())
    pure = (*local, *(m.登録() for m in (証拠統合モジュール(), 記載値採用モジュール(), 応答構成モジュール(),
            関係制約モジュール(), 数値関係化モジュール(), 関係判定採用モジュール())),
            *コード能力群(), *数学能力群(), 多言語変換モジュール().登録(),
            *構造化文書能力群(), *文章能力群(), *解決補助能力群())
    # 再利用はこの明示した純粋部品だけ。外部取得・可変文脈・探索全体は含めない。
    cached = tuple(登録能力(_再利用能力(r.モジュール, 再利用庫)) for r in pure)
    search = 取得器 if 取得器 is not None else 知識取得器(SearXNG検索供給器())
    browser = 閲覧器 if 閲覧器 is not None else ブラウザ閲覧器()
    return (*cached, 多段解決モジュール(cached, 純粋作用確認=True).登録(), 長文脈選択モジュール(庫).登録(),
            知識取得モジュール(search, 外部読取許可=外部読取許可).登録(),
            ブラウザ閲覧モジュール(browser, 外部読取許可=外部読取許可).登録())
