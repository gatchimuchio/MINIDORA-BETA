"""解釈条件・発言帰属・導出を分けて説明する。原因や信頼性を創作しない。"""
from __future__ import annotations
from .文脈命題接続 import 文脈報告整合, 取得命題整合
from .命題回答 import 導出の表現
from .応答構成 import 能力結果を復元


def 文脈の表現(value, *, 形式='文章', 手順=False):
    if 形式 not in ('文章', '表') or type(手順) is not bool: raise ValueError('文脈回答の表示設定不正')
    limits = ['発言・信念・伝聞の内容を、そのまま事実として採用していません。']
    if value.データ.get('種別') == '取得命題判定':
        if not 取得命題整合(value): raise ValueError('取得命題報告の再構成不一致')
        raw = value.データ['元取得']['データ']
        limits.append('取得できた全文の範囲の検討です。検索の網羅性・資料の真実性・最新性は未認定です。')
        origins = ['取得時刻：' + d['取得時刻'] + '、URL：' + d['最終URL'] for d in raw['資料']]
        report = 能力結果を復元(value.データ['文脈報告'])
    else:
        report = value; origins = []
    if not 文脈報告整合(report): raise ValueError('文脈報告の再構成不一致')
    結果 = report.データ['判定結果']; units = []
    状態 = 結果['解釈状態']
    if 状態 == '読み未確定':
        limits.append('資料の読みは未確定のまま、全ての保持した解釈を別々に検討しています。')
        text = (f'保持した{結果["場合総数"]}通りの読みで判定は共通です。読み自体の一意性は主張しません。'
                if 結果['判定'] != '解釈依存' else '資料の読みで結論が異なります。単一の結論へ統合しません。')
        units.append(('解釈境界', text, (), ()))
    elif 状態 == '利用者選択':
        limits.append(f'利用者が指定した資料解釈{結果["資料候補"]}の下での判定です。')
    for case in 結果['場合別']:
        number = case['場合']; readings = tuple(x['記載'] + '：' + x['読み'] for x in case['選択'])
        情報源 = {'判定結果': case['判定結果'], '記載': case['記載']}
        rows, caveats, refs = 導出の表現(情報源, 形式=形式, 手順=手順)
        for kind, text, deps, conditions in rows:
            header = f'解釈{number}：' if 結果['場合総数'] > 1 else ''
            units.append((kind, header + text, tuple(f'場合:{number}/'+d for d in deps),
                          (*readings, *conditions)))
        for resolved in case['照応解消']:
            units.append(('照応解消', f'「{resolved["原文"]}」を「{resolved["束縛先"]}」へ束縛しました。',
                          (resolved['記載'],), (resolved['理由'],)))
        limits.extend(caveats); origins.extend(refs)
    return tuple(units), tuple(dict.fromkeys(limits)), tuple(dict.fromkeys(origins))
