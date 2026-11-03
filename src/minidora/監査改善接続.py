"""限定した意味拡張・仮説探索・介入比較を既存能力合成器へ接続する。

追加能力は明示登録で使う。既定製品モード、HDS採否、永続会話を置換しない。
通常チャットの要求解釈への自動登録は、本モジュールの責任ではない。
"""
from __future__ import annotations

from copy import deepcopy
from datetime import datetime
from .能力合成 import (能力合成器, 合成計画, 合成工程, 素材参照, 登録能力,
                     _符号化, _結果辞書)
from .製品版.型 import 能力結果, 参照資料
from .製品版.能力契約 import 能力文脈
from .会話意味 import 意味指紋
from .命題解釈 import 命題を読む
from .文脈命題 import 文脈資料を読む, 文脈判定
from .有限仮説探索 import 仮説を検討, 仮説報告を検査, 仮説探索版, _欄, _列
from .有限因果モデル import 介入を比較, 介入報告を検査, 因果モデル版

意味拡張版 = 'MINIDORA-有界文脈検討-v0.2'
改善回答版 = 'MINIDORA-監査改善回答-v0.3'
from .資料読解 import 資料読解版, 資料を読解, 読解報告を検査, 読解説明節
from .導出説明 import 導出説明を構成, 記載を使わない導出の説明


def 拡張命題を検討(要求: dict) -> dict:
    _欄(要求, {'資料', '問い'}, {'照応距離', '問い候補', '資料候補', '述語別名'})
    if len(_符号化(要求)) > 300000:
        raise ValueError('拡張命題要求のバイト上限')
    要求 = deepcopy(要求)
    sources = _列(要求['資料'], '資料', 8, 1)
    docs = []
    for 情報源 in sources:
        _欄(情報源, {'名前', '本文'})
        docs.append(文脈資料を読む(情報源['本文'], 情報源['名前'], 照応距離=要求.get('照応距離', 1)))
    candidates = 命題を読む(要求['問い'])
    if len(candidates) != 1 and '問い候補' not in 要求:
        raise ValueError('問いの読みが複数。問い候補を明示して確認を継続する')
    結果 = 文脈判定(tuple(docs), 要求['問い'], 要求.get('問い候補', 1), 要求.get('資料候補', 0),
                    述語別名=要求.get('述語別名', ()))
    report = {'版': 意味拡張版, '要求': 要求, '状態': 結果['判定'],
              '資料候補': docs, '判定結果': 結果, '事実認定': False,
              '限界': '提供資料の対応構文と明示した有界照応規約での判定。話者の意図、一般語義、実世界の真実性は未認定。'}
    report['記録SHA256'] = 意味指紋(report)
    return report


def 拡張命題報告を検査(報告: dict) -> bool:
    try:
        return _符号化(報告) == _符号化(拡張命題を検討(報告['要求']))
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return False


def 改善報告を検査(報告: dict) -> bool:
    if type(報告) is not dict:
        return False
    validators = {意味拡張版: 拡張命題報告を検査, 仮説探索版: 仮説報告を検査,
                  因果モデル版: 介入報告を検査, 資料読解版: 読解報告を検査}
    version = 報告.get('版')
    if type(version) is not str or version not in validators:
        return False
    return validators[version](報告)


def 改善回答を構成(報告: dict, *, 詳細: bool = True) -> dict:
    """再計算で整合した報告から説明節を作る。根拠のない内容を補作しない。"""
    if type(詳細) is not bool or not 改善報告を検査(報告):
        raise ValueError('回答元報告の再構成不一致又は表示条件不正')
    報告 = deepcopy(報告)
    節 = []

    def 追加(役割, 本文, 根拠=()):
        節.append({'役割': 役割, '本文': 本文, '根拠': list(根拠)})

    if 報告['版'] == 仮説探索版:
        追加('境界', '提供規則と指定した仮説候補の範囲で検討しました。候補は事実ではありません。')
        if 報告['状態'] == '背景不整合':
            追加('不整合', '背景事実・規則又は観測が整合しないため、仮説候補を採用していません。')
        elif not 報告['候補']:
            追加('未確定', '指定した候補集合と仮説数の範囲には、観測を説明する整合した組合せがありません。')
        for number, 候補 in enumerate(報告['候補'], 1):
            assumption = '、'.join(候補['仮説'])
            追加('仮定', f'候補{number}：{assumption}を仮定します。' if assumption else f'候補{number}：追加仮説は不要です。')
            if 詳細:
                関係図, seen = 候補['導出'], set()

                def 説明(pid):
                    if pid in seen:
                        return
                    seen.add(pid)
                    node = 関係図[pid]
                    for parent in node['親']:
                        説明(parent)
                    if node['作用'] == '提供事実':
                        追加('提供記載', f"提供事実 {node['入力ID']}（{node['出典']}）：{node['命題']}。", (pid,))
                    elif node['作用'] == '仮説導入':
                        追加('仮定', f"仮説として置く命題：{node['命題']}。", (pid,))
                    elif node['作用'] == '規則適用':
                        premises = '、'.join(関係図[parent]['命題'] for parent in node['親'])
                        追加('条件導出', f"{premises}から、規則 {node['入力ID']}（{node['出典']}）を適用して{node['命題']}を導きます。", (pid, *node['親']))
                    else:
                        raise ValueError('未知の導出作用を文章で補完しない')
                for pid in 候補['観測の根拠'].values():
                    説明(pid)
            追加('条件付き結論', 'この仮定の下では、観測「' + '、'.join(候補['観測の根拠']) + '」を説明できます。', tuple(候補['観測の根拠'].values()))
    elif 報告['版'] == 因果モデル版:
        追加('境界', '同じ外生状態を保ち、指定変数の構造式を介入値へ置き換えたモデル内比較です。')
        values = 報告['要求']['介入']
        words = [name + '=' + ('真' if value else '偽') for name, value in sorted(values.items())]
        追加('介入条件', '介入：' + ('、'.join(words) if words else 'なし') + '。')
        for name, diff in 報告['変化'].items():
            before, after = ('真' if diff['前'] else '偽'), ('真' if diff['後'] else '偽')
            追加('モデル内結論', f'{name}は{before}から{after}へ変化します。', (name,))
        if not 報告['変化']:
            追加('モデル内結論', 'このモデルと外生状態では値の変化はありません。')
        if 詳細:
            for node in 報告['介入導出']:
                追加('由来', f"{node['変数']}：{node['作用']}。モデル出典：{node['出典']}。", (node['変数'],))
    elif 報告['版'] == 資料読解版:
        節.extend(読解説明節(報告, 詳細=詳細))
    else:
        結果 = 報告['判定結果']
        for definition in 結果.get('別名定義', ()):
            追加('語彙条件', f"明示定義（{definition['出典']}）：{definition['表記']}を{definition['正規名']}の同義述語として扱います（引数数{definition['引数数']}）。帰属・引用の内部には適用しません。")
        追加('資料内結論', f"提供資料内での判定は「{結果['判定']}」です。資料の解釈状態は「{結果['解釈状態']}」です。")
        if 結果['資料候補']:
            追加('選択条件', f"資料候補{結果['資料候補']}を選んだ条件の下での結果です。")
        if len(命題を読む(報告['要求']['問い'])) > 1:
            追加('選択条件', f"問い候補{結果['問い候補']}を選んだ条件の下での結果です。")
        for case in 結果['場合別']:
            追加('場合条件', f"場合{case['場合']}：{case['判定結果']['判定']}。", tuple(case['判定結果']['導出']))
            for 選択肢 in case['選択']:
                追加('選択条件', f"記載{選択肢['記載']}の読み：{選択肢['読み']}。")
            for resolution in case['照応解消']:
                追加('照応条件', f"「{resolution['原文']}」を「{resolution['束縛先']}」へ束縛：{resolution['理由']}。")
            explanation = 導出説明を構成(case['判定結果'], case['記載'])
            if 詳細:
                追加('導出境界', explanation['境界'])
                節.extend(explanation['節'])
            if not explanation['使用記載']:
                if explanation['節']:
                    # 原記載なしの論理導出と、支持・反証そのものがない未導出は別。
                    追加('導出範囲', 記載を使わない導出の説明)
                elif 詳細:
                    追加('未導出', '支持・反証の原記載根拠は得られていません。未導出は否定でも情報不在の認定でもありません。')
    追加('留保', 報告['限界'])
    out = {'版': 改善回答版, '報告': 報告, '詳細': 詳細, '節': 節,
           '本文': '\n'.join(row['本文'] for row in 節), '事実認定': False}
    out['記録SHA256'] = 意味指紋(out)
    return out


def 改善回答を検査(回答: dict) -> bool:
    try:
        return _符号化(回答) == _符号化(改善回答を構成(回答['報告'], 詳細=回答['詳細']))
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return False


def _合成素材(文脈: 能力文脈):
    if type(文脈) is not 能力文脈 or type(文脈.補助) is not dict:
        raise ValueError('明示した能力合成の文脈が必要')
    auxiliary = 文脈.補助
    rows = auxiliary.get('合成入力')
    if type(rows) is not tuple or len(rows) != 1:
        raise ValueError('単一の明示原データが必要')
    _欄(rows[0], {'参照', '結果'})
    資料 = _欄(rows[0]['結果'], {'成立', '本文', '根拠', '参照', 'データ', '保留理由'})
    refs = []
    for ref in _列(資料['参照'], '参照', 128):
        _欄(ref, {'識別子', '題名', '出典', 'URL', '公開時刻', '本文'})
        ref = dict(ref)
        if ref['公開時刻'] is not None:
            ref['公開時刻'] = datetime.fromisoformat(ref['公開時刻'])
        refs.append(参照資料(**ref))
    if type(資料['根拠']) not in (tuple, list):
        raise ValueError('原データの根拠列型')
    original = 能力結果(資料['成立'], 資料['本文'], tuple(資料['根拠']), tuple(refs), 資料['データ'], 資料['保留理由'])
    _結果辞書(original)
    if not original.成立:
        raise ValueError('未成立の原データ')
    settings = auxiliary.get('合成設定', {})
    if type(settings) is not dict:
        raise ValueError('合成設定の型不正')
    return deepcopy(original.データ), deepcopy(settings)


class 監査改善モジュール:
    版 = 'MINIDORA-監査改善接続-v0.3'
    優先度 = 0

    def __init__(self, 名前: str):
        if 名前 not in ('拡張命題検討', '有限仮説検討', '有限介入比較', '資料読解', '監査改善回答'):
            raise ValueError('未登録の監査改善能力')
        self.名前 = 名前

    def 判定(self, 文脈):
        return 1.0

    def 実行(self, 文脈):
        try:
            資料, settings = _合成素材(文脈)
            if self.名前 == '監査改善回答':
                _欄(settings, set(), {'詳細'})
                report = 改善回答を構成(資料, 詳細=settings.get('詳細', True))
                body = report['本文']
            else:
                if settings:
                    raise ValueError('検討能力の未知設定')
                functions = {'拡張命題検討': 拡張命題を検討,
                             '有限仮説検討': 仮説を検討, '有限介入比較': 介入を比較, '資料読解': 資料を読解}
                report = functions[self.名前](資料)
                body = '検討処理：' + report['状態']
            return 能力結果(True, body, 根拠=('原データ:' + 意味指紋(資料),),
                            参照=文脈.直前参照, データ=report)
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
            return 能力結果(False, '', 保留理由='監査改善:保留:' + str(exc))

    def 登録(self):
        return 登録能力(self)


def 改善能力群():
    return tuple(監査改善モジュール(name).登録() for name in
                 ('拡張命題検討', '有限仮説検討', '有限介入比較', '資料読解', '監査改善回答'))


def 改善計画を実行(種類: str, 要求: dict, *, 詳細: bool = True):
    """明示IR用の二工程実行例。任意自然文の解釈器として扱わない。"""
    names = {'命題': '拡張命題検討', '仮説': '有限仮説検討', '介入': '有限介入比較', '読解': '資料読解'}
    if type(種類) is not str or 種類 not in names or type(詳細) is not bool:
        raise ValueError('検討種類又は表示条件不正')
    plan = 合成計画((
        合成工程('検討', (names[種類],), '検討指示', (素材参照('入力', '要求'),)),
        合成工程('回答', ('監査改善回答',), '回答指示', (素材参照('工程', '検討'),), '表示設定'),
    ), ('回答',))
    資料 = {'要求': 能力結果(True, '検討の原データ', データ=deepcopy(要求)),
            '検討指示': 能力結果(True, '提供データの範囲で検討する'),
            '回答指示': 能力結果(True, '根拠・仮定・留保を保持して説明する'),
            '表示設定': 能力結果(True, '', データ={'詳細': 詳細})}
    return 能力合成器(改善能力群()).実行(plan, 資料)
