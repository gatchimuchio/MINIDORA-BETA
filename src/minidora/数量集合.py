"""原文に再接続できる数量群の選別・比較・集約。

事業の優劣や原因を推定しない。合計・平均は依頼された記載値の算術演算である。
"""
from __future__ import annotations
from copy import deepcopy
from fractions import Fraction
import re
from html import escape
from .能力合成 import _結果辞書, _参照結合
from .応答構成 import 能力結果を復元
from .会話意味 import 意味指紋
from .会話数量 import 数量記録整合, 会話処理不成立
from .証拠統合 import _単位, _有理数, _数表記
from .製品版.型 import 能力結果

数量集合版='MINIDORA-数量集合-v0.1'
_操作=frozenset({'一覧','比較','合計','平均','最大','最小','件数'})
_演算={'以上':lambda a,b:a>=b, '以下':lambda a,b:a<=b, '未満':lambda a,b:a<b,
       '超':lambda a,b:a>b, 'と一致':lambda a,b:a==b, 'と不一致':lambda a,b:a!=b}


def _拒否(code,reason):
    raise 会話処理不成立(code,reason)


def 有理数を表記(value: Fraction) -> str:
    """循環小数は丸めず分数として返す。既存の有限小数規則は変更しない。"""
    den=value.denominator
    for p in (2,5):
        while den%p==0: den//=p
    return _数表記(value) if den==1 else f'{value.numerator}/{value.denominator}'


def _設定を確認(settings):
    if type(settings) is not dict or set(settings)!={'操作','時点差','選別','除外資料'}:
        _拒否('入力不正','集合操作の設定項目が不一致')
    ops=settings['操作']
    if (type(ops) not in (list,tuple) or not 1<=len(ops)<=7
            or any(type(x) is not str or x not in _操作 for x in ops) or len(set(ops))!=len(ops)):
        _拒否('入力不正','集合操作が未知・重複・上限超過')
    if type(settings['時点差']) is not bool or type(settings['選別']) is not dict:
        _拒否('入力不正','時点差又は選別の型不正')
    omit=settings['除外資料']
    if (type(omit) not in (list,tuple) or len(omit)>8
            or any(type(x) is not str or not x or len(x)>128 for x in omit) or len(set(omit))!=len(omit)):
        _拒否('入力不正','除外資料の型・件数・重複')
    criterion=settings['選別']
    if criterion:
        if (set(criterion)!={'値','単位','比較'} or criterion['比較'] not in _演算
                or criterion['単位'] not in _単位 or type(criterion['値']) is not str
                or len(criterion['値'])>128 or not re.fullmatch(r'[+-]?[0-9]+(?:\.[0-9]+)?',criterion['値'])):
            _拒否('入力不正','数量選別条件が未対応')


def 数量群を処理(values: tuple[能力結果,...], settings: dict) -> 能力結果:
    _設定を確認(settings)
    if type(values) is not tuple or not 1<=len(values)<=8:
        _拒否('入力不正','数量集合は1〜8対象')
    if any(not 数量記録整合(v) for v in values):
        _拒否('検証失敗','数量集合の原資料再構成が不一致')
    records=[v.データ for v in values]; first=records[0]
    identities=[(q['資料'],q['位置']) for q in records]
    if len(set(identities))!=len(identities):
        _拒否('意味未確定','同じ資料の同じ数量を複数対象として数えない')
    if any(q['資料'] in settings['除外資料'] for q in records):
        _拒否('入力不正','除外指定した資料が数量集合へ混入')
    for key in ('属性','単位','条件'):
        if any(q[key]!=first[key] for q in records):
            _拒否('前提矛盾','異なる'+key+'を同じ集合として処理しない')
    if len({q['時点'] for q in records})>1 and not settings['時点差']:
        _拒否('意味未確定','異時点の集合には時点差を扱う明示目的が必要')
    unit=first['表示単位'] if all(q['表示単位']==first['表示単位'] for q in records) else first['単位']
    scale=Fraction(_単位[unit][1]); numbers=[Fraction(q['値']) for q in records]
    criterion=settings['選別']
    selected=list(range(len(values)))
    if criterion:
        base,factor=_単位[criterion['単位']]
        if base!=first['単位']: _拒否('前提矛盾','選別の閾値と対象の単位次元が異なる')
        threshold=_有理数(criterion['値'])*Fraction(factor)
        selected=[i for i,x in enumerate(numbers) if _演算[criterion['比較']](x,threshold)]
    if not selected and set(settings['操作'])-{'一覧','件数'}:
        _拒否('情報不足','条件に合う数量がないため集約値を捏造しない')
    if '比較' in settings['操作'] and len(selected)<2:
        _拒否('情報不足','比較に必要な二つ以上の数量が選別後に残らない')
    chosen=[numbers[i] for i in selected]
    ordered=sorted(selected,key=lambda i:-numbers[i])  # 同値は元の役割順。順位は後で同順位にする。
    aggregates={'件数':len(selected)}
    if chosen:
        aggregates.update({'合計':有理数を表記(sum(chosen,Fraction(0))),
                           '平均':有理数を表記(sum(chosen,Fraction(0))/len(chosen)),
                           '最大':有理数を表記(max(chosen)), '最小':有理数を表記(min(chosen)),
                           '範囲':有理数を表記(max(chosen)-min(chosen))})
    ranks=[]; prior=None; rank=0
    for position,index in enumerate(ordered,1):
        if prior is None or numbers[index]!=prior: rank=position
        ranks.append({'対象':index,'順位':rank});prior=numbers[index]
    claims=[]
    for i,q in enumerate(records):
        claims.append({'ID':f'数量:{i}','種別':'記載値','対象':q['資料'],'時点':q['時点'],
                       '属性':q['属性'],'値':q['値'],'単位':q['単位'],
                       '根拠':{'元結果':i,'位置':q['位置'],'開始':q['開始'],'終了':q['終了']},'依存':[]})
    dependencies=[f'数量:{i}' for i in selected]
    for op in settings['操作']:
        claims.append({'ID':'演算:'+op,'種別':op,'依存':dependencies,
                       '結果':aggregates.get(op,ranks if op=='比較' else selected)})
    display={k:(有理数を表記(Fraction(v)/scale) if k!='件数' else v) for k,v in aggregates.items()}
    資料={'版':数量集合版,'種別':'数量集合','設定':deepcopy(settings),
          '元結果':[_結果辞書(v) for v in values], '採用対象':selected,'順位':ranks,
          '集約':aggregates,'表示集約':display,'表示単位':unit,'生成関係':claims,
          '本文':f'{len(values)}対象から{len(selected)}対象の記載値を処理しました。'}
    資料['記録SHA256']=意味指紋(資料)
    return 能力結果(True,資料['本文'],参照=_参照結合(r for v in values for r in v.参照),データ=資料)


def 集合記録整合(value) -> bool:
    try:
        _結果辞書(value)
        d=value.データ
        rebuilt=数量群を処理(tuple(能力結果を復元(v) for v in d['元結果']),d['設定'])
        available={r.識別子:r for r in _参照結合(value.参照)}
        return (value.成立 and rebuilt.本文==value.本文 and rebuilt.データ==d
                and rebuilt.根拠==value.根拠 and rebuilt.保留理由==value.保留理由
                and all(available.get(r.識別子)==r for r in rebuilt.参照))
    except (KeyError,ValueError,TypeError,AttributeError,RecursionError,OverflowError):
        return False


def _資料名(q):
    結果='資料「'+q['資料']+'」'
    if q['時点'] is not None: 結果+='（'+q['時点']+'）'
    return 結果


def _セル(text):
    return escape(str(text),quote=False).replace('|','\\|').replace('`','\\`').replace('\n',' ').replace('\r',' ')


def 集合の表現(value: 能力結果, *, 形式='文章', 手順=False):
    """意味が再構成できた数量関係だけを、文・表・算術手順へ構成する。"""
    if 形式 not in ('文章','表') or type(手順) is not bool: raise ValueError('集合の表現設定不正')
    if not 集合記録整合(value): raise ValueError('数量集合の原成果再構成不一致')
    d=value.データ; records=[v['データ'] for v in d['元結果']]; selected=d['採用対象']
    unit=d['表示単位'];scale=Fraction(_単位[unit][1]);first=records[0]
    numbers=[有理数を表記(Fraction(q['値'])/scale) for q in records]
    propositions=[];origins=[];caveats=[];conditions=[]
    if first['条件'] is not None: conditions.append('条件：'+first['条件'])
    else: caveats.append('条件未記載同士の記載値を処理しており、現実の条件一致は認定していません。')
    if d['設定']['時点差']: conditions.append('指定された複数時点の記載値を扱っています。')
    omit=d['設定']['除外資料']
    if omit: conditions.append('要求で除外：'+'、'.join('資料「'+x+'」' for x in omit))
    criterion=d['設定']['選別']
    if criterion: conditions.append('選別条件：'+criterion['値']+criterion['単位']+criterion['比較'])
    labels=[_資料名(q) for q in records]
    if 形式=='表':
        rows=['| 資料・時点 | '+_セル(first['属性'])+'（'+_セル(unit)+'） | 選別結果 |',
              '|---|---:|---|']
        rows += ['| '+_セル(label)+' | '+number+' | '+('対象' if i in selected else '条件外')+' |'
                 for i,(label,number) in enumerate(zip(labels,numbers))]
        listing='\n'.join(rows)
    else:
        listing=first['属性']+'の記載値：\n'+'\n'.join(label+'：'+number+unit+('（選別条件外）' if i not in selected else '')
                   for i,(label,number) in enumerate(zip(labels,numbers)))
    propositions.append(('記載一覧',listing,tuple(f'数量:{i}' for i in range(len(records))),tuple(conditions)))
    for op in d['設定']['操作']:
        if op=='一覧': continue
        if op=='比較':
            text='大きい順（同値は同順位）：'+'、'.join(str(r['順位'])+'位 '+labels[r['対象']] for r in d['順位'])
            text+='。最大－最小の差は'+d['表示集約']['範囲']+unit+'です。'
        elif op=='件数': text='選別後の対象数は'+str(len(selected))+'件です。'
        else:
            text='記載値の'+op+'は'+str(d['表示集約'][op])+unit+'です。'
            if op in ('最大','最小'):
                indices=[i for i in selected if Fraction(records[i]['値'])==Fraction(d['集約'][op])]
                text+='該当：'+'、'.join(labels[i] for i in indices)+'。'
        propositions.append((op,text,tuple(f'数量:{i}' for i in selected),()))
    if any(op in d['設定']['操作'] for op in ('合計','平均')):
        caveats.append('合計・平均は指定記載値の算術演算です。対象の重複、事業上の加算可能性、母集団の代表性は認定していません。')
    caveats.append('資料の記載値の処理であり、原因・望ましさ・資料の真偽は判定していません。')
    if any(q['単位由来']=='依頼の宣言' for q in records):
        caveats.append('資料に単位の記載がない値には、依頼で指定された単位を適用しています。')
    if 手順:
        text='処理手順：資料の属性・行条件から数量を読み、同じ次元の単位へ換算し、指定条件で選別しました。'
        if selected and any(op in d['設定']['操作'] for op in ('合計','平均')):
            expr=' + '.join('('+numbers[i]+')' for i in selected)
            text+='\n合計 = '+expr+' = '+str(d['表示集約']['合計'])+unit+'。'
            if '平均' in d['設定']['操作']: text+='\n平均 = 合計 / '+str(len(selected))+' = '+str(d['表示集約']['平均'])+unit+'。分数表記は丸め前の厳密値です。'
        propositions.append(('手順',text,tuple(f'数量:{i}' for i in selected),()))
    for q in records:
        origins.append(_資料名(q)+'：'+q['位置']+'、原文「'+q['原文片']+'」')
    return tuple(propositions),tuple(caveats),tuple(origins)
