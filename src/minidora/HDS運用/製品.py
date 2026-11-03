"""既存CLI/API/画面から同じHDS通常運用を呼ぶ。別のrouterは置かない。"""
from __future__ import annotations
import os
from threading import RLock
from ..製品版.型 import 製品応答
from ..製品版.監査 import 監査台帳
from .セッション import HDS運用セッション
from .値 import 運用版, 文字を検査
from ..取得実行制御 import 公開取得上限
from ..公開送信条件 import 公開送信条件, 安全監査表現版


class HDS製品ミニドラ:
    def __init__(self, *, 外部読取許可=False, 取得器=None, 監査台帳_=None, 最大セッション数=64, 手順形成=True, 基本チャット=False, 公開取得方針=None, 取得上限=None):
        if type(外部読取許可) is not bool or type(手順形成) is not bool:
            raise TypeError("外部読取許可はbool")
        if type(最大セッション数) is not int or not 1 <= 最大セッション数 <= 256:
            raise ValueError("セッション数上限")
        if type(基本チャット) is not bool:
            raise TypeError("基本チャット設定はbool")
        if 取得上限 is not None and not isinstance(取得上限, 公開取得上限):
            raise TypeError("製品の取得上限の型不正")
        if not 基本チャット and 取得上限 is not None:
            raise ValueError("製品の取得上限には基本チャット設定が必要です")
        # 初回の6候補を狭めず、内容不足後の別検索にも6候補を確保する。
        self._取得上限 = (取得上限 if 取得上限 is not None else 公開取得上限(最大候補数=12, 最大本文取得数=12)) if 基本チャット else None
        if 基本チャット:
            方針 = os.getenv("MINIDORA_RETRIEVAL_POLICY", "auto") if 公開取得方針 is None else 公開取得方針
            if type(方針) is not str or 方針 not in {"auto", "off"}:
                raise ValueError("基本チャットの公開取得方針はauto又はoff")
        else:
            if 公開取得方針 is not None:
                raise ValueError("公開取得方針には基本チャット設定が必要です")
            方針 = None
        self.公開取得方針 = 方針
        self.要求単位外部読取 = 基本チャット
        if 基本チャット and 取得器 is None:
            from ..製品版.検索 import Wikipedia知識取得器
            取得器 = Wikipedia知識取得器()
        self.監査台帳 = 監査台帳_ if 監査台帳_ is not None else 監査台帳()
        self._形成 = 手順形成
        self._許可 = 方針 == "auto" if 基本チャット else 外部読取許可
        self._取得器, self._上限 = 取得器, 最大セッション数
        self._会話, self._前監査 = {}, {}
        self._ロック = RLock()
        self._会話ロック = {}

    def 能力一覧(self):
        return ("HDS単一主体の依頼解釈・目的計画・能力工程・最終採否", "数式・JSON/CSV・文書処理",
                "数量比較・集計と条件訂正", "提供知識・本文・命題・仮説・介入の検討",
                "条件付き回復・再表現・資料更新・保存復元",
                "提供知識の出典付き形成と横断導出", "原文・依存成果の長文脈同期と再参照",
                "純粋工程の実再現検証と条件付き手順再利用", "資料の主題・明示別名・見出しを用いた選択と条件同伴",
                "資料の抽出要約・観点対照・確認事項提案", "明示関係の条件導出・不足調査・根拠説明", "旧保存v1〜v4の原本保持移行", "工程別監査")

    def セッション(self, ID):
        文字を検査(ID, "セッションID", 128)
        with self._ロック:
            if ID not in self._会話:
                if len(self._会話) >= self._上限:
                    raise ValueError("セッション数上限")
                self._会話[ID] = HDS運用セッション(ID, 外部読取許可=self._許可, 取得器=self._取得器,
                                                   手順形成=self._形成, 取得上限=self._取得上限)
                self._会話ロック[ID] = RLock()
            return self._会話[ID]

    def 応答(self, 入力文, *, セッションID="default", 外部読取許可=None):
        if 外部読取許可 is not None and type(外部読取許可) is not bool:
            raise TypeError("要求の外部読取許可はbool")
        if not self.要求単位外部読取 and 外部読取許可 is not None:
            raise ValueError("要求単位の外部読取には基本チャット設定が必要です")
        許可 = self._許可 and 外部読取許可 is not False
        session = self.セッション(セッションID)
        with self._会話ロック[セッションID]:
            公開条件 = 公開送信条件((入力文, *(項["入力"] for 項 in session._発話), セッションID))
            audit = self.監査台帳.開始(入力文, セッションID, self._前監査.get(セッションID, ""),
                                      安全表現=公開条件.安全表現)
            audit.経路設定("HDS通常運用")
            if self.要求単位外部読取:
                audit.記録("要求条件", "外部読取", "要求許可-v2", {"指定": 外部読取許可},
                           {"許可": 許可, "要求単位": True, "管理方針": self.公開取得方針})
            else:
                audit.記録("要求条件", "外部読取", "要求許可-v1", {"許可": 許可}, {"要求単位": False})
            結果 = session.応答(入力文, 外部読取許可=許可)
            for row in (結果.追跡 or {}).get("作用", ()):
                audit.記録("HDS作用", row["作用ID"], 運用版,
                    {"署名": row["作用入力署名"]},
                    {"状態": row["作用状態"], "状態差": row["状態差"], "理由": row["理由"]})
            audit.記録("HDS終端", "HDS実行主体", 運用版, 入力文,
                         {"状態": 結果.状態, "本文": 結果.本文, "理由": 結果.理由})
            status = "APPROVE" if 結果.成立 else "FAIL" if 結果.状態 == "FAIL" else "SUSPEND"
            record = audit.確定(結果.本文, status)
            self._前監査[セッションID] = record.ルートハッシュ
            from dataclasses import replace
            安全参照 = tuple(replace(項, **{名: 公開条件.安全表現(getattr(項, 名))
                               for 名 in ("識別子", "題名", "出典", "URL", "本文")})
                             for 項 in (結果.結果.参照 if 結果.結果 else ()))
            return 製品応答(公開条件.安全表現(セッションID), 公開条件.安全表現(結果.本文), status, "HDS通常運用", record.追跡ID,
                    record.ルートハッシュ, 安全参照,
                    tuple(row["能力"] for row in (結果.追跡 or {}).get("能力試行", ())),
                    {"HDS": 公開条件.安全表現(結果.追跡), "HDS終端": 結果.状態, "公開表現版": 安全監査表現版})
