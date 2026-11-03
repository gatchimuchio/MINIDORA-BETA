"""原文対応付き表・数値証拠を、対象役割を保った数量比較へ接続する。

既存の文書読取・数値証拠・関係制約を使用する。因果や出典の真偽は判定しない。
"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import asdict
from fractions import Fraction
import re
from .能力合成 import _結果辞書, _参照結合
from .応答構成 import 能力結果を復元
from .構造化文書操作 import 文書記録整合
from .証拠統合 import 証拠統合器, 証拠照合要求, 記載値を採用, _単位, _有理数, _数表記
from .関係制約 import 関係制約器, 関係問題, 関係式
from .製品版.型 import 能力結果
from .会話意味 import 意味指紋

会話数量版='MINIDORA-会話数量-v0.1'


class 会話処理不成立(ValueError):
    def __init__(self, 種別, 理由):
        self.種別, self.理由=種別,理由
        super().__init__('会話失敗:'+種別+':'+理由)


def _失敗(kind, why):
    raise 会話処理不成立(kind,why)


def _文字(value):
    if type(value) is not str or not value or len(value)>128 or any(ord(c)<32 for c in value):
        _失敗('入力不正','短い識別文字列が必要')
    return value


def _選択設定(settings):
    if type(settings) is not dict or set(settings)!={'資料','属性','単位','行条件','方式'}:
        _失敗('入力不正','数量選択の設定項目不一致')
    for key in ('資料','属性'): _文字(settings[key])
    if type(settings['単位']) is not str or settings['単位'] and settings['単位'] not in _単位:
        _失敗('入力不足','対応する単位を指定する')
    filters=settings['行条件']
    if type(filters) is not dict or len(filters)>4:
        _失敗('入力不正','行条件不正')
    for k,v in filters.items(): _文字(k);_文字(v)
    if settings['方式'] not in ('直下','入れ子'):
        _失敗('入力不正','数量選択方式不正')


def _素値(node):
    return node['値'] if node['型'] in ('数値','文字列','真偽','空値') else None


def _表候補(doc, attribute, nested):
    structure=doc['構造']; rows=[]
    if doc['形式']=='CSV':
        names=doc['列名']
        if not names: _失敗('入力不正','CSVの見出しが必要')
        for i,row in enumerate(structure['値']):
            cells={name: (node,f'/{i}/{j}') for j,(name,node) in enumerate(zip(names,row['値']))}
            rows.append((cells,{},f'/{i}'))
        return rows
    def walk(node,path,ancestors,depth):
        if depth>12: _失敗('入力不正','表の入れ子上限')
        if node['型']=='対象':
            cells={k:(v,path+'/'+k.replace('~','~0').replace('/','~1')) for k,v in node['値'].items()}
            if attribute in cells:
                rows.append((cells,ancestors,path))
            if nested:
                inherited=dict(ancestors)
                for k,(v,_) in cells.items():
                    if k in ('単位','条件','時点','年','備考','注記','留保') and _素値(v) not in (None,''):
                        if k in inherited and inherited[k]!=_素値(v):
                            _失敗('前提矛盾','入れ子の同伴条件が競合:'+k)
                        inherited[k]=_素値(v)
                for k,(child,cp) in cells.items():
                    if child['型'] in ('対象','配列'): walk(child,cp,inherited,depth+1)
        elif node['型']=='配列':
            for i,child in enumerate(node['値']):
                if child['型']=='対象': walk(child,path+'/'+str(i),ancestors,depth+1)
                elif nested and child['型']=='配列': walk(child,path+'/'+str(i),ancestors,depth+1)
    walk(structure,'',{},0)
    return rows


def 表数量を読む(value: 能力結果, settings: dict) -> 能力結果:
    _選択設定(settings)
    if not 文書記録整合(value): _失敗('入力不正','文書の原本再構成不一致')
    doc=value.データ['文書']; attribute=settings['属性']
    if value.データ['操作列']: _失敗('入力不正','数量選択は未選択の原文書から行う')
    candidates=_表候補(doc,attribute,settings['方式']=='入れ子')
    # 別の階層に同じ属性の候補がある場合も、短い経路だけを理由に採らない。
    if settings['方式']=='直下' and doc['形式']=='JSON':
        all_candidates=_表候補(doc,attribute,True)
        full_matches=[]
        for cells,ancestor,path in all_candidates:
            metadata={**ancestor,**{k:_素値(v) for k,(v,_) in cells.items() if _素値(v) is not None}}
            if all(k in metadata and str(metadata[k])==v for k,v in settings['行条件'].items()):
                full_matches.append(path)
        if len(full_matches)>1:
            _失敗('意味未確定','複数階層に対象属性がある。明示した位置又は行条件が必要')
    matched=[]; has_attribute=False
    for cells,inherited,path in candidates:
        if attribute not in cells: continue
        has_attribute=True
        for key in ('単位','条件','時点','年','備考','注記','留保'):
            if key in cells and cells[key][0]['型'] not in ('数値','文字列','空値'):
                _失敗('入力不正','未対応の同伴条件構造:'+key)
        metadata={**inherited,**{k:_素値(v) for k,(v,_) in cells.items() if _素値(v) is not None}}
        if not all(k in metadata and str(metadata[k])==v for k,v in settings['行条件'].items()): continue
        matched.append((cells,metadata,path))
    if not matched:
        if not has_attribute and settings['方式']=='直下' and doc['形式']=='JSON':
            _失敗('構造未到達','直下に対象属性がない。入れ子経路の再探索が必要')
        _失敗('情報不足','指定された属性・行条件に合う値がない')
    if len(matched)!=1: _失敗('意味未確定','対象行が一意でない。行条件を指定する')
    cells,metadata,path=matched[0]; node,pointer=cells[attribute]
    for name in ('備考','注記','留保'):
        if metadata.get(name) not in (None,''):
            _失敗('未解釈注記','値に同伴する'+name+'を未解釈のまま採用しない')
    if node['型']=='真偽' or node['型']=='空値': _失敗('入力不正','真偽・空値を数値化しない')
    if doc['形式']=='JSON' and node['型']!='数値':
        _失敗('入力不正','JSON文字列を数値型へ読み替えない')
    字句=_素値(node)
    if type(字句) is not str or len(字句)>128 or not re.fullmatch(r'[+-]?[0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]{1,3})?',字句):
        _失敗('入力不正','明示的な有限数値表記が必要')
    unit=metadata.get('単位') or settings['単位']
    if unit not in _単位: _失敗('入力不足','単位が未確定又は未対応')
    base,mul=_単位[unit]
    if settings['単位'] and _単位[settings['単位']][0]!=base:
        _失敗('前提矛盾','指定単位と資料単位の次元が異なる')
    condition=metadata.get('条件'); moment=metadata.get('時点',metadata.get('年'))
    if condition is not None: _文字(str(condition))
    if moment is not None: _文字(str(moment))
    span=doc['対応'][pointer]
    情報源=value.データ['原本']['原文']
    quantity=_有理数(字句)*Fraction(mul)
    display_unit=settings['単位'] or base
    display_value=_数表記(quantity/Fraction(_単位[display_unit][1]))
    body=display_value+' '+display_unit
    資料={'版':会話数量版,'種別':'数量','資料':settings['資料'],'属性':attribute,
          '値':_数表記(quantity),'単位':base,'入力単位':unit,'表示単位':display_unit,'表示値':display_value,
          '単位由来':'資料' if metadata.get('単位') else '依頼の宣言',
          '条件':None if condition is None else str(condition),'時点':None if moment is None else str(moment),
          '位置':pointer,'開始':span['開始'],'終了':span['終了'],
          '原文片':情報源[span['開始']:span['終了']], '同伴':metadata,
          '選択':deepcopy(settings),'元結果':_結果辞書(value),'方式':'表', '本文':body}
    資料['記録SHA256']=意味指紋(資料)
    return 能力結果(True,body,参照=value.参照,データ=資料)


def 記載数量を読む(value: 能力結果, settings: dict) -> 能力結果:
    if type(settings) is not dict or set(settings)!={'対象','属性','単位'}:
        _失敗('入力不正','数値記載照合の設定項目不一致')
    for k in settings: _文字(settings[k])
    report=証拠統合器().実行(証拠照合要求(**settings),value.参照)
    if not report.成立: _失敗('入力不正',report.保留理由)
    adopted=記載値を採用(report)
    if not adopted.成立:
        kind='前提矛盾' if '記載競合' in report.データ.get('理由',()) else '情報不足'
        _失敗(kind,'資料中の数値記載を採用できない:'+','.join(report.データ.get('理由',())))
    claims=report.データ['主張']; groups=report.データ['群']
    if len(groups)!=1 or not claims: _失敗('意味未確定','数値の適用群が一意でない')
    group=groups[0]; selected=[x for x in claims if x['主張ID'] in group['主張ID']]
    quantity=Fraction(selected[0]['値'])
    if any(Fraction(x['値'])!=quantity or x['比較']!='一致' for x in selected):
        _失敗('意味未確定','一点の一致記載だけを数量として扱う')
    unit=selected[0]['単位'];display_unit=settings['単位']
    display_value=_数表記(quantity/Fraction(_単位[display_unit][1]));body=display_value+' '+display_unit
    資料={'版':会話数量版,'種別':'数量','資料':settings['対象'],'属性':settings['属性'],
          '値':_数表記(quantity),'単位':unit,'表示単位':display_unit,'表示値':display_value,'条件':group['条件'],'時点':group['時点'],
          '入力単位':settings['単位'],'単位由来':'資料','同伴':{},'方式':'記載',
          '選択':deepcopy(settings),'元結果':_結果辞書(value),'証拠':_結果辞書(report),
          '原文片':selected[0]['原文'],'開始':selected[0]['開始'],'終了':selected[0]['終了'],
          '位置':selected[0]['参照ID'],'本文':body}
    資料['記録SHA256']=意味指紋(資料)
    return 能力結果(True,body,参照=report.参照,データ=資料)


def 数量記録整合(value):
    try:
        raw=deepcopy(value.データ); hash_=raw.pop('記録SHA256')
        if hash_!=意味指紋(raw) or raw['本文']!=value.本文 or not value.成立: return False
        情報源=能力結果を復元(raw['元結果'])
        rebuilt=(表数量を読む if raw['方式']=='表' else 記載数量を読む)(情報源,raw['選択'])
        # 上流参照は合成器が追加できるが、消去・書換えは許さない。
        available={r.識別子:r for r in value.参照}
        return rebuilt.データ==value.データ and all(available.get(r.識別子)==r for r in rebuilt.参照)
    except (KeyError,ValueError,TypeError,AttributeError,RecursionError,OverflowError):
        return False


def 数量を比較(left: 能力結果, right: 能力結果, settings: dict) -> 能力結果:
    if type(settings) is not dict or set(settings)!={'時点差'} or type(settings['時点差']) is not bool:
        _失敗('入力不正','比較する時点の契約が必要')
    if not 数量記録整合(left) or not 数量記録整合(right):
        _失敗('入力不正','数量の原文再構成不一致')
    a,b=left.データ,right.データ
    for k in ('属性','単位','条件'):
        if a[k]!=b[k]: _失敗('前提矛盾','異なる'+k+'を無断で比較しない')
    if a['時点']!=b['時点'] and not settings['時点差']:
        _失敗('意味未確定','時点が異なる。時点差を比較する目的を明示する')
    if a['資料']==b['資料'] and a.get('位置')==b.get('位置'):
        _失敗('意味未確定','同じ値を異なる比較対象として扱わない')
    av,bv=Fraction(a['値']),Fraction(b['値']); delta=bv-av
    関係=関係制約器().実行(関係問題(('左','右','基準'),
        (関係式('前提左','左','一致','基準',a['値']),関係式('前提右','右','一致','基準',b['値'])),
        (関係式('右大','右','超','左'),関係式('同値','右','一致','左'),関係式('右小','右','未満','左')),
        属性=a['属性'],単位=a['単位'],条件=a['条件']))
    if not 関係.成立: _失敗('能力不成立','既存関係制約器が成立しない')
    decisions={r['問い']['識別子']:r['判定'] for r in 関係.データ['回答']}
    expected='右大' if delta>0 else '右小' if delta<0 else '同値'
    if decisions.get(expected)!='導出': _失敗('能力不成立','関係推論と数量差が一致しない')
    display_unit=a['表示単位'] if a['表示単位']==b['表示単位'] else a['単位']
    display_delta=_数表記(delta/Fraction(_単位[display_unit][1]))
    body='右－左の差は'+display_delta+' '+display_unit
    資料={'版':会話数量版,'種別':'数量比較','左':_結果辞書(left),'右':_結果辞書(right),
          '設定':deepcopy(settings),'差':_数表記(delta),'方向':expected,
          '表示単位':display_unit,'表示差':display_delta,
          '関係報告':_結果辞書(関係),'本文':body,
          '限界':'資料の記載値の比較。原因・優劣・資料の真偽は判定していない'}
    資料['記録SHA256']=意味指紋(資料)
    return 能力結果(True,body,参照=_参照結合((*left.参照,*right.参照)),データ=資料)


def 比較記録整合(value):
    try:
        _結果辞書(value)
        資料=value.データ
        rebuilt=数量を比較(能力結果を復元(資料['左']),能力結果を復元(資料['右']),資料['設定'])
        available={r.識別子:r for r in _参照結合(value.参照)}
        return value.成立 and rebuilt.データ==資料 and rebuilt.本文==value.本文 and rebuilt.根拠==value.根拠 and rebuilt.保留理由==value.保留理由 and all(available.get(r.識別子)==r for r in rebuilt.参照)
    except (ValueError,KeyError,TypeError,AttributeError,RecursionError):
        return False
