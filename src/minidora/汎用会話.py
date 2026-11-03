"""目的理解・複数素材計画・実行監督・回答・確認の継続を閉じる会話入口。

有限な会話意味を実装する。成功成果、ユーザー発話、未解決目的、訂正を区別する。
"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import asdict,dataclass,replace
from threading import Lock
import re
from .会話意味 import 会話要求,比較対象,意味目的,意味指紋
from .会話解釈 import 会話を解釈,HDS会話を照合
from .会話作用契約 import 会話作用群
from .会話能力接続 import 会話能力群
from .集合会話接続 import 数量集合モジュール, 集合作用群
from .命題能力接続 import 命題能力群, 命題作用群
from .文脈命題接続 import 文脈命題能力群, 文脈命題作用群, 文脈資料群
from .文脈命題 import 文脈判定
from .命題会話解釈 import HDS命題を照合
from .命題解釈 import 命題を読む
from .会話実行監督 import 会話実行監督
from .会話回答 import 会話回答版,回答記録整合
from .役割計画 import 役割計画器
from .統合実行 import 統合セッション
from .能力意味カタログ import 能力意味カタログ
from .目的計画 import 目的計画器
from .HDS目的射影 import HDSから目的要求,_HDS照合
from .HDS構文化器 import 公開HDSコンパイラ
from .能力合成 import 合成計画,合成工程,素材参照,_結果辞書,_符号化
from .応答構成 import 能力結果を復元
from .製品版.型 import 能力結果,参照資料
from .知識取得 import 知識取得器
from .製品版.検索 import SearXNG検索供給器

汎用会話版='MINIDORA-汎用会話-v0.5'


@dataclass(frozen=True, slots=True)
class 汎用会話応答:
    状態: str
    本文: str
    理由: str = ''
    追跡: dict | None = None
    結果: 能力結果 | None = None
    @property
    def 成立(self): return self.状態=='合格'
    def 辞書化(self):
        return {'状態':self.状態,'本文':self.本文,'理由':self.理由,'追跡':deepcopy(self.追跡),
                '結果':_結果辞書(self.結果) if self.結果 else None}


class 汎用会話セッション:
    def __init__(self, セッションID, *, 取得器=None, 外部読取許可=False, 最大発話=128, 監査改善=False):
        if type(外部読取許可) is not bool or type(最大発話) is not int or not 1<=最大発話<=512:
            raise ValueError('会話許可・発話上限不正')
        if type(監査改善) is not bool: raise ValueError('監査改善接続はbool')
        self._監査改善=None; self._監査改善焦点=False
        backend=取得器 if 取得器 is not None else 知識取得器(SearXNG検索供給器())
        added=(*会話能力群(backend,外部許可=外部読取許可),数量集合モジュール().登録(),*命題能力群(),*文脈命題能力群())
        if 監査改善:
            from .監査改善計画 import 改善統合能力群
            added=(*added,*改善統合能力群())
        self.統合=統合セッション(セッションID,外部読取許可=外部読取許可,取得器=backend,追加能力=added)
        if 監査改善:
            from .監査改善会話 import 監査改善会話セッション
            self._監査改善=監査改善会話セッション(セッションID,統合=self.統合,最大発話=min(最大発話,128))
        extra={r.モジュール.名前 for r in added}
        base=tuple(r for r in self.統合.能力一覧() if r['名前'] not in extra)
        self._旧計画=目的計画器(能力意味カタログ(base))
        self._計画=役割計画器((*会話作用群(),*集合作用群(),*命題作用群(),*文脈命題作用群()),self.統合.能力一覧())
        self._監督=会話実行監督(self._計画,self.統合)
        self._許可=外部読取許可; self._上限=最大発話
        self._資料={};self._発話=();self._保留=None;self._最後目的=None
        self._最後結果=None;self._最後依存={};self._最後有効=True;self._最後起点=None
        self._ロック=Lock()

    def 監査改善に対応する(self, 原文, *, 継続許可=True):
        """製品入口の読取専用振分。別話題の省略依頼を以前の成果へ結び付けない。"""
        if type(継続許可) is not bool:
            return False
        return self._監査改善 is not None and self._監査改善.対応する(
            原文, 継続許可=継続許可 and self._監査改善焦点)

    def 状態(self):
        if not self._ロック.acquire(blocking=False): raise ValueError('会話の処理中')
        try:
            return {'発話':deepcopy(self._発話),'保留目的':asdict(self._保留) if self._保留 else None,
                    '最後の成果有効':self._最後有効,'資料版':{k:意味指紋(_結果辞書(v)) for k,v in self._資料.items()},
                    '採用起点':asdict(self.統合.起点()),
                    **({'監査改善':self._監査改善.状態()} if self._監査改善 is not None else {})}
        finally: self._ロック.release()

    def _資料入力(self, materials, *, 更新=True):
        if type(materials) is not dict or len(materials)>32:
            raise ValueError('名前付き資料の数・型')
        候補=deepcopy(self._資料);changed=[]
        for name,value in materials.items():
            if type(name) is not str or not 0<len(name)<=128 or any(ord(c)<32 for c in name):
                raise ValueError('資料名不正')
            if self._監査改善 is not None and name in self._監査改善.状態()['資料版']:
                raise ValueError('監査改善資料と通常資料の名前衝突。資料名を分ける')
            _結果辞書(value)
            if not value.成立: raise ValueError('資料の入力が不成立')
            if name in 候補 and not 更新: raise ValueError('既存資料の変更には更新を明示する')
            rawhash=意味指紋(_結果辞書(value))
            if name not in 候補 or 意味指紋(_結果辞書(候補[name]))!=rawhash:
                changed.append(name)
            候補[name]=deepcopy(value)
        if len(候補)>32 or len(_符号化({k:_結果辞書(v) for k,v in 候補.items()}))>1000000:
            raise ValueError('保持資料の数・サイズ上限')
        self._資料=候補
        if any(name in self._最後依存 for name in changed): self._最後有効=False
        return changed

    def _素材(self):
        資料={}
        for name,value in self._資料.items():
            if value.参照: 資料[name]=deepcopy(value);continue
            情報源=参照資料('会話資料:'+意味指紋({'名前':name,'本文':value.本文}),name,'利用者提供資料',本文=value.本文)
            資料[name]=replace(value,参照=(情報源,))
        return 資料

    @staticmethod
    def _形式(value):
        if value.データ.get('形式') in ('JSON','CSV'): return value.データ['形式']
        text=value.本文.lstrip()
        if text.startswith(('{','[')): return 'JSON'
        if ',' in text.split('\n',1)[0]: return 'CSV'
        raise ValueError('比較資料はJSON又は見出し付きCSVを指定する')

    def _比較目的(self, request, materials):
        values=[]
        for target in request.対象:
            if target.資料 not in materials: raise ValueError('資料がない:'+target.資料)
            values.append({'資料':target.資料,'形式':self._形式(materials[target.資料]),
                '属性':request.属性,'単位':request.単位,'行条件':dict(target.行条件)})
        if len(values)!=2: raise ValueError('比較は二つの対象役割が必要')
        return 意味目的('比較回答',{'左':values[0],'右':values[1],
                    '時点差':request.時点差,'詳細':request.詳細})

    def _集合目的(self, request, materials):
        targets=[]
        for target in request.対象:
            if request.補助.get('供給')=='取得':
                targets.append({'主題':target.資料,'属性':request.属性,'単位':request.単位})
                continue
            if target.資料 not in materials: raise ValueError('資料がない:'+target.資料)
            targets.append({'資料':target.資料,'形式':self._形式(materials[target.資料]),
                            '属性':request.属性,'単位':request.単位,'行条件':dict(target.行条件)})
        aux=request.補助
        return 意味目的('集合回答',{'対象':targets,'供給':aux.get('供給','資料'),'操作':list(aux['操作']),
                        '時点差':request.時点差,'詳細':request.詳細,'形式':aux['形式'],
                        '手順':aux['手順'],'選別':aux['選別'],'除外資料':list(aux['除外資料'])})

    def _元成果(self):
        if self._最後結果 is None: return ()
        if self._最後起点!=self.統合.起点(): self._最後有効=False
        if not self._最後有効: raise ValueError('参照資料が更新されたため前回成果は失効。元の目的を再実行する')
        if not 回答記録整合(self._最後結果): raise ValueError('最後の回答記録が不整合')
        return tuple((f'前回:{i}',能力結果を復元(v)) for i,v in enumerate(self._最後結果.データ['元結果']))

    def _旧実行(self, request, materials, original_ir, stop, start):
        canonical=request.補助['射影文']; prior=self._元成果() if re.search('それ|その結果',canonical) else ()
        ir=original_ir if canonical==request.原文 else 公開HDSコンパイラ().コンパイル(canonical)
        射影=HDSから目的要求(ir,materials,前回成果=prior)
        if not 射影.成立: raise ValueError(射影.理由)
        # 元の入力での条件や意味損失も照合する。表層射影文だけを正本にしない。
        if canonical!=request.原文:
            literals=tuple(m.span() for m in re.finditer('「[^「」]*」',request.原文))
            refs=tuple(m.span() for m in re.finditer('それ|その結果',request.原文))
            equations=tuple((a,b) for a,b in literals if '=' in request.原文[a:b])
            _HDS照合(original_ir,literals,refs,equations)
        plan=self._旧計画.計画する(射影.要求)
        if not plan.成立: raise ValueError(plan.理由)
        steps=list(plan.計画.工程);資料=deepcopy(plan.資料)
        資料['会話:回答指示']=能力結果(True,'得られた成果を回答へ構成する')
        資料['会話:回答設定']=能力結果(True,'',データ={'詳細':request.詳細})
        steps.append(合成工程('会話:回答',('会話回答構成',),'会話:回答指示',
            tuple(素材参照('工程',k) for k in plan.計画.出力工程),'会話:回答設定'))
        packed=self.統合.準備(合成計画(tuple(steps),('会話:回答',)),資料,依頼文=request.原文)
        if packed.起点!=start: raise ValueError('意味解釈後に会話状態が変わった')
        結果=self.統合.実行(packed,停止要求=stop)
        return 結果,{'射影文':canonical,'作用経路':plan.作用経路,'実行印':結果.実行.ルートハッシュ if 結果.実行 else ''}

    def _改訂(self, request):
        target=self._保留 or self._最後目的
        if target is None or target.行為 not in ('比較','集合','取得'):
            raise ValueError('対応する確認待ち又は訂正対象の目的がない')
        key,value=request.補助['欄'],request.補助['値']
        kw={}
        if key=='単位': kw['単位']=value
        elif key=='属性': kw['属性']=value
        elif key=='年' and target.行為 in ('比較','集合') and target.補助.get('供給')!='取得':
            if not re.fullmatch('[0-9]{4}',value): raise ValueError('年は4桁')
            kw={'対象':tuple(replace(t,行条件=tuple({**dict(t.行条件),'年':value}.items())) for t in target.対象),'時点差':False}
        elif key in ('左の年','右の年') and target.行為=='比較':
            if not re.fullmatch('[0-9]{4}',value): raise ValueError('年は4桁')
            i=0 if key=='左の年' else 1
            targets=list(target.対象);filters=dict(targets[i].行条件);filters['年']=value
            targets[i]=replace(targets[i],行条件=tuple(filters.items()));kw={'対象':tuple(targets),'時点差':True}
        else: raise ValueError('対象目的に適用できない訂正')
        return replace(target,原文=request.原文,補助={**target.補助,'改訂元':target.補助.get('改訂元',target.原文)},
            対応=(('目的の明示改訂',0,len(request.原文)),),**kw).固定複製()

    def 応答(self, 原文, 資料=None, *, 外部読取許可=False, 停止要求=None):
        if not self._ロック.acquire(blocking=False): return 汎用会話応答('保留','同じ会話の処理中です。')
        追跡={'版':汎用会話版};request=None
        try:
            if type(外部読取許可) is not bool or 外部読取許可 and not self._許可:
                raise ValueError('外部読取の許可範囲外')
            self.統合._停止(停止要求)
            if len(self._発話)>=self._上限 and 原文 not in ('/初期化','会話を初期化して'): raise ValueError('会話発話上限。初期化してから再開する')
            if type(原文) is not str or not 原文.strip() or len(原文)>8192:
                raise ValueError('会話原文の型・上限')
            start=self.統合.起点()
            if self._監査改善 is not None and self._監査改善.対応する(原文,継続許可=self._監査改善焦点):
                # 通常資料と追加資料を暗黙に混合・置換しない。
                if 資料 is not None: raise ValueError('追加能力の資料は種類付き登録で明示する')
                from .監査改善会話解釈 import 改善発話を解釈
                command=改善発話を解釈(原文)
                if command.get('資料') in self._資料:
                    raise ValueError('通常資料と監査改善資料の名前衝突。資料名を分ける')
                ir=公開HDSコンパイラ().コンパイル(原文)
                if ir.原文!=原文: raise ValueError('HDS原文と追加会話の原文が不一致')
                追跡['HDS原文']=ir.原文;追跡['HDS保持']=asdict(ir)
                追跡['HDS照合範囲']='原文保持。意味解釈は有限会話契約による。一般HDS意味照合の完了ではない'
                結果=self._監査改善.応答(原文,停止要求=停止要求)
                追跡['監査改善']=結果.追跡 or {}
                self._監査改善焦点=True
                return self._返す(原文,汎用会話応答(結果.状態,結果.本文,結果.理由,追跡,結果.結果))
            self._監査改善焦点=False
            if 資料 is not None: 追跡['更新資料']=self._資料入力(資料)
            request=会話を解釈(原文,tuple(self._資料))
            if request.行為 in ('登録','更新'):
                value=能力結果(True,request.補助['本文'])
                name=request.対象[0].資料
                if request.行為=='更新' and name not in self._資料: raise ValueError('更新対象の資料がない')
                changed=self._資料入力({name:value},更新=request.行為=='更新')
                追跡['更新資料']=changed
                return self._返す(原文,汎用会話応答('合格',f'資料「{name}」を{request.行為}しました。内容を事実として採用したわけではありません。',追跡=追跡))
            if request.行為=='初期化':
                self.統合.初期化();self._資料={};self._発話=();self._保留=None
                self._最後目的=None;self._最後結果=None;self._最後依存={};self._最後有効=True;self._最後起点=None
                if self._監査改善 is not None:
                    from .監査改善会話 import 監査改善会話セッション
                    self._監査改善=監査改善会話セッション(self.統合.起点().セッションID,統合=self.統合,最大発話=min(self._上限,128))
                self._監査改善焦点=False
                return 汎用会話応答('合格','会話と資料を初期化しました。',追跡=追跡)
            ir=公開HDSコンパイラ().コンパイル(原文)
            追跡['HDS原文']=ir.原文
            追跡['HDS保持']=asdict(ir)
            if request.行為 in ('命題照合','命題選択','命題訂正','資料命題選択','命題取得'):
                追跡['HDS局所解消']=HDS命題を照合(ir,request)
            elif request.行為!='既存目的':
                追跡['HDS局所解消']=HDS会話を照合(ir,request,文脈解消=self._最後結果 is not None or self._保留 is not None)
            if request.行為=='会話':
                phrase=request.補助['発話']
                body={'こんにちは':'こんにちは。','こんばんは':'こんばんは。','ありがとう':'どういたしまして。',
                    '外部禁止':'この発話では外部検索を実行しません。',
                    'できることを教えて':'数式の計算・微積分、JSON/CSVの数値比較・最大8対象の一覧/合計/平均/最大/最小、資料の変換、数値記載の取得、確認への返答と訂正に対応しています。資料からの命題導出・支持/反証/矛盾/未確定の区別・発言や信念の帰属・局所照応・資料解釈の確認・明示許可時の公開本文による命題検討にも対応しています。自由作文・一般的な原因推定は未対応です。'}[phrase]
                if phrase=='できることを教えて' and self._監査改善 is not None:
                    body+='追加の種類付き資料では、有限仮説検討・明示ブールモデルの介入比較・有界照応・未解釈を保持する本文読解に対応します。抽出記載の局所導出は資料全体の判定ではなく、一般原因認定や自由作文でもありません。'
                return self._返す(原文,汎用会話応答('合格',body,追跡=追跡))
            if request.行為 in ('命題選択','資料命題選択'):
                pending=self._保留
                if pending is None or pending.行為 not in ('命題照合','命題取得') or '保留資料版' not in pending.補助:
                    raise ValueError('選択待ちの命題解釈がない')
                current={t.資料:意味指紋(_結果辞書(self._資料[t.資料])) for t in pending.対象}
                if current!=pending.補助['保留資料版']:
                    raise ValueError('確認中に資料が更新されたため、元の問いを再指定する')
                selection_key='資料候補' if request.行為=='資料命題選択' else '候補'
                if selection_key=='資料候補' and not pending.補助.get('文脈'):
                    raise ValueError('資料解釈の選択待ちではない')
                request=replace(pending,原文=原文,補助={**pending.補助,selection_key:request.補助['番号']},対応=())
            elif request.行為=='命題訂正':
                prior=self._保留 or self._最後目的
                if prior is None or prior.行為 not in ('命題照合','命題取得'):
                    raise ValueError('訂正する命題の問いがない')
                aux={k:v for k,v in prior.補助.items() if k not in ('保留資料版','資料候補','文脈')}
                aux.update({'問い':request.補助['問い'],'候補':0,'命題範囲':request.補助['命題範囲']})
                if prior.行為=='命題取得':
                    from .命題会話解釈 import 命題会話を解釈
                    repl=命題会話を解釈('公開資料から「'+request.補助['問い']+'」を検討して',())
                    aux['取得要求']=repl.補助['取得要求']
                request=replace(prior,原文=原文,補助=aux,対応=())
            if request.行為 in ('命題照合','命題取得'):
                candidates=命題を読む(request.補助['問い'])
                追跡['命題解釈候補']=[asdict(c) for c in candidates]
                index=request.補助['候補']
                if index==0 and len(candidates)>1:
                    versions={t.資料:意味指紋(_結果辞書(self._資料[t.資料])) for t in request.対象}
                    self._保留=replace(request,補助={**request.補助,'保留資料版':versions})
                    body='問いの意味が複数あります。解釈を指定してください。\n'+'\n'.join(
                        str(i+1)+'. '+c.読み for i,c in enumerate(candidates))+'\n例：解釈は1です'
                    return self._返す(原文,汎用会話応答('確認待ち',body,'意味候補未確定',追跡))
                if index==0: index=1
                if type(index) is not int or not 1<=index<=len(candidates):
                    raise ValueError('命題の解釈番号が範囲外')
                request=replace(request,補助={**request.補助,'候補':index})
            if request.行為=='命題照合':
                names=tuple(t.資料 for t in request.対象)
                material_map=self._素材()
                docs=文脈資料群(tuple(material_map[n] for n in names),names)
                if any(d['文脈拡張'] for d in docs):
                    選択肢=request.補助.get('資料候補',0)
                    preview=文脈判定(docs,request.補助['問い'],request.補助['候補'],選択肢)
                    追跡['資料解釈検討']=preview
                    request=replace(request,補助={**request.補助,'文脈':True,'資料候補':選択肢})
                    if not 選択肢 and preview['判定']=='解釈依存':
                        versions={n:意味指紋(_結果辞書(self._資料[n])) for n in names}
                        self._保留=replace(request,補助={**request.補助,'保留資料版':versions})
                        lines=['資料の読みで結論が異なります。資料解釈を指定してください。']
                        for case in preview['場合別']:
                            reading=' / '.join(x['記載']+'：'+x['読み'] for x in case['選択'])
                            lines.append(str(case['場合'])+'. '+reading+' → '+case['判定結果']['判定'])
                        lines.append('例：資料解釈は1です')
                        return self._返す(原文,汎用会話応答('確認待ち','\n'.join(lines),'資料意味候補未確定',追跡))
            if request.行為 in ('訂正','確認返答'):
                request=self._改訂(request);追跡['目的改訂']=asdict(request)
            materials=self._素材();追跡['会話意味']=asdict(request)
            if request.行為=='再表現':
                self._元成果()
                if self._最後結果 is None: raise ValueError('再表現する成果がない')
                資料={'a':self._最後結果,'i':能力結果(True,'前回成果を意味保持して再表現'),
                      'c':能力結果(True,'',データ={'詳細':request.詳細,**request.補助})}
                plan=合成計画((合成工程('再表現',('会話再表現',),'i',(素材参照('入力','a'),),'c'),),('再表現',))
                packed=self.統合.準備(plan,資料,依頼文=原文)
                if packed.起点!=start: raise ValueError('再表現中に会話状態が変わった')
                結果=self.統合.実行(packed,停止要求=停止要求)
            elif request.行為=='既存目的':
                結果,detail=self._旧実行(request,materials,ir,停止要求,start);追跡.update(detail)
            else:
                if request.行為=='命題取得':
                    if request.外部禁止 or not (外部読取許可 and self._許可):
                        self._保留=request
                        return self._返す(原文,汎用会話応答('確認待ち','公開本文の命題検討には明示した外部読取許可が必要です。','権限不足',追跡))
                    goal=意味目的('取得命題回答',{k:request.補助[k] for k in ('問い','候補','取得要求','形式','手順')}|{'詳細':request.詳細})
                elif request.行為=='命題照合':
                    contextual=request.補助.get('文脈',False)
                    goal=意味目的('文脈命題回答' if contextual else '命題回答',
                        {'資料':[t.資料 for t in request.対象],
                        '問い':request.補助['問い'],'候補':request.補助['候補'],
                        '詳細':request.詳細,'形式':request.補助['形式'],'手順':request.補助['手順'],
                        **({'資料候補':request.補助['資料候補']} if contextual else {})})
                elif request.行為=='比較':
                    goal=self._比較目的(request,materials)
                elif request.行為=='集合':
                    if request.補助.get('供給')=='取得' and (request.外部禁止 or not (外部読取許可 and self._許可)):
                        self._保留=request
                        return self._返す(原文,汎用会話応答('確認待ち','複数主題の外部取得には明示した外部読取許可が必要です。','権限不足',追跡))
                    if request.補助.get('供給')=='取得':
                        from .証拠統合 import _単位
                        if request.単位 not in _単位:
                            self._保留=request
                            return self._返す(原文,汎用会話応答('確認待ち','取得する数量の対応単位を指定してください。例：単位はVです。','入力不足',追跡))
                    goal=self._集合目的(request,materials)
                elif request.行為=='取得':
                    if request.外部禁止 or not (外部読取許可 and self._許可):
                        self._保留=request
                        return self._返す(原文,汎用会話応答('確認待ち','外部取得は許可されていません。提供資料での処理、又は明示した外部読取許可が必要です。','権限不足',追跡))
                    goal=意味目的('取得回答',{'主題':request.補助['主題'],'属性':request.属性,'単位':request.単位,'詳細':request.詳細})
                else: raise ValueError('未接続の会話行為')
                supervised=self._監督.実行(goal,materials,原文=原文,
                    外部許可=外部読取許可 and not request.外部禁止,停止要求=停止要求,要求起点=start)
                結果=supervised.応答
                追跡['再計画']=[asdict(f) for f in supervised.失敗]
                追跡['試行']=list(supervised.試行)
                if not 結果.成立:
                    signature=supervised.失敗[-1] if supervised.失敗 else None
                    if request.行為 not in ('命題照合','命題取得') and signature and signature.種別 in ('入力不足','意味未確定'):
                        self._保留=request
                        prompt=('比較する単位を指定してください。例：単位は円です。' if signature.種別=='入力不足' else
                                '対象の行又は時点が一意に定まりません。共通年なら「年は2025です」と指定してください。' if request.行為=='集合' else
                                '対象の行又は時点が一意に定まりません。例：左の年は2025です。右の年は2026です。')
                        return self._返す(原文,汎用会話応答('確認待ち',prompt,signature.理由,追跡))
            if not 結果.成立:
                return self._返す(原文,汎用会話応答(結果.状態,'処理を完了できませんでした。'+結果.理由,結果.理由,追跡))
            output=結果.出力[0][1]
            # 回答構成Module自身が採用前に意味を再構成している。ここでは同じ結果を保持。
            self._最後結果=deepcopy(output);self._最後有効=True;self._最後起点=結果.更新後
            if request.行為!='再表現':
                self._最後目的=request;self._保留=None
                used=set() if request.行為=='集合' and request.補助.get('供給')=='取得' else {t.資料 for t in request.対象} if request.行為 in ('比較','集合','命題照合') else set(self._資料) if request.行為=='既存目的' else set()
                self._最後依存={k:意味指紋(_結果辞書(self._資料[k])) for k in used}
            追跡['採用起点']=asdict(結果.更新後)
            return self._返す(原文,汎用会話応答('合格',output.本文,追跡=追跡,結果=output))
        except (ValueError,TypeError,KeyError,AttributeError,RecursionError,InterruptedError) as exc:
            状態='中止' if isinstance(exc,InterruptedError) else '保留'
            return self._返す(原文,汎用会話応答(状態,'処理を確定しませんでした。'+str(exc),str(exc),追跡))
        finally: self._ロック.release()

    def _返す(self, original, response):
        # 入力と診断の保持は、成果・事実の採用とは独立。元資料や全実行記録の複写はしない。
        if type(original) is str and len(original)<=8192 and len(self._発話)<self._上限:
            event={'入力':original,'状態':response.状態,'応答':response.本文,
                   '入力の扱い':'利用者発話。事実として未採用'}
            self._発話=(*self._発話,event)
        return response
