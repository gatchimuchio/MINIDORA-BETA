from __future__ import annotations
from typing import Any
from ..採否 import 実行状態
from .型 import 製品応答, 能力結果
from .監査 import 監査台帳
from .会話状態 import 会話状態庫
from .能力契約 import 能力文脈
from .能力レジストリ import 能力レジストリ, レジストリ版
from .ニュース import ニュースモジュール, RSSニュース供給器
from .要約 import 汎用要約モジュール
from .変換 import 文脈変換モジュール
from .抽出 import 情報抽出モジュール
from .計算 import 計算モジュール
from .基本会話 import 基本会話モジュール
from .知識 import 知識参照モジュール本体, Wikipedia知識供給器
from .検索 import Web検索モジュール本体, SearXNG検索供給器
from .組込モジュール import ニュース能力,要約能力,変換能力,抽出能力,計算能力,基本会話能力,Web検索能力,知識参照能力,模型核能力

製品チャット版="MINIDORA-PRODUCT-CHAT-v8"


def _不採用本文(結果: 能力結果) -> str:
    reason = 結果.保留理由 or 結果.状態.value
    if 結果.状態 == 実行状態.失敗:
        return f"処理に失敗しました。{reason}"
    if 結果.状態 == 実行状態.非適用:
        return f"この経路では処理できませんでした。{reason}"
    return f"回答を確定できませんでした。{reason}"


class 製品ミニドラ:
    def __init__(self,*,基礎ミニドラ:Any=None,ニュース供給器=None,知識供給器=None,検索供給器=None,監査台帳_:監査台帳|None=None,状態庫:会話状態庫|None=None,追加モジュール:tuple=(),汎用会話:bool=False,汎用外部読取許可:bool=False,汎用取得器=None,監査改善:bool=True) -> None:
        if type(汎用会話) is not bool or type(汎用外部読取許可) is not bool:
            raise ValueError("汎用会話と外部許可はbool")
        if type(監査改善) is not bool:
            raise ValueError("監査改善はbool")
        self._改善有効=監査改善
        from threading import Lock
        self._汎用有効=汎用会話; self._汎用許可=汎用外部読取許可
        self._汎用取得器=汎用取得器; self._汎用セッション={}; self._汎用ロック=Lock()
        self.基礎ミニドラ=基礎ミニドラ; self.監査台帳=監査台帳_ or 監査台帳(); self.状態庫=状態庫 or 会話状態庫()
        news=ニュースモジュール(ニュース供給器 or RSSニュース供給器()); 要約=汎用要約モジュール(); transform=文脈変換モジュール(); extract=情報抽出モジュール(); calc=計算モジュール(); basic=基本会話モジュール(); web=Web検索モジュール本体(検索供給器 or SearXNG検索供給器()); knowledge=知識参照モジュール本体(知識供給器 or Wikipedia知識供給器())
        builtin=(ニュース能力(news),要約能力(要約),変換能力(transform),抽出能力(extract),計算能力(calc),基本会話能力(basic),Web検索能力(web),知識参照能力(knowledge),模型核能力(self._模型核))
        self.能力レジストリ=能力レジストリ((*builtin,*追加モジュール))

    def 能力一覧(self)->tuple[str,...]:
        improved=(("明示資料の命題・仮説・介入検討、原文範囲付き読解と訂正・失効",) if self._改善有効 else ())
        if self._汎用有効:
            return ("目的からの数式・文書・コード構造処理", "二資料・指定二時点の数値比較", "最大8対象の数量集合・選別・合計・平均・表・計算手順",
                    "資料命題の条件・否定・量化推論と意味候補確認", "発言・信念の帰属、資料内照応、資料解釈の場合別判定", "不足条件の確認と明示訂正", "根拠と条件を保持する回答", "回復契約に基づく限定再計画",
                    *( ("公開資料の数値記載取得・取得全文の命題検討（明示許可済み）",) if self._汎用許可 else () ), *improved, "完全経路監査")
        return tuple(m.名前 for m in self.能力レジストリ.一覧())+improved+("完全経路監査",)
    def モジュール登録(self,モジュール)->None: self.能力レジストリ.登録(モジュール)
    def モジュール解除(self,name:str)->None: self.能力レジストリ.解除(name)

    def _模型核(self,text:str)->tuple[能力結果,dict[str,Any]]:
        if self.基礎ミニドラ is None:
            return 能力結果(False,"",保留理由='基礎MINIDORA 模型核未接続',採否状態=実行状態.非適用),{}
        if hasattr(self.基礎ミニドラ,"実行"):
            try:
                from minidora.実行系 import 要求
                r=self.基礎ミニドラ.実行(要求(text))
            except Exception as exc:
                追跡={"例外":type(exc).__name__}
                return 能力結果(False,"",保留理由=f"Core実行失敗:{type(exc).__name__}",採否状態=実行状態.失敗),追跡
            value=getattr(r,"値",None); refs=getattr(r,"参照",()) or (); hist=getattr(r,"履歴",()) or ()
            decision=getattr(r,"採否",None); 状態=getattr(decision,"状態",None); reasons=tuple(getattr(decision,"理由",()) or ())
            status=getattr(状態,"value",str(状態 or ""))
            追跡={"値":value,"採否":status,"採否理由":reasons,"参照数":len(refs),"履歴":hist,"言語計画":getattr(r,"言語計画",None),"HDS_IR":repr(getattr(r,"HDS_IR",None))}
            if not isinstance(状態,実行状態):
                return 能力結果(False,"",保留理由='模型核採否状態不正',採否状態=実行状態.失敗,データ=追跡),追跡
            if 状態 != 実行状態.合格:
                reason=" / ".join(str(x) for x in reasons) or f"Core状態:{状態.value}"
                return 能力結果(False,"" if value is None else str(value),根拠=('MINIDORA 模型核採否',),データ=追跡,保留理由=reason,採否状態=状態),追跡
            if value is None:
                return 能力結果(False,"",保留理由='模型核合格結果に値がない',採否状態=実行状態.失敗,データ=追跡),追跡
            return 能力結果(True,str(value),根拠=('MINIDORA 模型核実行結果',),データ=追跡,採否状態=実行状態.合格),追跡
        if hasattr(self.基礎ミニドラ,"応答"):
            try:
                response=str(self.基礎ミニドラ.応答(text)); 追跡={"追跡範囲":"公開応答境界"}
                return 能力結果(True,response,根拠=('MINIDORA 模型核応答',),データ=追跡),追跡
            except Exception as exc:
                追跡={"例外":type(exc).__name__}
                return 能力結果(False,"",保留理由=f"Core応答失敗:{type(exc).__name__}",採否状態=実行状態.失敗),追跡
        return 能力結果(False,"",保留理由='模型核実行契約なし',採否状態=実行状態.非適用),{}

    def 応答(self,入力文:str,*,セッションID:str="default")->製品応答:
        with self.状態庫.排他(セッションID) as st:
            return self._応答_locked(str(入力文 or "").strip(), st)

    def _応答_locked(self,text,st)->製品応答:
        if self._汎用有効 or self._改善対象(text,st):
            return self._汎用応答_locked(text,st)
        audit=self.監査台帳.開始(text,st.セッションID,st.直前監査ハッシュ)
        audit.記録("入力受理","製品チャット",製品チャット版,{"入力":text,"session":st.セッションID},{"履歴件数":len(st.履歴),"直前経路":st.直前経路})
        文脈=能力文脈(text,st.セッションID,st.直前応答,st.直前参照,tuple(st.履歴),{})
        selected=self.能力レジストリ.選択(文脈)
        if selected is None:
            経路="保留"; 結果=能力結果(False,"",保留理由='利用可能モジュールなし'); candidates=()
        else:
            経路=selected.モジュール.名前; candidates=selected.候補
            audit.経路設定(経路); audit.記録("能力選択","能力レジストリ",レジストリ版,text,{"選択":経路,"信頼":selected.信頼,"候補":candidates})
            try: 結果=selected.モジュール.実行(文脈)
            except Exception as exc: 結果=能力結果(False,"",保留理由=f"Module実行失敗:{type(exc).__name__}",採否状態=実行状態.失敗)
            audit.記録("能力実行",selected.モジュール.名前,selected.モジュール.版,text,{"成立":結果.成立,"状態":結果.状態.value,"本文":結果.本文,"保留理由":結果.保留理由,"データ":結果.データ},tuple(結果.根拠))
            if not 結果.成立 and 経路!='基礎模型核' and self.基礎ミニドラ is not None:
                audit.記録('モジュール透過',"能力レジストリ",レジストリ版,{'不成立モジュール':経路,"状態":結果.状態.value,"理由":結果.保留理由},{"次経路":'基礎模型核'})
                経路=f"{経路}→基礎Core"; 結果,_=self._模型核(text)
                audit.記録("能力実行",'基礎模型核',"repository-current",text,{"成立":結果.成立,"状態":結果.状態.value,"本文":結果.本文,"保留理由":結果.保留理由,"データ":結果.データ},tuple(結果.根拠))
        audit.経路設定(経路)
        status=結果.状態.value; body=結果.本文 if 結果.成立 else _不採用本文(結果)
        audit.記録("応答構成","製品チャット",製品チャット版,{"能力結果":結果.成立,"結果状態":status},{"本文":body,"状態":status})
        st.追加("user",text); st.追加("assistant",body); st.直前応答=body; st.直前参照=結果.参照 if 結果.成立 else (); st.直前経路=経路
        audit.記録("会話状態更新","会話状態庫",'conversation-状態-v1',{"履歴追加":2},{"履歴件数":len(st.履歴),"参照件数":len(st.直前参照)})
        record=audit.確定(body,status); st.直前追跡ID=record.追跡ID; st.直前監査ハッシュ=record.ルートハッシュ
        return 製品応答(st.セッションID,body,status,経路,record.追跡ID,record.ルートハッシュ,結果.参照,(経路,),{'能力_candidates':candidates,'結果_状態':status})

    def _改善対象(self,text,st):
        '既存モジュール経路を残し、明示された資料操作と同じ会話の継続だけを接続する。'
        if not self._改善有効:
            return False
        with self._汎用ロック:
            session=self._汎用セッション.get(st.セッションID)
        if session is not None:
            if text in ('/初期化','会話を初期化して'):
                return True
            return session.監査改善に対応する(text,継続許可=st.直前経路=='汎用会話')
        return text.startswith(('命題資料','仮説資料','介入資料','本文資料','監査改善の'))

    def _汎用応答_locked(self,text,st):
        from ..汎用会話 import 汎用会話セッション,汎用会話応答,汎用会話版
        from ..要求解釈 import _正規値
        with self._汎用ロック:
            session=self._汎用セッション.get(st.セッションID)
            if session is None and len(self._汎用セッション)<64:
                session=汎用会話セッション(st.セッションID,取得器=self._汎用取得器,外部読取許可=self._汎用許可,監査改善=self._改善有効)
                self._汎用セッション[st.セッションID]=session
        結果=(session.応答(text,外部読取許可=self._汎用許可) if session is not None else
                汎用会話応答("保留","汎用会話のセッション数上限です。"))
        audit=self.監査台帳.開始(text,st.セッションID,st.直前監査ハッシュ)
        audit.経路設定("汎用会話")
        追跡=_正規値(結果.追跡 or {})
        # 外側の製品ログは完全実行Dataを二重保存せず、その指紋と状態を記録する。
        from ..会話意味 import 意味指紋
        audit.記録("会話統合","汎用会話",汎用会話版,text,
            {"状態":結果.状態,"本文":結果.本文,"理由":結果.理由,"内部追跡印":意味指紋(追跡)})
        st.追加("user",text);st.追加("assistant",結果.本文)
        st.直前応答=結果.本文;st.直前経路="汎用会話"
        st.直前参照=結果.結果.参照 if 結果.結果 else ()
        record=audit.確定(結果.本文,結果.状態)
        st.直前追跡ID=record.追跡ID;st.直前監査ハッシュ=record.ルートハッシュ
        return 製品応答(st.セッションID,結果.本文,結果.状態,"汎用会話",record.追跡ID,
            record.ルートハッシュ,st.直前参照,("汎用会話",),{"汎用追跡":追跡})
