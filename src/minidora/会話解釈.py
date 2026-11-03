"""資料参照・対象属性・単位・行条件・会話行為を組み合わせる局所意味解釈。

原文と引用の範囲を保つ。未対応の条件・尾部を落とさず、未知理解を名付けで済ませない。
"""
from __future__ import annotations
from dataclasses import replace
import re
from .会話意味 import 会話要求, 比較対象
from .会話語彙 import 比較述語,取得述語,詳細表現,簡潔表現,外部禁止表現,数式質問
from .会話句 import 句を分割 as _節


def _名前(text):
    value=text.strip()
    if value.startswith('「') and value.endswith('」'): value=value[1:-1]
    if not value or len(value)>80 or any(c in value for c in '「」\n\r\x00'):
        raise ValueError('対象又は属性の表現が未確定')
    return value


def _末尾(text,words):
    hits=[s for s in words if text.endswith(s)]
    if not hits: return None
    chosen=max(hits,key=len)
    return text[:-len(chosen)].rstrip(),chosen


def 会話を解釈(原文: str, 資料名: tuple[str,...]=()) -> 会話要求:
    if type(原文) is not str or not 原文.strip() or len(原文)>8192:
        raise ValueError('会話原文の型・長さ')
    text=原文.strip()
    register=re.fullmatch(r'資料「([^「」\n]{1,128})」を(登録|更新)(?:：|:|\n)([\s\S]+)',text)
    if register:
        name,kind,body=register.groups()
        return 会話要求(原文,kind,(比較対象(name),),補助={'本文':body},対応=(('資料入力',0,len(原文)),)).固定複製()
    if text in ('/初期化','会話を初期化して'):
        return 会話要求(原文,'初期化').固定複製()
    if text in ('こんにちは','こんばんは','ありがとう','できることを教えて'):
        return 会話要求(原文,'会話',補助={'発話':text}).固定複製()
    from .命題会話解釈 import 命題会話を解釈
    proposition = 命題会話を解釈(原文, 資料名)
    if proposition is not None: return proposition
    from .集合会話解釈 import 集合会話を解釈
    collection=集合会話を解釈(原文,資料名)
    if collection is not None: return collection
    presentation=text.rstrip('。？?')
    presentation=re.sub(r'^(?:それ|その結果)(?:を|の)', '', presentation)
    presentation_options={'表にして':{'形式':'表'},'表で説明して':{'形式':'表'},
                          '文章で説明して':{'形式':'文章'},'計算過程も説明して':{'手順':True},
                          '手順も説明して':{'手順':True}}
    if presentation in presentation_options:
        return 会話要求(原文,'再表現',補助=presentation_options[presentation],対応=(('成果再表現',0,len(原文)),)).固定複製()
    raw_clauses=[原文[a:b].strip().rstrip('？?！!').strip() for a,b in _節(原文)]
    clauses=[c for c in raw_clauses if c]
    detailed=False; forbid=False; task=[]
    for c in clauses:
        prefix='ただし' if c.startswith('ただし') else ''
        plain=c[len(prefix):].strip()
        if plain in 外部禁止表現: forbid=True; continue
        if plain in 詳細表現: detailed=True; continue
        if plain in 簡潔表現: detailed=False; continue
        task.append(c)
    if not task:
        if forbid: return 会話要求(原文,'会話',外部禁止=True,補助={'発話':'外部禁止'}).固定複製()
        if clauses: return 会話要求(原文,'再表現',詳細=detailed).固定複製()
        raise ValueError('会話の目的がない')
    if len(task)==1:
        t=task[0]
        for word in (*詳細表現,*簡潔表現):
            if t in ('それを'+word,'その結果を'+word):
                return 会話要求(原文,'再表現',詳細=word in 詳細表現,対応=(('成果参照',原文.find('それ') if 'それ' in 原文 else 原文.find('その結果'),原文.find(word)),)).固定複製()
        # 確認への返答・訂正は、未解決目的又は直前目的の同じスロットへだけ帰す。
        fix=re.fullmatch(r'(訂正[：:]?)?(?:比較する)?(単位|属性|左の年|右の年|年)は(.+?)(?:です|にして)?',t)
        if fix:
            correction,key,value=fix.groups()
            return 会話要求(原文,'訂正' if correction else '確認返答',補助={'欄':key,'値':_名前(value)},対応=(('スロット指定',0,len(原文)),)).固定複製()
        # 同一表の二時点を、二つの独立した対象役割として保持する。
        pair=re.match(r'(?:(?:CSV|JSON)?資料「([^「」]+)」|この資料)の([0-9]{4})年と([0-9]{4})年の',t)
        targets=(); rest=t; time_diff=False; material_refs=[]
        if pair:
            name,y1,y2=pair.groups()
            if name is None:
                if len(資料名)!=1: raise ValueError('この資料の参照先が一意でない')
                name=資料名[0]
                pos=原文.find('この資料');material_refs.append((pos,pos+2))
            targets=(比較対象(name,(('年',y1),)),比較対象(name,(('年',y2),)))
            rest=t[pair.end():];time_diff=True
        else:
            pair=re.match(r'(?:CSV|JSON)?資料「([^「」]+)」と(?:CSV|JSON)?資料「([^「」]+)」の',t)
            if pair:
                targets=tuple(比較対象(n) for n in pair.groups());rest=t[pair.end():]
            elif t.startswith(('この2つの','この二つの')):
                if len(資料名)!=2: raise ValueError('この2つの参照先が一意でない')
                targets=tuple(比較対象(n) for n in 資料名);rest=t[5:]
                pos=原文.find('この');material_refs.append((pos,pos+2))
        if targets:
            split=_末尾(rest,比較述語)
            if split is None: raise ValueError('比較対象に対する目的が未対応')
            head,verb=split
            if head.endswith(('を','は','が')): head=head[:-1].strip()
            unit=''
            m=re.fullmatch(r'(.+?)を([^「」\s]+)で',head)
            if m: attribute,unit=m.groups()
            else: attribute=head
            if verb=='差はいくら' and attribute.endswith('の'): attribute=attribute[:-1]
            attribute=_名前(attribute)
            return 会話要求(原文,'比較',targets,attribute,unit,detailed,time_diff,forbid,
                {'問い':verb,'資料参照解消':material_refs},(('比較要求',0,len(原文)),)).固定複製()
        split=_末尾(t,取得述語)
        if split:
            head,verb=split
            m=re.fullmatch(r'(.+?)の(.+?)を([^「」\s]+)で',head)
            if m:
                主体,attribute,unit=m.groups()
                return 会話要求(原文,'取得',属性=_名前(attribute),単位=_名前(unit),詳細=detailed,
                    外部禁止=forbid,補助={'主題':_名前(主体)},対応=(('取得要求',0,len(原文)),)).固定複製()
    # 第17バッチが理解する対象・目的の文法へだけ明示的に戻す。
    # 新しい言い換えは数学の結果質問と語順に限定。生成した文と原文を両方保持する。
    if len(task)==1 and re.fullmatch(r'[0-9+*/(). \-]+(?:は)?',task[0]):
        expression=task[0].removesuffix('は').strip()
        task=['「'+expression+'」を計算して']
    canonical=[]
    for clause in task:
        c=clause
        for old,new in 数式質問:
            if c.endswith(old): c=c[:-len(old)]+new;break
        question=re.fullmatch(r'(.+?)の([A-Za-z_][A-Za-z_0-9]*)による(微分|積分)は',c)
        if question:
            value,var,act=question.groups();c=f'{value}を{var}で{act}して'
        expr=re.fullmatch(r'([^「」]+)を([A-Za-z_][A-Za-z_0-9]*)で(微分|積分)して',c)
        if expr and not expr.group(1).startswith(('それ','その結果','資料','本文','元の資料')):
            value,var,act=expr.groups();c=f'「{value.strip()}」を{var}で{act}して'
        calc=re.fullmatch(r'([0-9+*/(). \-]+)を計算して',c)
        if calc: c='「'+calc.group(1).strip()+'」を計算して'
        canonical.append(c)
    return 会話要求(原文,'既存目的',詳細=detailed,外部禁止=forbid,
        補助={'射影文':'、'.join(canonical)},対応=(('既存目的への表層射影',0,len(原文)),)).固定複製()


def HDS会話を照合(ir, request: 会話要求, *, 文脈解消=False):
    """新規比較・確認等は、未処理のHDS条件を免除しない。原文も保存する。"""
    from .HDS中間表現 import HDSIR,値状態
    if type(ir) is not HDSIR or ir.原文!=request.原文:
        raise ValueError('実HDS原文の不一致')
    coords=ir.座標辞書()
    if len(coords)!=len(ir.座標): raise ValueError('HDS座標重複')
    sources=[x for x in ir.座標 if x.種別=='情報源_text']
    if len(sources)!=1 or sources[0].内容!=request.原文: raise ValueError('HDS原文座標不一致')
    allowed={'情報源_text','言語.normalized','文脈.言語','制御.選択意図','値.数量','属性.単位','対象.主題語','目的.検索焦点'}
    for x in ir.座標:
        if x.種別 not in allowed or x.値状態!=値状態.確定:
            raise ValueError('未処理HDS座標:'+x.種別)
        if x.種別=='制御.選択意図' and x.内容!='通常': raise ValueError('未解消の否定・選択意図')
    for r in ir.関係:
        if r.種別!='数量単位' or r.条件 or r.値状態!=値状態.確定:
            raise ValueError('未処理HDS関係:'+r.種別)
    reasons=set(); solved=[]
    for r in ir.残差:
        if r.種別!='未解共参照' or r.影響座標:
            raise ValueError('未解消HDS残差:'+r.種別)
        occurrences=tuple(re.finditer(re.escape(r.原文),request.原文)) if r.原文 else ()
        spans=request.補助.get('資料参照解消',())
        material_bound=(request.行為 in ('比較','集合') and occurrences and spans and
            all(any(a<=m.start() and m.end()<=b for a,b in spans) for m in occurrences))
        # 実際に二資料/一資料へ束縛した範囲だけを解消する。未知残差は免除しない。
        if not material_bound and not (文脈解消 and request.行為 in ('再表現','確認返答','訂正')):
            raise ValueError('未解消の対象参照')
        solved.append(r.残差ID);reasons.add(r.理由)
    if any(loss not in reasons for a in ir.意味作用履歴 for loss in a.損失):
        raise ValueError('未解消HDS意味損失')
    return tuple(solved)
