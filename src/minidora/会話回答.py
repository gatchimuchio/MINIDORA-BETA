"""採用候補の意味・根拠・留保を保持して文章化する。未知の説明を追加しない。"""
from __future__ import annotations
from copy import deepcopy
from fractions import Fraction
from .証拠統合 import _単位,_数表記
from dataclasses import asdict, dataclass
from .能力合成 import _結果辞書, _参照結合, _符号化
from .応答構成 import 能力結果を復元
from .会話意味 import 意味指紋
from .会話数量 import 数量記録整合,比較記録整合,会話処理不成立
from .製品版.型 import 能力結果
from .コア.内容計画 import 内容単位, 内容計画, 内容計画を検査, 内容計画を表現

会話回答版='MINIDORA-会話回答-v0.4'

@dataclass(frozen=True, slots=True)
class 回答命題:
    種別: str
    本文: str
    根拠: tuple[str,...]
    条件: tuple[str,...] = ()

@dataclass(frozen=True, slots=True)
class 回答意味IR:
    命題: tuple[回答命題,...]
    留保: tuple[str,...]
    由来: tuple[str,...]


def _呼称(q):
    label='資料「'+q['資料']+'」'
    if q.get('時点') is not None: label+='（'+q['時点']+'）'
    return label


def 回答を構成(values: tuple[能力結果,...], *, 詳細=False, 最大文字数=100000, 形式='文章', 手順=False):
    if type(values) is not tuple or not 1<=len(values)<=16 or type(詳細) is not bool:
        raise ValueError('回答の入力・表示設定不正')
    if type(最大文字数) is not int or not 1<=最大文字数<=100000:
        raise ValueError('回答文字数上限不正')
    if 形式 not in ('文章','表') or type(手順) is not bool: raise ValueError('回答の表現条件不正')
    propositions=[]; caveats=[]; origins=[]
    for i,value in enumerate(values):
        _結果辞書(value)
        if not value.成立: raise ValueError('不成立結果を文章化しない')
        if value.データ.get('版')==会話回答版:
            raise ValueError('回答IRは原成果へ戻してから再構成する')
        kind=value.データ.get('種別');ref=f'成果:{i}'
        if kind in ('文脈命題判定', '取得命題判定'):
            from .文脈命題回答 import 文脈の表現
            units, extra_caveats, extra_origins = 文脈の表現(value, 形式=形式, 手順=手順 or 詳細)
            for k, text, dependencies, conditions in units:
                propositions.append(回答命題(k, text, tuple(ref+'/'+x for x in dependencies), conditions))
            caveats.extend(extra_caveats); origins.extend(extra_origins)
        elif kind=='命題判定':
            from .命題回答 import 命題の表現
            units, extra_caveats, extra_origins = 命題の表現(value, 形式=形式, 手順=手順 or 詳細)
            for k, text, dependencies, conditions in units:
                propositions.append(回答命題(k, text, tuple(ref+'/'+x for x in dependencies), conditions))
            caveats.extend(extra_caveats); origins.extend(extra_origins)
        elif kind=='数量集合':
            from .数量集合 import 集合の表現
            units,extra_caveats,extra_origins=集合の表現(value,形式=形式,手順=手順)
            for k,text,dependencies,conditions in units:
                propositions.append(回答命題(k,text,tuple(ref+'/'+x for x in dependencies),conditions))
            caveats.extend(extra_caveats);origins.extend(extra_origins)
        elif kind=='数量比較':
            if 形式!='文章' or 手順: raise ValueError('二資料比較の表・手順指定は集合要求で実行する')
            if not 比較記録整合(value): raise ValueError('比較結果の再構成不一致')
            資料=value.データ;a=資料['左']['データ'];b=資料['右']['データ']
            unit=資料['表示単位']; direction=資料['方向']
            av=_数表記(Fraction(a['値'])/Fraction(_単位[unit][1]))
            bv=_数表記(Fraction(b['値'])/Fraction(_単位[unit][1]))
            first=f'{a["属性"]}の記載値は、{_呼称(a)}が{av}{unit}、{_呼称(b)}が{bv}{unit}です。'
            second=('両者は同じ値です。' if direction=='同値' else
                    f'{_呼称(b)}の方が大きく、差（右－左）は{資料["表示差"]}{unit}です。' if direction=='右大' else
                    f'{_呼称(b)}の方が小さく、差（右－左）は{資料["表示差"]}{unit}です。')
            conditions=tuple('条件：'+x for x in (a.get('条件'),) if x)
            if a.get('時点')!=b.get('時点'): conditions+=('指定された二時点間の比較です。',)
            propositions.append(回答命題('比較',first+second,(ref,),conditions))
            caveats.append(資料['限界'])
            if a['条件'] is None: caveats.append('条件未記載同士の記載比較であり、現実の条件が同一とは認定していません。')
            for x in (a,b):
                origins.append(_呼称(x)+'：'+x.get('位置','')+'、原文「'+x['原文片']+'」')
                if x['単位由来']=='依頼の宣言': caveats.append('資料に単位の記載がない値には、依頼で指定された単位を適用しています。')
        elif kind=='数量':
            if 形式!='文章' or 手順: raise ValueError('単一数量の表・手順表示は未対応')
            if not 数量記録整合(value): raise ValueError('数量結果の再構成不一致')
            q=value.データ
            conditions=tuple(f'{k}：{q[k]}' for k in ('条件','時点') if q.get(k) is not None)
            propositions.append(回答命題('記載',f'{q["資料"]}の{q["属性"]}は、参照資料では{q["表示値"]}{q["表示単位"]}と記載されています。',(ref,),conditions))
            origins.append(q.get('位置','')+'、原文「'+q['原文片']+'」')
            caveats.append('資料の記載内容であり、世界の事実性や最新性を認定したものではありません。')
        else:
            if 形式!='文章' or 手順: raise ValueError('この能力結果の表・手順生成は未実装')
            # 任意能力への説明を捏造しない。本文を保持する処理結果引用に限定。
            propositions.append(回答命題('処理結果','処理結果：\n'+value.本文,(ref,)))
            if value.保留理由: raise ValueError('成功結果に保留理由が混在')
        origins.extend(r.題名+('：'+r.URL if r.URL else '') for r in value.参照)
    meaning=回答意味IR(tuple(propositions),tuple(dict.fromkeys(caveats)),tuple(dict.fromkeys(origins)))
    内容計画値 = 内容計画(
        tuple(内容単位(p.種別, p.本文, p.根拠, p.条件) for p in meaning.命題),
        meaning.留保,
        meaning.由来 or (("既存能力の実行結果。外部資料の参照なし。",) if 詳細 else ()),
    )
    if not 内容計画を検査(内容計画値):
        raise ValueError('回答内容計画が不整合')
    body = 内容計画を表現(内容計画値, 詳細=詳細, 最大文字数=最大文字数)
    資料={'版':会話回答版,'意味':asdict(meaning),'元結果':[_結果辞書(v) for v in values],
          '詳細':詳細,'最大文字数':最大文字数,'形式':形式,'手順':手順,'本文':body}
    資料['記録SHA256']=意味指紋(資料)
    return 能力結果(True,body,根拠=tuple(f'成果:{i}' for i in range(len(values))),
                     参照=_参照結合(r for v in values for r in v.参照),データ=資料)


def 回答記録整合(value):
    try:
        _結果辞書(value)
        d=value.データ
        rebuilt=回答を構成(tuple(能力結果を復元(v) for v in d['元結果']),詳細=d['詳細'],最大文字数=d['最大文字数'],形式=d.get('形式','文章'),手順=d.get('手順',False))
        available={r.識別子:r for r in _参照結合(value.参照)}
        return value.成立 and _符号化(rebuilt.データ)==_符号化(d) and rebuilt.本文==value.本文 and rebuilt.根拠==value.根拠 and rebuilt.保留理由==value.保留理由 and all(available.get(r.識別子)==r for r in rebuilt.参照)
    except (KeyError,ValueError,TypeError,AttributeError,RecursionError):
        return False


def 回答を再表現(value, *, 詳細=False, 形式=None, 手順=None):
    if not 回答記録整合(value): raise ValueError('前回回答の原成果再構成不一致')
    return 回答を構成(tuple(能力結果を復元(v) for v in value.データ['元結果']),詳細=詳細,
        形式=value.データ.get('形式','文章') if 形式 is None else 形式,
        手順=value.データ.get('手順',False) if 手順 is None else 手順)
