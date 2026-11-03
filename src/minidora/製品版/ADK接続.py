"""ADK 2.xの実行境界から既存HDS製品を一度だけ呼ぶ。

会話と監査の正本はHDS製品に保持する。ADKのセッションは一要求内の
入出力配送に限定し、HDSの意味状態や内部追跡を保存しない。
"""
from __future__ import annotations

import asyncio
from dataclasses import replace
from importlib.metadata import PackageNotFoundError, version
import json
from uuid import uuid4

from .型 import 製品応答


ADK固定版 = "2.11.0"


class ADK製品ミニドラ:
    """既存の応答・能力一覧・監査台帳を維持するADK接続入口。"""

    def __init__(self, 製品):
        try:
            導入版 = version("google-adk")
        except PackageNotFoundError as 例外:
            raise RuntimeError("ADK接続にはminidora-notnn[cloud]の導入が必要です") from 例外
        if 導入版 != ADK固定版:
            raise RuntimeError(f"google-adkは{ADK固定版}が必要です。導入版: {導入版}")
        from google.adk import Workflow
        from google.adk.runners import Runner
        from google.adk.sessions import InMemorySessionService
        from google.adk.workflow import FunctionNode, RetryConfig
        from google.genai import types

        self._製品 = 製品
        self._版 = 導入版
        self._工程型 = FunctionNode
        self._接続型 = Workflow
        self._実行型 = Runner
        self._状態庫型 = InMemorySessionService
        self._再試行型 = RetryConfig
        self._外部型 = types

    @property
    def 監査台帳(self):
        return self._製品.監査台帳

    def 能力一覧(self):
        return self._製品.能力一覧()

    @property
    def 要求単位外部読取(self):
        return bool(getattr(self._製品, "要求単位外部読取", False))

    @property
    def 公開取得方針(self):
        return getattr(self._製品, "公開取得方針", None)

    def 応答(self, 入力文, *, セッションID="default", 外部読取許可=None) -> 製品応答:
        """同期CLI/HTTP入口。非同期呼出元は応答非同期を使用する。"""
        try:
            asyncio.get_running_loop()
        except RuntimeError:
            pass
        else:
            raise RuntimeError("実行中の非同期ループでは応答非同期を使用してください")
        return asyncio.run(self.応答非同期(入力文, セッションID=セッションID, 外部読取許可=外部読取許可))

    async def 応答非同期(self, 入力文, *, セッションID="default", 外部読取許可=None) -> 製品応答:
        # 外部形式への直列化だけを担当し、入力の意味判断はHDSに委ねる。
        if 外部読取許可 is not None and type(外部読取許可) is not bool:
            raise TypeError("外部読取許可はbool")
        配送文 = json.dumps({"入力文": 入力文, "セッションID": セッションID, "外部読取許可": 外部読取許可}, ensure_ascii=False)
        製品結果 = []

        def HDSを駆動(node_input):
            断片 = node_input.parts or ()
            if len(断片) != 1 or 断片[0].text is None:
                raise ValueError("ADK入力は単一のテキスト配送文が必要です")
            配送 = json.loads(断片[0].text)
            条件 = {"セッションID": 配送["セッションID"]}
            if 配送["外部読取許可"] is not None:
                条件["外部読取許可"] = 配送["外部読取許可"]
            結果 = self._製品.応答(配送["入力文"], **条件)
            if not isinstance(結果, 製品応答):
                raise TypeError("HDS製品応答の型が不正です")
            製品結果.append(結果)
            # HDSの内部追跡をADK Eventへ複製しない。
            return {"状態": 結果.状態, "追跡ID": 結果.追跡ID, "監査ハッシュ": 結果.監査ハッシュ}

        工程 = self._工程型(func=HDSを駆動, name="HDS接続", retry_config=self._再試行型(max_attempts=1))
        接続 = self._接続型(name="minidora_adk", edges=[("START", 工程)],
                            retry_config=self._再試行型(max_attempts=1))
        状態庫 = self._状態庫型()
        要求ID = uuid4().hex
        共通引数 = {"app_name": 接続.name, "user_id": "minidora_product", "session_id": 要求ID}
        await 状態庫.create_session(**共通引数)
        通過出力 = []
        実行ID = ""
        try:
            async with self._実行型(node=接続, session_service=状態庫) as 実行:
                入力 = self._外部型.Content(role="user", parts=[self._外部型.Part(text=配送文)])
                async for イベント in 実行.run_async(user_id=共通引数["user_id"], session_id=要求ID,
                                                new_message=入力):
                    if イベント.output is not None:
                        通過出力.append(イベント.output)
                        実行ID = イベント.invocation_id
        finally:
            await 状態庫.delete_session(**共通引数)

        if len(製品結果) != 1 or not 実行ID:
            raise RuntimeError("ADKからHDSへの単一実行を確認できませんでした")
        結果 = 製品結果[0]
        照合値 = {"状態": 結果.状態, "追跡ID": 結果.追跡ID, "監査ハッシュ": 結果.監査ハッシュ}
        if not 通過出力 or any(出力 != 照合値 for 出力 in 通過出力):
            raise RuntimeError("ADK通過出力とHDS製品応答が一致しません")
        return replace(結果, メタデータ={**結果.メタデータ, "ADK": {
            "ライブラリ": "google-adk", "版": self._版, "実行方式": "Workflow/FunctionNode",
            "実行ID": 実行ID, "通過": True, "HDS呼出数": 1, "追跡ID": 結果.追跡ID,
        }})
