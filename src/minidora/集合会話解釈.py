"""対象群・属性・操作・選別・表示を別の句として構成する局所会話解釈。

語彙は成果の意味に対応する。能力名や工程列をここから返さない。
任意日本語の解析器ではなく、全句を解釈できない依頼は実行へ渡さない。
"""
from __future__ import annotations
import re
from .会話意味 import 会話要求, 比較対象
from .会話語彙 import 詳細表現, 簡潔表現, 外部禁止表現
from .会話句 import 句を分割 as _節

_作用語 = {'一覧':'一覧', '比較':'比較', '合計':'合計', '平均':'平均',
           '最大':'最大', '最大値':'最大', '最小':'最小', '最小値':'最小', '件数':'件数'}
_対象語 = re.compile(r'(?:(?:CSV|JSON)?資料)「([^「」\r\n]{1,128})」')
_数 = r'[+-]?[0-9]+(?:\.[0-9]+)?'
_比較語 = ('以上','以下','未満','超','と一致','と不一致')


def _名前(value):
    text=value.strip()
    if text.startswith('「') and text.endswith('」'): text=text[1:-1]
    if not text or len(text)>80 or any(c in text for c in '「」\n\r\x00'):
        raise ValueError('集合要求の属性名が不正')
    return text


def _対象を分離(text, names, 種別='資料'):
    """対象句だけを読み、残りと局所解消した照応範囲を返す。"""
    target_word=_対象語 if 種別=='資料' else re.compile(r'主題「([^「」\r\n]{1,128})」')
    all_=re.match(r'(?:すべての資料|全資料|この([0-9]+|二|三|四|五|六|七|八)つ)の',text) if 種別=='資料' else None
    if all_:
        n=all_.group(1)
        count=int(n) if n and n.isdecimal() else {'二':2,'三':3,'四':4,'五':5,'六':6,'七':7,'八':8}.get(n)
        if not 1<=len(names)<=8 or count is not None and count!=len(names):
            raise ValueError('対象群の照応先が一意でない又は8件上限')
        return tuple(比較対象(x) for x in names),text[all_.end():],False,(0,2) if text.startswith('この') else None
    first=target_word.match(text)
    if not first: return None
    name=first.group(1); position=first.end()
    years=re.match(r'の([0-9]{4}年(?:と[0-9]{4}年){1,7})の',text[position:])
    if years and 種別=='資料':
        points=re.findall(r'[0-9]{4}',years.group(1))
        if len(set(points))!=len(points): raise ValueError('同じ時点を重複して数えない')
        return tuple(比較対象(name,(('年',p),)) for p in points),text[position+years.end():],True,None
    targets=[比較対象(name)]
    while True:
        connector=re.match(r'\s*と\s*',text[position:])
        if not connector: break
        nxt=target_word.match(text,position+connector.end())
        if not nxt: return None
        targets.append(比較対象(nxt.group(1)));position=nxt.end()
        if len(targets)>8: raise ValueError('対象群は8件以内')
    if not text[position:].startswith('の'): return None
    return tuple(targets),text[position+1:],False,None


def _目的を分離(text):
    # 動作語尾と表示単位を、要求属性の名称から分離する。
    unit=''
    unit_match=re.search(r'を([^「」\s]+)で(?=(?:比較|比べ|一覧|集計|教え|求め|計算))',text)
    if unit_match:
        unit=unit_match.group(1);text=text[:unit_match.start()]+'を'+text[unit_match.end():]
    endings=('比較してください','比較して','比べてください','比べて','一覧にしてください','一覧にして','集計して')
    for ending in endings:
        if text.endswith(ending):
            head=text[:-len(ending)].strip()
            if not head.endswith('を'): return None
            head=head[:-1]
            if ending.startswith('集計'): operations=('合計','平均')
            elif ending.startswith('一覧'): operations=('一覧',)
            else: operations=('比較',)
            return _名前(head),operations,unit
    m=re.fullmatch(r'(.+?)の(一覧|比較|合計|平均|最大値|最大|最小値|最小|件数)((?:と(?:一覧|比較|合計|平均|最大値|最大|最小値|最小|件数))*)(?:は|を(?:教えて(?:ください)?|求めて(?:ください)?|計算して|出して))',text)
    if not m: return None
    attribute,first,rest=m.groups()
    ops=tuple(_作用語[x] for x in (first,*rest.removeprefix('と').split('と'))) if rest else (_作用語[first],)
    if len(ops)>7 or len(set(ops))!=len(ops): raise ValueError('集合操作の重複又は上限')
    return _名前(attribute),ops,unit


def 集合会話を解釈(original: str, names: tuple[str,...]) -> 会話要求 | None:
    """既存の単純二資料比較は旧入口へ残し、それ以外の集合要求を構成する。"""
    if type(original) is not str or not 0<len(original)<=8192: raise ValueError('集合原文の上限')
    pieces=[]
    for a,b in _節(original):
        raw=original[a:b]; leading=len(raw)-len(raw.lstrip())
        text=raw.strip().rstrip('？?！!').strip()
        if text: pieces.append((a+leading,a+leading+len(text),text))
    模型核=None; options={}; excluded=[]; spans=[]; 未知=[]; conditional=False; modified=False
    def put(key,value):
        if key in options and options[key]!=value: raise ValueError('要求条件が競合:'+key)
        options[key]=value
    for a,b,text in pieces:
        if text=='ただし': conditional=True;continue
        constrained=conditional or text.startswith('ただし')
        conditional=False
        if text.startswith('ただし'): text=text[3:].strip()
        field=None
        if text in 外部禁止表現: put('外部禁止',True);field='外部境界'
        elif text in 詳細表現: put('詳細',True);field='詳細'
        elif text in 簡潔表現: put('詳細',False);field='詳細'
        elif text in ('表で','表で説明して','表にして','表形式で'): put('形式','表');field='形式'
        elif text in ('文章で','文章で説明して'): put('形式','文章');field='形式'
        elif text in ('手順も説明して','計算過程も説明して'): put('手順',True);field='手順'
        elif text.endswith('で'):
            from .証拠統合 import _単位
            if text[:-1] in _単位: put('単位',text[:-1]);field='単位'
            # 単位以外の「〜で」は下の条件解析へ渡す。
        if field is None:
            match=re.fullmatch(r'単位は(.+?)(?:です)?',text)
            year=re.fullmatch(r'([0-9]{4})年(?:だけ|で)',text)
            condition=re.fullmatch(r'条件「([^「」]+)」で',text)
            omit=re.fullmatch(r'資料「([^「」]+)」(?:を除いて|は除外して)',text)
            limit=re.fullmatch(rf'(?:値|数値|「([^「」]+)」)が({_数})([^「」\s]+?)(以上|以下|未満|超|と一致|と不一致)(?:だけ)?',text)
            if match: put('単位',_名前(match.group(1)));field='単位'
            elif year: put('年',year.group(1));field='年'
            elif condition: put('条件',condition.group(1));field='条件'
            elif omit:
                if omit.group(1) in excluded: raise ValueError('除外資料の重複')
                excluded.append(omit.group(1));field='対象除外'
            elif limit:
                attr,num,unit,op=limit.groups()
                put('選別',{'属性':attr or '', '値':num,'単位':unit,'比較':op});field='数量条件'
        if field:
            spans.append((field,a,b));modified=True;continue
        # 句頭にある表示単位、年、条件も同じ役割へ組み込む。
        作業=text
        prefix=re.match(r'([^「」\s、]+)で\s*(?=(?:(?:CSV|JSON)?資料「|この|全資料|すべての資料))',作業)
        if prefix: put('単位',prefix.group(1));作業=作業[prefix.end():];modified=True
        情報源_kind='主題' if 作業.startswith('主題「') else '資料'
        targets=_対象を分離(作業,names,情報源_kind)
        if targets is None or constrained:
            未知.append((a,b,text));continue
        selected,rest,time_diff,refspan=targets
        y=re.match(r'([0-9]{4})年の',rest)
        if y: put('年',y.group(1));rest=rest[y.end():];modified=True
        c=re.match(r'条件「([^「」]+)」の',rest)
        if c: put('条件',c.group(1));rest=rest[c.end():];modified=True
        if 情報源_kind=='主題':
            if rest.endswith('調べて比較して'): rest=rest[:-len('調べて比較して')]+'比較して'
            elif rest.endswith('調べて'): rest=rest[:-len('調べて')]+'教えて'
            else:
                未知.append((a,b,text));continue
        goal=_目的を分離(rest)
        if goal is None:
            未知.append((a,b,text));continue
        if 模型核 is not None: raise ValueError('独立した集合目的の複数句は未対応')
        attribute,operations,unit=goal
        if unit: put('単位',unit)
        ref=None
        if refspan is not None:
            p=original.find('この',a,b);ref=(p,p+2)
        模型核=(selected,time_diff,attribute,operations,ref,情報源_kind)
        spans.append(('集合目的',a,b))
    if 模型核 is None: return None
    selected,time_diff,attribute,operations,ref,情報源_kind=模型核
    # 従来二資料・二時点比較の互換性を維持する。
    if 情報源_kind=='資料' and len(selected)==2 and operations==('比較',) and not modified and not 未知:
        return None
    if 情報源_kind=='主題' and (excluded or '年' in options or '条件' in options):
        raise ValueError('取得主題の除外・年・条件指定は未対応')
    if conditional or 未知: raise ValueError('未解釈の条件又は集合要求の尾部:'+str(未知))
    all_names={x.資料 for x in selected}
    if any(x not in all_names for x in excluded): raise ValueError('除外対象が要求資料に存在しない')
    selected=tuple(x for x in selected if x.資料 not in excluded)
    if not selected or len(set(selected))!=len(selected): raise ValueError('対象群の空値又は重複')
    if '比較' in operations and len(selected)<2: raise ValueError('比較には二つ以上の対象が必要')
    if time_diff and '年' in options: raise ValueError('複数時点の指定と共通年の条件が競合')
    for key in ('年','条件'):
        if key in options:
            selected=tuple(比較対象(t.資料,tuple({**dict(t.行条件),key:options[key]}.items())) for t in selected)
    criterion=options.get('選別',{})
    if criterion.get('属性') and criterion['属性']!=attribute: raise ValueError('選別する属性と集計属性が異なる')
    criterion={k:v for k,v in criterion.items() if k!='属性'}
    aux={'供給':'取得' if 情報源_kind=='主題' else '資料','操作':operations,'形式':options.get('形式','文章'),'手順':options.get('手順',False),
         '選別':criterion,'除外資料':tuple(excluded),'資料参照解消':(ref,) if ref else ()}
    return 会話要求(original,'集合',selected,attribute,options.get('単位',''),
                    options.get('詳細',False),time_diff,options.get('外部禁止',False),aux,tuple(spans)).固定複製()
