"""既存Search/ADC単位試験用。GCS通信は別の局所TCP試験で検証する。"""
from datetime import datetime, timedelta, timezone
import json

from minidora.製品版.検索送信予算 import 固定保存先, 検索送信予算設定, GCS検索送信予算


def 局所送信予算(認証器, アプリID="minidora-public-web"):
    now = datetime.now(timezone.utc).replace(microsecond=0)
    target = 固定保存先("minidora-local-budget", "budget/search.json")
    config = 検索送信予算設定("local-test", "minidora-507702", アプリID, target,
                            now - timedelta(hours=1), now + timedelta(hours=1))
    送信予算 = GCS検索送信予算(config, 認証器)
    状態 = {"generation": 1, "count": 0}
    def 取得(url, method, body, control):
        target.要求を確認(url, method)
        if method == "POST":
            状態["generation"] += 1
            状態["count"] = json.loads(body)["予約数"]
        if url == target.本文URL(str(状態["generation"])):
            return 200, config.保存内容(状態["count"])
        return 200, json.dumps({"bucket": target.バケット, "name": target.オブジェクト,
                               "generation": str(状態["generation"])}).encode()
    送信予算._取得 = 取得
    return 送信予算
