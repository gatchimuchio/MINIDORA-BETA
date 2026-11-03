"""HDSを唯一の実行主体とする、資料・知識・会話・能力の通常運用入口。"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import dataclass, replace
from pathlib import Path
from threading import Lock
import os
import tempfile
import time
from ..HDS実行主体 import HDS実行主体, HDS実行状態, HDS作用結果, HDS作用状態, HDS関数作用, HDS作用供給器, HDS終端
from ..統合駆動_v2.政策 import HDS運用政策
from ..統合駆動_v2.検証 import HDS検証器
from ..能力合成 import _符号化, 合成計画
from ..製品版.型 import 能力結果, 参照資料
from ..監査改善会話解釈 import JSONを厳格に読む, 名前を確認
from ..会話回答 import 回答記録整合
from ..監査改善接続 import 改善回答を検査
from .能力 import 運用能力目録
from .値 import 運用版, 正準, 指紋, 結果を保存, 結果を復元, 文字を検査, 封緘, 開封, 計画を保存
from .解釈 import 依頼を解釈, 計画を構成, 型付き要求を保存, 構造を保存
from .工程 import 工程供給, 現行計画
from .原記録 import 運用原記録, 原記録成果を復元
from .手順形成 import 手順形成供給, 目的鍵, 手順を束縛, 形成結果を検査, 手順資産を検査
from .知識資産 import 知識を形成, 資産を検査
from .数量生成 import 数量回答を検査
from .内容構成 import 資料文章を検査
from .関係内容 import 関係回答を検査
from .保存移行 import 移行履歴を検査, 旧成果履歴を照合
from .一般依頼 import 一般要求を検査
from .会話目的束 import 会話目的を読む
from .会話回答束 import 会話束回答を検査
from .関係読解 import 関係要求を検査
from ..取得実行制御 import 公開取得上限, 共有取得制御
from ..公開送信条件 import 公開送信条件


def _目的の全資料範囲(目的, 所有セッション):
    if not 目的:
        return False
    if 目的.get("方式") in ("一般資料", "関係資料"):
        return 目的["要求"]["範囲"] == "全資料"
    if 目的.get("方式") == "一般会話目的束":
        from .会話文脈 import 会話保存文脈を読む
        群 = 会話目的を読む(目的["要求"], 所有セッション=所有セッション,
                      期待原文=目的["原文"], **会話保存文脈を読む(目的))
        return any(行["要求"]["範囲"] == "全資料" for 行 in 群)
    return False


def _束の登録資料(成果):
    対応 = {}
    for 行 in 成果.データ["目的成果"]:
        子 = 結果を復元(行["子成果"])
        for 名, 資料 in 子.データ["構造"]["資料群"].items():
            原 = 結果を復元(資料["資料"])
            if not any(参照.識別子.startswith("運用資料:" + 名 + ":") for 参照 in 原.参照):
                continue
            if 名 in 対応 and 対応[名] != 資料["資料"]:
                raise ValueError("会話目的間の登録原資料が競合しています")
            対応[名] = 資料["資料"]
    return 対応


@dataclass(frozen=True, slots=True)
class 運用応答:
    状態: str
    本文: str
    理由: tuple[str, ...] = ()
    結果: 能力結果 | None = None
    HDS結果: object = None
    追跡: dict | None = None

    @property
    def 成立(self):
        return self.状態 == HDS終端.採用.value

    def 辞書化(self):
        return {"状態": self.状態, "本文": self.本文, "理由": list(self.理由),
                "結果": 結果を保存(self.結果) if self.結果 else None, "追跡": deepcopy(self.追跡)}


class HDS運用セッション:
    """セッションは入出力・保存の所有境界。意味解釈・工程選択・採否はHDSへ置く。

    任意自然言語理解を名乗らず、現行部品の対応意味を同じ実行経路へ接続する。
    データ中の契約・命令を登録コードへ昇格させない。
    """
    def __init__(self, セッションID="default", *, 外部読取許可=False, 取得器=None,
                 追加能力=(), 追加役割作用=(), 再利用=True, 最大作用回数=128, 最大発話=128, 手順形成=True,
                 取得上限=None):
        文字を検査(セッションID, "セッションID", 128)
        if type(外部読取許可) is not bool or type(再利用) is not bool:
            raise TypeError("運用設定はbool")
        if type(最大作用回数) is not int or not 4 <= 最大作用回数 <= 512:
            raise ValueError("最大作用回数は4..512")
        if type(最大発話) is not int or not 1 <= 最大発話 <= 512:
            raise ValueError("最大発話は1..512")
        self.ID, self.外部読取許可 = セッションID, 外部読取許可
        self.最大作用回数, self.最大発話 = 最大作用回数, 最大発話
        if 取得上限 is not None and not isinstance(取得上限, 公開取得上限):
            raise TypeError("運用の取得上限の型不正")
        self.取得上限 = 取得上限
        self.目録 = 運用能力目録(セッションID, 外部読取許可=外部読取許可, 取得器=取得器,
                              追加能力=追加能力, 追加役割作用=追加役割作用, 再利用=再利用)
        if type(手順形成) is not bool:
            raise TypeError("手順形成はbool")
        self.手順形成 = 手順形成
        self._原記録 = 運用原記録(セッションID)
        self.目録.原記録庫 = self._原記録.庫
        self._知識資産 = {}
        self._形成手順 = {}
        self._資料 = {}
        self._旧資料 = []
        self._前回目的 = None
        self._保留目的 = None
        self._前回結果 = None
        self._前回依存 = {}
        self._焦点有効 = False
        self._前回知識範囲 = None
        self._移行履歴 = []
        self._発話 = []
        self._経験 = []
        self._ロック = Lock()

    def 能力一覧(self):
        return deepcopy(self.目録.一覧())

    def _有効(self):
        if _目的の全資料範囲(self._前回目的, self.ID):
            if set(self._前回依存) != set(self._資料):
                return False
        return (self._焦点有効 and self._前回結果 is not None
                and (self._前回知識範囲 is None or self._前回知識範囲 == self._知識範囲印())
                and all(k in self._資料 and self._資料[k]["版"] == v for k, v in self._前回依存.items()))

    def _会話原記録を読む(self, 識別子, 前束印):
        """前束の値を保存自身で認定せず、実庫の原成果から証明候補を作る。"""
        import json
        from ..HDS会話契約 import 会話契約版
        if self._原記録.庫.起点().セッションID != self.ID:
            raise ValueError("前採用会話と実原記録庫の所有が不一致です")
        原記録 = self._原記録.庫.原記録(識別子)
        # 庫が返す参照ビューから原履歴の行を一意に取得する。
        群 = [項 for 事象 in self._原記録.保存()["庫"]["履歴"] for 項 in 事象["追加"]
              if 項["識別子"] == 識別子]
        if len(群) != 1 or 群[0]["種別"] != "成果":
            raise ValueError("会話文脈の成果原記録が一意ではありません")
        行 = 群[0]
        保存結果 = 原記録成果を復元(行)
        結果 = 結果を復元(保存結果)
        包 = 結果.データ.get("契約包")
        記録 = 行["内容"]["データ"]
        if (結果.データ.get("種別") != "会話目的束回答" or type(包) is not dict
                or 包.get("版") != 会話契約版 or 包.get("所有セッション") != self.ID
                or 包.get("意味束印") != 前束印 or 結果.データ.get("所有セッション") != self.ID):
            raise ValueError("会話文脈と実成果の版・所有・包印が不一致です")
        ビュー = json.loads(包["証明正準JSON"])
        if 記録.get("原要求") != ビュー["原文"] or type(記録.get("依存版")) is not dict:
            raise ValueError("会話文脈と実成果の原要求・依存が不一致です")
        役割辺 = {"要求対象", "要求観点", "要求関係", "問い方向", "要求範囲", "条件適用", "表現適用"}
        参照名 = set(包["目的参照"])
        for 辺 in ビュー["関係"]:
            if 辺["種別"] in 役割辺:
                参照名.update((*辺["始点"], *辺["終点"]))
        参照 = [{"座標ID": 項["座標ID"], "役割": 項["種別"], "値印": 指紋(項["内容"]["値"])}
               for 項 in ビュー["座標"] if 項["座標ID"] in 参照名]
        if len(参照) > 64:
            raise ValueError("会話文脈の役割参照数が64件を超えています")
        証明 = {"前束印": 包["意味束印"], "所有セッション": self.ID,
                "原記録ID": 識別子, "依存版": deepcopy(記録["依存版"]), "参照": 参照}
        return deepcopy(包), 証明, 保存結果, 原記録["現行"]

    @staticmethod
    def _命令証明を作る(包, 証明):
        from hashlib import sha256
        # 実庫から取り出した包の原要求を再検査し、自己申告の保存証明を使わない。
        import json
        原文=json.loads(包["証明正準JSON"])["原文"]
        印=sha256(原文.encode("utf-8")).hexdigest()
        if 印 != 包["原文印"]:
            raise ValueError("歴史命令の実原要求印が不一致です")
        return {**deepcopy(証明), "版":"HDS会話命令履歴証明-v1",
                "用途":"命令再実行", "原要求印":印}

    def _会話文脈引数を復元(self, 包):
        """実庫から親とその直接参照を再発行し、保存証明との比較へ供与する。"""
        from ..HDS会話契約 import 会話契約を検査
        from .会話文脈 import 会話保存文脈を読む
        束縛=包.get("文脈束縛")
        if type(束縛) is not list or len(束縛)>1:
            raise ValueError("会話文脈の元源包が一意ではありません")
        if not 束縛:
            return 会話保存文脈を読む({})
        文脈証明,命令証明=[],[]
        def 再発行(元):
            親,証明,_,_=self._会話原記録を読む(元["原記録ID"],元["前束印"])
            if 元.get("用途")=="命令再実行":
                証明=self._命令証明を作る(親,証明)
                群=命令証明
            elif "用途" in 元:
                raise ValueError("保存会話証明の用途が未対応です")
            else:
                群=文脈証明
            if not any(項["前束印"]==証明["前束印"] for 項 in 群):
                群.append(証明)
            return 親
        元=束縛[0]
        前包=再発行(元)
        for 先行 in 前包["文脈束縛"]:
            再発行(先行)
        文脈=会話保存文脈を読む({"前採用包":None if 元.get("用途")=="命令再実行" else 前包,
                  "文脈証明":文脈証明,
                  "前命令包":前包 if 元.get("用途")=="命令再実行" else None,
                  "命令証明":命令証明})
        証明引数={鍵:文脈[鍵] for 鍵 in ("文脈証明","命令証明")}
        会話契約を検査(前包,所有セッション=self.ID,**証明引数)
        会話契約を検査(包,所有セッション=self.ID,**証明引数)
        return 文脈

    def _会話文脈を復元(self, 包):
        """旧普通文脈の二欄APIを保つ。歴史用途を普通証明へ投影しない。"""
        文脈=self._会話文脈引数を復元(包)
        if 文脈["前命令包"] is not None or 文脈["命令証明"]:
            raise ValueError("命令履歴を旧普通文脈APIへ変換しません")
        return 文脈["前採用包"],文脈["文脈証明"]

    def _会話実成果を照合(self, *, 現行):
        """保存前目的を実原成果と照合する。現行と歴史は呼出側が指定する。"""
        from .工程 import _会話束HDS対応を検査
        if self._前回結果 is None or not self._前回目的 or self._前回目的.get("方式")!="一般会話目的束":
            return None
        前回=結果を復元(self._前回結果)
        包=前回.データ["契約包"]
        文脈=self._会話文脈引数を復元(包)
        if not 会話束回答を検査(前回,所有セッション=self.ID,**文脈):
            raise ValueError("前会話の意味・原資料・文章が不整合です")
        _会話束HDS対応を検査(self._前回目的,包,self.ID,**文脈)
        対応=[行 for 事象 in self._原記録.保存()["庫"]["履歴"] for 行 in 事象["追加"]
              if 行["種別"]=="成果" and 原記録成果を復元(行)==self._前回結果
              and 行["内容"]["データ"].get("原要求")==self._前回目的["原文"]
              and 行["内容"]["データ"].get("依存版")==self._前回依存
              and (not 現行 or self._原記録.庫.原記録(行["識別子"])["現行"])]
        if not 対応:
            raise ValueError("前会話と同セッションの実成果原記録が不一致です")
        _,自己証明,_,_=self._会話原記録を読む(対応[-1]["識別子"],包["意味束印"])
        return 包,自己証明,文脈

    @staticmethod
    def _直接会話証明を選ぶ(包, 自己証明, 文脈):
        """次の作用で必要な自己と直接祖先だけ。履歴を無制限に複製しない。"""
        普通=[] if "用途" in 自己証明 else [deepcopy(自己証明)]
        命令=[deepcopy(自己証明)] if "用途" in 自己証明 else []
        for 元 in 包["文脈束縛"]:
            群=文脈["命令証明"] if 元.get("用途")=="命令再実行" else 文脈["文脈証明"]
            対応=[項 for 項 in 群 if 項["前束印"]==元["前束印"] and 項["原記録ID"]==元["原記録ID"]]
            if len(対応)!=1:
                raise ValueError("次作用の直接参照証明が一意ではありません")
            (命令 if 元.get("用途")=="命令再実行" else 普通).append(deepcopy(対応[0]))
        if len(普通)+len(命令)>4:
            raise ValueError("次作用の会話証明全体上限4")
        return 普通,命令

    def _前採用会話を証明(self):
        """現行成果だけを普通照応の源として供与する。"""
        if not self._有効():
            return None
        照合=self._会話実成果を照合(現行=True)
        if 照合 is None:
            return None
        包,自己,文脈=照合
        普通,命令=self._直接会話証明を選ぶ(包,自己,文脈)
        値={"契約包":deepcopy(包),"文脈証明":普通}
        if 命令:
            値["命令証明"]=命令
        return 値

    def _前命令候補を証明(self, 前採用=None):
        """失効後も実採用命令の履歴を供与する。成果の現行性は認定しない。"""
        if 前採用 is not None:
            包=前採用["契約包"]
            対応=[項 for 項 in 前採用["文脈証明"] if 項["前束印"]==包["意味束印"]]
            if len(対応)!=1:
                raise ValueError("現行命令の自己原記録証明が一意ではありません")
            自己=対応[0]
            普通=[項 for 項 in 前採用["文脈証明"] if 項 is not 自己]
            命令=[self._命令証明を作る(包,自己),*deepcopy(前採用.get("命令証明",[]))]
        else:
            照合=self._会話実成果を照合(現行=False)
            if 照合 is None:
                return None
            包,自己,文脈=照合
            普通,命令=self._直接会話証明を選ぶ(包,self._命令証明を作る(包,自己),文脈)
        if len(普通)+len(命令)>4:
            raise ValueError("歴史命令の証明全体上限4")
        return {"契約包":deepcopy(包),"文脈証明":deepcopy(普通),"命令証明":命令}

    def _命令提示原文を証明(self, 候補):
        """採用命令の適用辺を実庫で遡り、利用者の提示原文だけを発行する。"""
        from ..HDS会話契約 import 会話契約を検査
        from .会話目的束 import 会話前目的対応を読む
        if 候補 is None:
            return {}
        源 = 候補["契約包"]
        自己 = [項 for 項 in 候補["命令証明"] if 項["前束印"] == 源["意味束印"]]
        if len(自己) != 1:
            raise ValueError("提示命令の自己実原記録が一意ではありません")
        源ID, 結果 = 自己[0]["原記録ID"], {}
        for 源目的 in 源["目的参照"]:
            包, 目的ID, 記録ID, 訪問 = 源, 源目的, 源ID, set()
            while True:
                鍵 = (包["意味束印"], 目的ID)
                if 鍵 in 訪問:
                    raise ValueError("提示命令の実原記録系譜が循環しています")
                訪問.add(鍵)
                実包, _, _, _ = self._会話原記録を読む(記録ID, 包["意味束印"])
                if 指紋(実包) != 指紋(包):
                    raise ValueError("提示命令系譜と実庫の元包が不一致です")
                文脈 = self._会話文脈引数を復元(包)
                ビュー = 会話契約を検査(包, 所有セッション=self.ID,
                                文脈証明=文脈["文脈証明"], 命令証明=文脈["命令証明"])
                行群 = 会話目的を読む(包, 所有セッション=self.ID, **文脈)
                行 = next((項 for 項 in 行群 if 項["目的ID"] == 目的ID), None)
                if 行 is None:
                    raise ValueError("提示命令の実目的が欠落しています")
                if not 行["要求"]["提示本文"] and 行["要求"]["対象"] != ["提示本文"]:
                    break
                座標 = {項["座標ID"]: 項 for 項 in ビュー["座標"]}
                材料 = [座標[辺["始点"][0]] for 辺 in ビュー["関係"]
                      if 辺["種別"] == "材料適用" and 辺["終点"] == [目的ID]]
                if 材料:
                    if len(材料) != 1:
                        raise ValueError("提示命令の原材料適用が一意ではありません")
                    原 = self._原記録.庫.原記録(記録ID)["内容"]["データ"]["原要求"]
                    範囲 = 材料[0]["原文範囲"]
                    if (材料[0]["種別"] != "情報源.会話入力区分" or 材料[0]["内容"]["値"] != "提示本文"
                            or 範囲 is None or 原[範囲[0]:範囲[1]] != 材料[0]["内容"]["原文"]):
                        raise ValueError("提示命令の原材料が利用者原文へ束縛されていません")
                    結果[源目的] = {"所有セッション": self.ID, "前束印": 源["意味束印"],
                        "元目的ID": 源目的, "原記録ID": 源ID, "材料原記録ID": 記録ID,
                        "材料原文印": 包["原文印"], "原要求": 原, "原文範囲": list(範囲)}
                    break
                対応 = [項 for 項 in 会話前目的対応を読む(包, 所有セッション=self.ID, **文脈)
                      if 項["目的ID"] == 目的ID]
                if not 対応:
                    break
                if len(対応) != 1:
                    raise ValueError("提示命令の元目的対応が一意ではありません")
                元 = 対応[0]
                包 = 文脈["前命令包" if 元["方式"] == "再実行" else "前採用包"]
                if 包 is None or 包["意味束印"] != 元["前束印"]:
                    raise ValueError("提示命令の実系譜が不一致です")
                目的ID, 記録ID = 元["元目的ID"], 元["原記録ID"]
        return 結果

    def 状態(self):
        if not self._ロック.acquire(blocking=False):
            raise ValueError("会話の処理中")
        try:
            return {"版": 運用版, "セッションID": self.ID, "資料": deepcopy(self._資料),
                    "前回有効": self._有効(), "保留目的": deepcopy(self._保留目的),
                    "発話数": len(self._発話), "経験数": len(self._経験),
                    "知識資産数": len(self._知識資産), "形成手順数": len(self._形成手順),
                    "原記録資料": deepcopy(self._原記録.資料対応), "原記録改訂": self._原記録.庫.起点().改訂,
                    "能力": self.能力一覧(), "再利用": self.目録.再利用庫.統計()}
        finally:
            self._ロック.release()

    def 資料を登録(self, 名前, 本文, *, 種類="資料", データ=None, 更新=False):
        名前を確認(名前)
        文字を検査(本文, "資料本文", 100_000)
        if type(更新) is not bool or 種類 not in ("資料", "知識", "本文", "命題", "仮説", "介入"):
            raise ValueError("資料の登録条件不正")
        if データ is None:
            データ = {}
        if type(データ) is not dict:
            raise TypeError("資料データはdict")
        # 本文が長くても、制御入力と資料は別の欄で渡す。本文を命令へ埋めない。
        request = {"方式": "管理", "行為": "更新" if 更新 else "登録", "名前": 名前,
                   "本文": 本文, "種類": 種類, "データ": 正準(データ)}
        raw = f'資料「{名前}」を{"更新" if 更新 else "登録"}する'
        return self._応答(raw, 管理要求=request)

    def 文脈を取得(self, *, 必須資料=(), 検索語=(), 最大バイト数=32768):
        if type(必須資料) not in (list, tuple) or type(検索語) not in (list, tuple):
            raise TypeError("必須資料・検索語は文字列の配列で指定する")
        return self._応答("原記録の文脈を取得する", 文脈要求={"必須資料": list(必須資料),
                         "検索語": list(検索語), "最大バイト数": 最大バイト数})

    def 応答(self, 原文, *, 外部読取許可=False, 停止要求=None, 期限秒=None):
        return self._応答(原文, 外部読取許可=外部読取許可, 停止要求=停止要求, 期限秒=期限秒)

    def 目的を実行(self, 要求, *, 停止要求=None):
        packed = 型付き要求を保存(要求)
        return self._応答(要求.原文, 型付き目的=packed, 停止要求=停止要求)

    def 役割目的を実行(self, 目的, *, 原文, 外部読取許可=False, 停止要求=None):
        from ..会話意味 import 意味目的
        from dataclasses import asdict
        if type(目的) is not 意味目的:
            raise TypeError("意味目的が必要")
        目的.鍵()
        return self._応答(原文, 意味目的=正準(asdict(目的)), 外部読取許可=外部読取許可, 停止要求=停止要求)

    def 合成を実行(self, 原文, 計画, 資料, *, 外部読取許可=False, 停止要求=None):
        if type(資料) is not dict or len(資料) > 128:
            raise ValueError("合成資料の型・数")
        packed = {"計画": 計画を保存(計画), "資料": {k: 結果を保存(v) for k, v in 資料.items()}}
        return self._応答(原文, 明示合成=packed, 外部読取許可=外部読取許可, 停止要求=停止要求)

    def _管理作用(self, 状態):
        current = 現行計画(状態)
        if current is None or current[1]["方式"] != "管理":
            return ()
        key, packet = current
        def execute(s):
            values = dict(s.成果)
            task = packet["解釈"]
            行為 = task["行為"]
            delta = None
            if 行為 in ("登録", "更新"):
                name = 名前を確認(task["名前"])
                if 行為 == "登録" and name in values["運用入力"]["資料"]:
                    raise ValueError("既存資料は明示的に更新してください")
                if 行為 == "更新" and name not in values["運用入力"]["資料"]:
                    raise ValueError("未登録資料は更新できない")
                body = 文字を検査(task["本文"], "資料本文", 100_000)
                payload = task.get("データ", {})
                if body.lstrip().startswith(("{", "[")) and task["種類"] in ("資料", "知識"):
                    JSONを厳格に読む(body)
                version = 指紋({"種類": task["種類"], "本文": body, "データ": payload})
                ref = 参照資料("運用資料:" + name + ":" + version, name, "利用者提供" + task["種類"], 本文=body)
                value = 能力結果(True, body, 参照=(ref,), データ=payload)
                record = {"種類": task["種類"], "本文": body, "データ": payload,
                          "版": version, "結果": 結果を保存(value)}
                prospective = deepcopy(values["運用入力"]["資料"])
                prospective[name] = record
                if len(prospective) > 32 or len(_符号化(prospective)) > 2_000_000:
                    raise ValueError("保持資料の容量上限")
                delta = {"行為": 行為, "名前": name, "資料": record}
                if task["種類"] == "知識":
                    delta["知識資産"] = 知識を形成(name, record)
                response = 能力結果(True, f'資料「{name}」を{行為}しました。', データ={"版": version, "種類": task["種類"], "事実認定": False})
            elif 行為 == "初期化":
                delta = {"行為": "初期化"}
                response = 能力結果(True, "会話・資料・再利用結果を初期化しました。")
            elif 行為 == "会話":
                phrase = task.get("発話", "")
                body = ("HDSが資料・知識・目的・各能力の中間成果を保持して処理します。"
                        "対応入口は数式、JSON/CSV、数量比較・集計、本文・命題・仮説・介入の検討、"
                        "条件訂正と再表現です。未対応の自由文は残差として返します。") if phrase == "できることを教えて" else (
                        "外部読取は各依頼の明示許可がある場合だけ行います。" if phrase == "外部禁止" else "どうぞ。")
                response = 能力結果(True, body)
            else:
                raise ValueError("未対応の管理作用")
            proof = {"原文": values["運用入力"]["原文"], "計画鍵": key, "計画印": 指紋(packet),
                     "目録": self.目録.ハッシュ, "出力": [], "出力印": [], "回答印": 指紋(結果を保存(response))}
            return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"運用:応答成立"}),
                    解消残差=frozenset({"運用:成果未構成"}),
                    成果=(("運用応答", 結果を保存(response)), ("運用採用対応", proof), ("運用更新", delta)),
                    理由=("利用者操作をHDS内で検査し更新候補を構成",))
        return (HDS関数作用("運用/管理", execute, 入力状態=("運用:計画構成済",),
                    出力状態=("運用:応答成立",), 解消対象=("運用:成果未構成",), 読取成果=("運用入力", key), 契約版=運用版),)

    def _知識範囲印(self):
        return 指紋({名前: 資産["資料版"] for 名前, 資産 in self._知識資産.items()})

    def _知識を読む目的(self, 状態):
        解釈 = dict(状態.成果).get("運用解釈", {})
        if 解釈.get("方式") == "継続":
            解釈 = 解釈["目的"]
        if 解釈.get("方式") == "知識横断":
            return True
        return (解釈.get("方式") == "会話" and 解釈.get("依頼", {}).get("行為") == "再表現"
                and self._前回知識範囲 is not None)

    def _全資料を読む目的(self, 状態):
        解釈 = dict(状態.成果).get("運用解釈", {})
        if 解釈.get("方式") == "継続":
            解釈 = 解釈["目的"]
        if 解釈.get("方式") in ("一般再表現", "関係再表現"):
            解釈 = self._前回目的 or {}
        return _目的の全資料範囲(解釈, self.ID)

    def _訂正失効成果IDを読む(self, 状態):
        """当回の型付き訂正と実採用庫が一致した元成果だけを失効させる。"""
        from .会話文脈 import 会話入力文脈を読む
        from .会話目的束 import 会話前目的対応を読む
        値 = dict(状態.成果)
        解釈 = 値.get("運用解釈", {})
        if 解釈.get("方式") != "一般会話目的束":
            return ()
        入力, 包 = 値["運用入力"], 解釈["要求"]
        文脈 = 会話入力文脈を読む(包, 入力)
        対応 = 会話前目的対応を読む(包, 所有セッション=self.ID,
                              期待原文=入力["原文"], **文脈)
        if not any(項["方式"] == "訂正" for 項 in 対応):
            return ()
        源 = 文脈["前採用包"]
        if 源 is None or not 入力["前回有効"]:
            raise ValueError("訂正の元成果が有効な採用成果ではありません")
        自己 = [項 for 項 in 文脈["文脈証明"] if 項["前束印"] == 源["意味束印"]]
        if len(自己) != 1:
            raise ValueError("訂正の元成果原記録が一意ではありません")
        実包, 実証明, 実結果, 現行 = self._会話原記録を読む(自己[0]["原記録ID"], 源["意味束印"])
        if (not 現行 or 指紋(実包) != 指紋(源) or 指紋(実証明) != 指紋(自己[0])
                or 指紋(実結果) != 指紋(入力["前回結果"])):
            raise ValueError("訂正の元成果と実庫の現行採用対応が不一致です")
        return (自己[0]["原記録ID"],)

    def _会話入力所有を検査(self, 状態):
        """最終同期でも、保存宣言を実庫から再発行した独立証明へ照合する。"""
        from .会話文脈 import 会話入力文脈を読む
        値 = dict(状態.成果)
        解釈 = 値.get("運用解釈", {})
        if 解釈.get("方式") != "一般会話目的束":
            return
        入力, 包 = 値["運用入力"], 解釈["要求"]
        文脈 = 会話入力文脈を読む(包, 入力)
        if 文脈["前採用包"] is not None:
            if 指紋(入力["前採用会話"]) != 指紋(self._前採用会話を証明()):
                raise ValueError("会話入力の現行証明が実採用庫へ一致しません")
        elif 文脈["前命令包"] is not None:
            実候補 = self._前命令候補を証明(self._前採用会話を証明())
            if (指紋(入力["前命令候補"]) != 指紋(実候補)
                    or 指紋(入力.get("命令提示原文", {})) != 指紋(self._命令提示原文を証明(実候補))):
                raise ValueError("会話入力の歴史命令・提示原文が実採用庫へ一致しません")

    def _記憶同期作用(self, 状態):
        if "運用:応答成立" not in 状態.成立状態 or "運用:記憶同期済" in 状態.成立状態:
            return ()
        def 構成(s):
            値 = dict(s.成果)
            self._会話入力所有を検査(s)
            回答 = 結果を復元(値["運用応答"])
            候補 = self._原記録.同期候補(値["運用入力"], 値.get("運用更新"), 回答,
                                        値["運用採用対応"].get("資料依存", {}), 知識範囲=self._知識を読む目的(s), 全資料範囲=self._全資料を読む目的(s),
                                        失効成果ID=self._訂正失効成果IDを読む(s))
            return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"運用:記憶同期済"}),
                    成果=(("運用原記録候補", 候補.保存()),), 理由=("原文・成果・版・依存の同期候補を構成",))
        return (HDS関数作用("運用/原記録同期", 構成, 入力状態=("運用:応答成立", "運用:形成処理済"),
                  出力状態=("運用:記憶同期済",), 読取成果=("運用入力", "運用解釈", "運用応答", "運用採用対応"), 契約版=運用版),)

    def _記憶を検証(self, 状態, _):
        try:
            値 = dict(状態.成果)
            if not 形成結果を検査(状態, self.目録):
                return False
            self._会話入力所有を検査(状態)
            更新 = 値.get("運用更新")
            if 更新 and "知識資産" in 更新:
                資産を検査(更新["知識資産"], 更新["資料"])
            候補 = self._原記録.同期候補(値["運用入力"], 更新, 結果を復元(値["運用応答"]),
                                        値["運用採用対応"].get("資料依存", {}), 知識範囲=self._知識を読む目的(状態), 全資料範囲=self._全資料を読む目的(状態),
                                        失効成果ID=self._訂正失効成果IDを読む(状態))
            return 候補.保存() == 値["運用原記録候補"]
        except (ValueError, TypeError, KeyError):
            return False

    def _応答(self, 原文, *, 型付き目的=None, 明示合成=None, 管理要求=None, 意味目的=None, 文脈要求=None,
             外部読取許可=False, 停止要求=None, 期限秒=None):
        入口開始 = time.monotonic()
        if not self._ロック.acquire(blocking=False):
            return 運用応答("SUSPEND", "同じ会話を処理中です。", ("同時更新を拒否",))
        try:
            文字を検査(原文)
            if type(外部読取許可) is not bool or 外部読取許可 and not self.外部読取許可:
                raise ValueError("外部読取は設定と各依頼の両方で許可が必要")
            if 停止要求 is not None and not callable(停止要求):
                raise TypeError("停止要求はcallable")
            if 期限秒 is not None and (type(期限秒) not in (int, float) or not 0 < 期限秒 <= 3600):
                raise ValueError("期限秒は0超3600以下の有限数")
            if self.取得上限 is not None:
                期限秒 = min(期限秒, self.取得上限.全体期限秒) if 期限秒 is not None else self.取得上限.全体期限秒
            if len(self._発話) >= self.最大発話 and 原文 not in ("/初期化", "会話を初期化して"):
                raise ValueError("発話容量上限。初期化又は別セッションを利用してください")
            self.目録.照合()
            前採用 = self._前採用会話を証明()
            前命令 = self._前命令候補を証明(前採用)
            提示命令原文 = self._命令提示原文を証明(前命令)
            inputs = 正準({"原文": 原文, "セッションID": self.ID, "資料": self._資料,
                     "前回目的": self._前回目的, "保留目的": self._保留目的,
                     "前回結果": self._前回結果, "前回有効": self._有効(), "前回依存": self._前回依存,
                     "前採用会話": 前採用, "前命令候補": 前命令, "命令提示原文": 提示命令原文,
                     "型付き目的": 型付き目的, "明示合成": 明示合成, "管理要求": 管理要求, "意味目的": 意味目的,
                     "外部許可": 外部読取許可, "知識資産": self._知識資産,
                     "文脈要求": 文脈要求, "原記録資料": self._原記録.資料対応,
                     "原記録起点": 構造を保存(self._原記録.庫.起点())})
            if len(_符号化(inputs)) > 8_000_000:
                raise ValueError("運用入力の保存上限")
            def interpret(s):
                データ = dict(s.成果)["運用入力"]
                try:
                    parsed = 依頼を解釈(データ) if データ["管理要求"] is None else {**データ["管理要求"], "原文": データ["原文"]}
                    return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"運用:依頼解釈済"}),
                        解消残差=frozenset({"運用:依頼未解釈"}), 成果=(("運用解釈", parsed),), 理由=("依頼の意味を構文化",))
                except (ValueError, TypeError, KeyError) as exc:
                    return HDS作用結果(HDS作用状態.保留, 成果=(("運用診断", {"段階": "依頼解釈", "理由": str(exc)}),), 理由=(str(exc),))
            def plan(s):
                values = dict(s.成果)
                try:
                    packet = 計画を構成(values["運用解釈"], values["運用入力"], self.目録)
                    記録 = self._形成手順.get(目的鍵(values["運用解釈"]))
                    if 記録 is not None and packet["方式"] != "管理":
                        復元計画 = 手順を束縛(記録, values["運用解釈"], values["運用入力"], self.目録)
                        # 現行の目的解釈・計画と一致する手順だけ使う。保存物は信頼根にしない。
                        if 復元計画 is not None:
                            比較 = {鍵: 値 for 鍵, 値 in packet.items() if 鍵 != "解釈"}
                            復元比較 = {鍵: 値 for 鍵, 値 in 復元計画.items() if 鍵 != "解釈"}
                            if 比較 == 復元比較:
                                packet = 復元計画
                                packet["手順由来"] = 記録["SHA256"]
                    return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"運用:計画構成済"}),
                        解消残差=frozenset({"運用:計画未構成"}), 成果=(("運用計画:0", packet),), 理由=("要求から作用の依存計画を構成",))
                except (ValueError, TypeError, KeyError) as exc:
                    return HDS作用結果(HDS作用状態.保留, 成果=(("運用診断", {"段階": "目的計画", "理由": str(exc)}),), 理由=(str(exc),))
            形成供給 = 手順形成供給(self.目録, 有効=self.手順形成, 最大作用回数=self.最大作用回数)
            actions = (
                HDS関数作用("運用/依頼構文化", interpret, 出力状態=("運用:依頼解釈済",),
                           解消対象=("運用:依頼未解釈",), 読取成果=("運用入力",), 契約版=運用版),
                HDS関数作用("運用/目的計画", plan, 入力状態=("運用:依頼解釈済",), 出力状態=("運用:計画構成済",),
                           解消対象=("運用:計画未構成",), 読取成果=("運用入力", "運用解釈"), 契約版=運用版))
            started = 入口開始 if self.取得上限 is not None else time.monotonic()
            def stop():
                flag = 停止要求() if 停止要求 is not None else False
                if type(flag) is not bool:
                    raise TypeError("停止要求はboolを返す必要がある")
                return flag or (期限秒 is not None and time.monotonic() - started >= 期限秒)
            公開条件 = 公開送信条件((原文, *(項["入力"] for 項 in self._発話))) if self.取得上限 is not None else None
            取得制御 = (共有取得制御(self.取得上限, 停止要求=stop, 開始時刻=started, 公開条件=公開条件)
                      if self.取得上限 is not None else None)
            bridge = 工程供給(self.目録, 停止要求=stop, 取得制御=取得制御)
            policy = HDS運用政策(探索深さ=64, 最大探索状態=8192, 最大内部生成=256,
                                 許可権限=("外部読取",) if 外部読取許可 else (), 自動形成=False)
            initial = HDS実行状態(目的=(原文,), 要求状態=frozenset({"運用:応答成立", "運用:形成処理済", "運用:記憶同期済"}),
                        残差=frozenset({"運用:依頼未解釈", "運用:計画未構成", "運用:成果未構成"}),
                        成果=(("運用入力", inputs),))
            結果 = HDS実行主体(actions, 最大作用回数=self.最大作用回数, 政策=policy,
                        作用供給器=(HDS作用供給器("能力工程", bridge, 運用版), HDS作用供給器("管理操作", self._管理作用, 運用版),
                                     HDS作用供給器("手順形成", 形成供給, 運用版), HDS作用供給器("原記録同期", self._記憶同期作用, 運用版)),
                        最終検証器=(HDS検証器("運用目的と実成果", bridge.最終検証, 運用版), HDS検証器("原記録と資料版", self._記憶を検証, 運用版)), 停止要求=stop).実行(initial)
            values = dict(結果.状態.成果)
            追跡 = {"版": 運用版, "目録": self.目録.ハッシュ, "入力印": 指紋(inputs),
                     "終端": 結果.終端.value, "理由": list(結果.理由), "阻害": 構造を保存(結果.阻害履歴),
                     "作用": [構造を保存(h) for h in 結果.履歴], "計装": 構造を保存(結果.計装),
                     "候補計画": [k for k in values if k.startswith("運用計画:") and "/" not in k],
                     "能力試行": [v for k, v in values.items() if "/試行/" in k],
                     "残差": sorted(結果.状態.残差), "再利用": self.目録.再利用庫.統計(),
                     "手順形成": {"状態": values.get("運用形成結果", {}).get("理由", "未到達"),
                                  "再現工程数": sum(k.startswith("運用形成:") for k in values),
                                  "手順再利用": bool(現行計画(結果.状態) and 現行計画(結果.状態)[1].get("手順由来"))}}
            if 取得制御 is not None:
                追跡["公開取得資源"] = 取得制御.記録()
            accepted = 結果.終端 == HDS終端.採用
            answer = 結果を復元(values["運用応答"]) if accepted else None
            parsed = values.get("運用解釈")
            if accepted:
                # HDS採用後にだけセッションへ反映する。別の意味判定は行わない。
                原記録候補 = 運用原記録.復元(values["運用原記録候補"], self.ID)
                delta = values.get("運用更新")
                if delta and delta["行為"] == "初期化":
                    self._資料 = {}; self._旧資料 = []; self._前回目的 = self._保留目的 = self._前回結果 = None
                    self._前回依存 = {}; self._焦点有効 = False; self._発話 = []; self._経験 = []
                    self.目録.再利用庫.消去()
                elif delta:
                    name = delta["名前"]
                    if name in self._資料:
                        self._旧資料.append({"名前": name, **deepcopy(self._資料[name])})
                        self._旧資料 = self._旧資料[-32:]
                    self._資料[name] = deepcopy(delta["資料"])
                elif parsed and parsed["方式"] != "管理":
                    current_key, packet = 現行計画(結果.状態)
                    effective = parsed["目的"] if parsed["方式"] == "継続" else parsed
                    is_reword = (effective.get("方式") in ("数量再表現", "一般再表現", "関係再表現") or (effective.get("方式") == "会話" and effective.get("依頼", {}).get("行為") == "再表現"))
                    if not is_reword:
                        self._前回目的 = deepcopy(effective)
                        self._前回知識範囲 = self._知識範囲印() if effective.get("方式") == "知識横断" else None
                    self._保留目的 = None
                    self._前回結果 = 結果を保存(answer)
                    # 回答に伝播した参照と、計画で実際に使う登録資料の両方から依存を残す。
                    self._前回依存 = deepcopy(values["運用採用対応"]["資料依存"])
                    self._焦点有効 = True
                    experience = {"目的": effective.get("方式"), "目録": self.目録.ハッシュ,
                        "計画印": 指紋(packet), "資料版": self._前回依存,
                        "作用列": [row["能力"] for row in 追跡["能力試行"] if row.get("採否") == "合格"],
                        "範囲": "この入力・契約で完遂した記録。一般法則又は自動学習とはしない"}
                    self._経験.append(正準(experience)); self._経験 = self._経験[-64:]
                self._原記録 = 原記録候補
                self.目録.原記録庫 = self._原記録.庫
                if delta and delta["行為"] == "初期化":
                    self._知識資産 = {}; self._形成手順 = {}; self._前回知識範囲 = None; self._移行履歴 = []
                elif delta:
                    if "知識資産" in delta:
                        self._知識資産[delta["名前"]] = deepcopy(delta["知識資産"])
                    else:
                        self._知識資産.pop(delta["名前"], None)
                形成 = values.get("運用形成結果", {}).get("手順")
                if 形成 is not None:
                    self._形成手順[形成["目的鍵"]] = deepcopy(形成)
                    if len(self._形成手順) > 64:
                        del self._形成手順[next(iter(self._形成手順))]
                body = answer.本文
                reasons = 結果.理由
            else:
                is_reword = (parsed is not None and (parsed.get("方式") in ("数量再表現", "一般再表現", "関係再表現") or (parsed.get("方式") == "会話"
                             and parsed.get("依頼", {}).get("行為") == "再表現")))
                if parsed and parsed["方式"] not in ("管理",) and not is_reword:
                    effective = parsed["目的"] if parsed["方式"] == "継続" else parsed
                    self._保留目的 = deepcopy(effective)
                if not is_reword:
                    self._焦点有効 = False
                diagnostic = values.get("運用診断", {}).get("理由")
                failures = [v.get("理由", "") for v in 追跡["能力試行"] if v.get("理由")]
                reasons = tuple(x for x in (diagnostic, *failures, *結果.理由) if x)
                body = "回答は未確定です。" + (reasons[0] if reasons else "必要な作用又は観測が不足しています。")
            self._発話.append({"入力": 原文, "状態": 結果.終端.value, "本文": body,
                               "追跡印": 指紋(追跡)})
            return 運用応答(結果.終端.value, body, tuple(reasons), answer, 結果, 追跡)
        except (ValueError, TypeError, KeyError) as exc:
            return 運用応答("SUSPEND", "処理を確定できません。" + str(exc), (str(exc),))
        finally:
            self._ロック.release()

    def 保存(self):
        if not self._ロック.acquire(blocking=False):
            raise ValueError("処理中の状態は保存できない")
        try:
            状態 = {"版": 運用版, "セッションID": self.ID, "目録": self.目録.ハッシュ,
                     "外部読取許可": self.外部読取許可, "最大作用回数": self.最大作用回数, "最大発話": self.最大発話,
                     "資料": self._資料, "旧資料": self._旧資料, "前回目的": self._前回目的,
                     "保留目的": self._保留目的, "前回結果": self._前回結果,
                     "前回依存": self._前回依存, "焦点有効": self._焦点有効,
                     "発話": self._発話, "経験": self._経験,
                     "原記録": self._原記録.保存(), "知識資産": self._知識資産,
                     "形成手順": self._形成手順, "手順形成": self.手順形成, "前回知識範囲": self._前回知識範囲, "移行履歴": self._移行履歴}
            packed = _符号化(封緘(状態)).decode("utf-8")
            if len(packed.encode()) > 16_000_000:
                raise ValueError("保存上限")
            return packed
        finally:
            self._ロック.release()

    @classmethod
    def 復元(cls, text, *, 外部読取許可=False, 取得器=None, 追加能力=(), 追加役割作用=(), 取得上限=None):
        状態 = 開封(JSONを厳格に読む(text, 最大バイト数=16_000_000))
        required = {"版", "セッションID", "目録", "外部読取許可", "最大作用回数", "最大発話", "資料", "旧資料",
                    "前回目的", "保留目的", "前回結果", "前回依存", "焦点有効", "発話", "経験", "原記録", "知識資産", "形成手順", "手順形成", "前回知識範囲", "移行履歴"}
        if type(状態) is not dict or set(状態) != required or 状態["版"] != 運用版:
            raise ValueError("保存形式・版不一致")
        if 状態["外部読取許可"] is not 外部読取許可:
            raise ValueError("保存データから外部権限を復元しない。呼出側設定と一致が必要")
        obj = cls(状態["セッションID"], 外部読取許可=外部読取許可, 取得器=取得器,
                  追加能力=追加能力, 追加役割作用=追加役割作用,
                  最大作用回数=状態["最大作用回数"], 最大発話=状態["最大発話"], 手順形成=状態["手順形成"],
                  取得上限=取得上限)
        if 状態["目録"] != obj.目録.ハッシュ:
            raise ValueError("保存時の能力・意味契約から変更されている")
        if type(状態["資料"]) is not dict or len(状態["資料"]) > 32 or len(_符号化(状態["資料"])) > 2_000_000:
            raise ValueError("保存資料の型・上限")
        for name, value in 状態["資料"].items():
            名前を確認(name)
            if type(value) is not dict or set(value) != {"種類", "本文", "データ", "版", "結果"}:
                raise ValueError("保存資料の欄不一致")
            if value["版"] != 指紋({k: value[k] for k in ("種類", "本文", "データ")}):
                raise ValueError("保存資料の版不一致")
            文字を検査(value["本文"], "資料本文", 100_000)
            if value["種類"] not in ("資料", "知識", "本文", "命題", "仮説", "介入") or type(value["データ"]) is not dict:
                raise ValueError("保存資料の種類・データ不正")
            ref = 参照資料("運用資料:" + name + ":" + value["版"], name,
                        "利用者提供" + value["種類"], 本文=value["本文"])
            expected = 能力結果(True, value["本文"], 参照=(ref,), データ=value["データ"])
            if 指紋(結果を保存(expected)) != 指紋(value["結果"]):
                raise ValueError("保存資料の本体・参照不一致")
        # 保存された文脈証明を信用せず、先に実原記録庫を復元して再発行する。
        obj._資料 = deepcopy(状態["資料"])
        obj._原記録 = 運用原記録.復元(状態["原記録"], obj.ID)
        obj._原記録.資料を照合(obj._資料)
        obj.目録.原記録庫 = obj._原記録.庫
        旧失効履歴 = False
        if 状態["前回結果"] is not None:
            old = 結果を復元(状態["前回結果"])
            from .会話文脈 import 会話保存文脈を読む
            文脈引数 = (obj._会話文脈引数を復元(old.データ["契約包"])
                       if old.データ.get("種別") == "会話目的束回答" else 会話保存文脈を読む({}))
            if not (回答記録整合(old) or 改善回答を検査(old.データ) or 数量回答を検査(old) or 資料文章を検査(old)
                    or 関係回答を検査(old) or 会話束回答を検査(old, 所有セッション=obj.ID,
                                                    **文脈引数)):
                旧失効履歴 = 旧成果履歴を照合(状態, 状態["前回結果"])
                if not 旧失効履歴:
                    raise ValueError("保存回答の整合不一致")
        if type(状態["焦点有効"]) is not bool or type(状態["前回依存"]) is not dict:
            raise ValueError("保存会話状態の型不正")
        for name in ("前回目的", "保留目的"):
            if 状態[name] is not None and (type(状態[name]) is not dict or type(状態[name].get("方式")) is not str):
                raise ValueError("保存目的の型不正")
            if 状態[name] is not None and 状態[name].get("方式") == "一般資料":
                一般要求を検査(状態[name].get("要求"))
                if 状態[name]["要求"]["版"] == "HDS一般会話要求-v3":
                    原文 = 状態[name].get("原文")
                    if (type(原文) is not str or not 原文
                            or any(項["原発話"] != 原文 for 欄 in ("関係要求", "動作目的要求")
                                   for 項 in 状態[name]["要求"][欄])):
                        raise ValueError("新会話要求と保存目的の原発話が不一致です")
            if 状態[name] is not None and 状態[name].get("方式") == "一般会話目的束":
                from .工程 import _会話束HDS対応を検査
                文脈引数 = obj._会話文脈引数を復元(状態[name]["要求"])
                _会話束HDS対応を検査(状態[name], 状態[name]["要求"], obj.ID,
                               **文脈引数)
                会話目的を読む(状態[name]["要求"], 所有セッション=obj.ID,
                          期待原文=状態[name]["原文"], **文脈引数)
            if 状態[name] is not None and 状態[name].get("方式") == "関係資料":
                関係要求を検査(状態[name].get("要求"))
            if 状態[name] is not None and 状態[name].get("方式") == "関係検討":
                from .関係依頼 import 会話主張HDSを検査
                会話主張HDSを検査(状態[name], obj.ID)
                if name == "前回目的":
                    関係要求を検査(状態[name]["要求"])
        for name, version in 状態["前回依存"].items():
            名前を確認(name)
            if type(version) is not str or len(version) != 64:
                raise ValueError("保存依存の版不正")
        for name, limit in (("旧資料", 32), ("発話", obj.最大発話), ("経験", 64)):
            if type(状態[name]) is not list or len(状態[name]) > limit:
                raise ValueError("保存履歴の型・上限")
        for public, internal in (("資料", "_資料"), ("旧資料", "_旧資料"), ("前回目的", "_前回目的"),
                ("保留目的", "_保留目的"), ("前回結果", "_前回結果"), ("前回依存", "_前回依存"),
                ("焦点有効", "_焦点有効"), ("発話", "_発話"), ("経験", "_経験")):
            setattr(obj, internal, deepcopy(状態[public]))
        if obj._前回結果 is not None:
            前回 = 結果を復元(obj._前回結果)
            if 前回.データ.get("種別") == "数量言語回答":
                構造 = 前回.データ["構造"]
                目的 = obj._前回目的
                if (not 目的 or 目的.get("方式") != "数量言語"
                        or 目的.get("要求") != 構造["要求"]):
                    raise ValueError("数量保存回答と前回目的が不一致")
                if set(構造["資料"]) != set(obj._前回依存):
                    raise ValueError("数量保存回答の資料依存が不一致")
                # 失効した旧回答は履歴として残せる。現行利用する回答は元資料とも照合する。
                if obj._有効() and any(本文 != obj._資料[名]["本文"] for 名, 本文 in 構造["資料"].items()):
                    raise ValueError("数量保存回答と現行原資料が不一致")
        if obj._前回結果 is not None:
            前回 = 結果を復元(obj._前回結果)
            if 前回.データ.get("種別") == "資料文章回答" and not 旧失効履歴:
                構造, 目的 = 前回.データ["構造"], obj._前回目的
                if not 目的 or 目的.get("方式") != "一般資料" or 目的.get("要求") != 構造["要求"]:
                    raise ValueError("資料文章保存回答と前回目的が不一致")
                if 構造["要求"]["版"] in ("HDS一般会話要求-v2", "HDS一般会話要求-v3"):
                    登録名 = {名 for 名, 行 in 構造["資料群"].items()
                              if any(r.識別子.startswith("運用資料:" + 名 + ":") for r in 結果を復元(行["資料"]).参照)}
                    if 登録名 != set(obj._前回依存):
                        raise ValueError("会話資料の登録版依存が不一致")
                    if obj._有効() and any(構造["資料群"][名]["資料"] != obj._資料[名]["結果"] for 名 in 登録名):
                        raise ValueError("会話保存回答と現行原資料が不一致")
                elif 構造["要求"]["範囲"] != "公開取得":
                    if set(構造["資料群"]) != set(obj._前回依存):
                        raise ValueError("資料文章保存回答の資料依存が不一致")
                    if obj._有効() and any(行["資料"] != obj._資料[名]["結果"] for 名, 行 in 構造["資料群"].items()):
                        raise ValueError("資料文章保存回答と現行原資料が不一致")
        if obj._前回結果 is not None:
            前回 = 結果を復元(obj._前回結果)
            if 前回.データ.get("種別") == "会話目的束回答":
                目的 = obj._前回目的
                if (not 目的 or 目的.get("方式") != "一般会話目的束"
                        or 目的.get("要求") != 前回.データ["契約包"]):
                    raise ValueError("会話束保存回答と前回目的が不一致")
                登録資料 = _束の登録資料(前回)
                if set(登録資料) != set(obj._前回依存):
                    raise ValueError("会話束保存回答の登録版依存が不一致")
                if obj._有効() and any(資料 != obj._資料[名]["結果"] for 名, 資料 in 登録資料.items()):
                    raise ValueError("会話束保存回答と現行原資料が不一致")
            if 前回.データ.get("種別") == "関係資料回答":
                構造, 目的 = 前回.データ["構造"], obj._前回目的
                if 目的 is not None and 目的.get("方式") == "関係検討":
                    from .関係依頼 import 会話主張HDSを検査, 主張資料を読む
                    接続 = 会話主張HDSを検査(目的, obj.ID)
                    if (接続["要求"] != 構造["要求"] or 前回.データ["表示形式"] != 目的["要求"]["形式"]
                            or 構造["公開資料群"] != {} or 構造["取得"] is not None):
                        raise ValueError("主張保存回答と原IR要求又は取得範囲が不一致")
                    登録名 = set(接続["資料"]["対象"])
                    if set(obj._前回依存) != 登録名 or set(構造["資料群"]) != set(目的["要求"]["対象"]):
                        raise ValueError("主張保存回答の資料依存が不一致")
                    if 接続["資料"]["方式"] == "提示原文" or obj._有効():
                        原資料 = 主張資料を読む(目的, {"原文": 目的["原文"], "セッションID": obj.ID, "資料": obj._資料})
                        if any(項["資料"] != 結果を保存(原資料[名]) for 名, 項 in 構造["資料群"].items()):
                            raise ValueError("主張保存回答と実原資料が不一致")
                elif not 目的 or 目的.get("方式") != "関係資料" or 目的.get("要求") != 構造["要求"]:
                    raise ValueError("関係保存回答と前回目的が不一致")
                else:
                    if set(構造["資料群"]) != set(obj._前回依存):
                        raise ValueError("関係保存回答の資料依存が不一致")
                    if obj._有効() and any(行["資料"] != obj._資料[名]["結果"] for 名, 行 in 構造["資料群"].items()):
                        raise ValueError("関係保存回答と現行原資料が不一致")
        移行履歴を検査(状態["移行履歴"])
        obj._移行履歴 = deepcopy(状態["移行履歴"])
        if obj._前回結果 is not None:
            前回 = 結果を復元(obj._前回結果)
            束回答 = 前回.データ.get("種別") == "会話目的束回答"
            主張回答 = (前回.データ.get("種別") == "関係資料回答"
                    and obj._前回目的 is not None and obj._前回目的.get("方式") == "関係検討")
            原発話付き回答 = (not 旧失効履歴 and 前回.データ.get("種別") == "資料文章回答"
                        and 前回.データ["構造"]["要求"]["版"] == "HDS一般会話要求-v3")
            if not 旧失効履歴 and 原発話付き回答:
                元対応 = []
                for 事象 in 状態["原記録"]["庫"]["履歴"]:
                    for 行 in 事象["追加"]:
                        if (行["種別"] != "成果"
                                or 行["内容"]["データ"].get("原要求") != obj._前回目的["原文"]
                                or 行["内容"]["データ"].get("依存版") != obj._前回依存):
                            continue
                        元成果 = 結果を復元(原記録成果を復元(行))
                        if (元成果.成立 is True
                                and 元成果.データ.get("種別") == "資料文章回答"
                                and 元成果.データ.get("構造") == 前回.データ["構造"]):
                            元対応.append(行["識別子"])
                if not 元対応:
                    raise ValueError("新会話保存の原発話と実元採用成果が不一致です")
            if (not 旧失効履歴 and (束回答 or 主張回答 or (前回.データ.get("種別") == "資料文章回答"
                    and 前回.データ["構造"]["要求"]["版"] in ("HDS一般会話要求-v2", "HDS一般会話要求-v3")))):
                # 新しい会話結果は、この所有セッションでHDS採用された
                # 原記録にも存在しなければならない。結果だけの移植を拒否する。
                対応 = [行 for 事象 in 状態["原記録"]["庫"]["履歴"] for 行 in 事象["追加"]
                        if 行["種別"] == "成果"
                        and 原記録成果を復元(行) == obj._前回結果
                        and 行["内容"]["データ"].get("依存版") == obj._前回依存
                        and (not (束回答 or 主張回答) or 行["内容"]["データ"].get("原要求") == obj._前回目的["原文"])]
                if not 対応 or (obj._有効() and not any(
                        obj._原記録.庫.原記録(行["識別子"])["現行"] for 行 in 対応)):
                    raise ValueError("会話保存回答と同セッションの採用原記録が不一致")
        obj.目録.原記録庫 = obj._原記録.庫
        資産群 = 状態["知識資産"]
        if type(資産群) is not dict or set(資産群) != {名前 for 名前, 値 in obj._資料.items() if 値["種類"] == "知識"}:
            raise ValueError("保存知識資産と現行資料の対応不一致")
        for 名前, 資産 in 資産群.items():
            if type(資産) is not dict or 資産.get("名前") != 名前:
                raise ValueError("知識資産の名前が不一致")
            資産を検査(資産, obj._資料[名前])
        手順群 = 状態["形成手順"]
        if type(手順群) is not dict or len(手順群) > 64:
            raise ValueError("保存手順の型・上限")
        for 鍵, 手順 in 手順群.items():
            手順資産を検査(手順, obj.目録)
            if (type(手順) is not dict or 手順.get("目的鍵") != 鍵 or 手順.get("目録") != obj.目録.ハッシュ
                    or 手順.get("検証") != "再実行一致"
                    or 手順.get("SHA256") != 指紋({k: v for k, v in 手順.items() if k != "SHA256"})):
                raise ValueError("保存手順の整合不一致")
        範囲 = 状態["前回知識範囲"]
        if 範囲 is not None and (type(範囲) is not str or len(範囲) != 64):
            raise ValueError("保存知識範囲の型不正")
        obj._前回知識範囲 = 範囲
        obj._知識資産, obj._形成手順 = deepcopy(資産群), deepcopy(手順群)
        return obj

    def 保存先へ書く(self, path):
        content = self.保存()
        target = Path(path)
        target.parent.mkdir(parents=True, exist_ok=True)
        fd, temporary = tempfile.mkstemp(prefix=".hds-", dir=target.parent)
        try:
            with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
                stream.write(content); stream.flush(); os.fsync(stream.fileno())
            os.replace(temporary, target)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
