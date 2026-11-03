"""採否を変更せず表現する。言語変換の意味処理はここで新造しない。"""
from __future__ import annotations
import json
from copy import deepcopy
from dataclasses import asdict
from ..共通契約.出力 import 出力取得結果, 表現結果, 表現器
from ..共通契約.封緘 import 封緘する
from .互換 import 既存内容計画を表現, 既存多言語値を表現


class 表現契約不成立(ValueError):
    """固定の契約診断。外部部品の例外本文とは分離する。"""


_見出し = {
    'ja': ('状態', '理由', '根拠', '条件', '留保', '保証', '未充足', '仮説'),
    'en': ('Status', 'Reasons', 'Evidence', 'Conditions', 'Qualifications', 'Guarantee', 'Unresolved', 'Hypotheses'),
    'zh': ('状态', '理由', '依据', '条件', '保留', '保证', '未满足', '假设'),
}


def _言語(指定):
    code = 指定.lower().split('-', 1)[0]
    if code not in _見出し:
        raise 表現契約不成立('標準表現器の未対応言語')
    return code


def 構造辞書を作る(束):
    """JSONは完全な型付き内容を保持。内部履歴や未選択成果は含めない。"""
    return {
        '形式版': 'MINIDORA-構造出力-v1',
        '案件ID': 束.案件ID, '出力ID': 束.出力ID, '判断版': 束.判断版,
        '目的署名': 束.目的署名, '結果署名': 束.結果署名, '入力署名': 束.入力署名,
        '状態': 束.状態, '内容成立': 束.状態 == '成立', '内容種別': 束.内容種別,
        '内容言語': 束.内容言語, '内容署名': 束.内容署名,
        '内容符号化': '封緘木-v1', '内容': 束.内容.木,
        '理由': 束.理由, '根拠': 束.根拠, '条件': 束.条件, '留保': 束.留保,
        '保証': 束.保証, '未充足': 束.未充足,
    }


def _関係文(内容):
    行 = []
    for 群名, 群 in (('回答', 内容.回答), ('未検証仮説', 内容.仮説)):
        for 項目 in 群:
            節 = 項目.節
            # 記号・型・単位・束縛域を削除せず、全項目を表示する。
            値 = {'関係': asdict(節),
                '束縛': tuple((asdict(k), asdict(v)) for k, v in 項目.束縛),
                '根拠': 項目.根拠, '使用契約': 項目.使用契約, '保証': 項目.保証.value}
            行.append(群名 + ': ' + json.dumps(値, ensure_ascii=False, sort_keys=True, allow_nan=False))
    if 内容.使用形成:
        行.append('使用形成: ' + ', '.join(内容.使用形成))
    return '\n'.join(行)


def 必須付随文(束):
    """出力条件を短文化指定だけで切らない。文字列内容そのものは翻訳しない。"""
    ラベル = _見出し[_言語(束.表現.言語)]
    行 = [ラベル[0] + ': ' + 束.状態]
    for 見出し, 群 in zip(ラベル[1:7], (束.理由, 束.根拠, 束.条件, 束.留保, 束.保証, 束.未充足)):
        for 要素 in 群:
            行.append(見出し + ': ' + 要素)
    return tuple(行)


def _標準表現(束):
    if 束.表現.形式 == 'JSON':
        return json.dumps(構造辞書を作る(束), ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False), 'application/json; charset=utf-8'
    if 束.表現.形式 != 'テキスト':
        raise 表現契約不成立('標準表現器の未対応形式')
    言語 = _言語(束.表現.言語)
    内容 = 束.内容を読む()
    if 束.内容種別 in ('内容計画', '値') and 束.内容言語 not in ('非言語', 束.表現.言語):
        raise 表現契約不成立('表現器へ未翻訳の内容を渡して言語変換を捏造しない')
    if 束.内容種別 in ('関係', '構造') and 言語 != 'ja':
        raise 表現契約不成立('関係・構造の標準文章表現は日本語。JSONまたは明示表現器を使う')
    if 束.内容種別 == '内容計画':
        本文 = 既存内容計画を表現(内容, 詳細=束.表現.詳細, 最大文字数=min(100000, 束.表現.最大文字数))
    elif 束.内容種別 == '値':
        本文 = 既存多言語値を表現(内容, '成立', 束.理由, 束.表現.言語)
    elif 束.内容種別 == '関係':
        本文 = _関係文(内容)
    elif 束.内容種別 == '構造':
        本文 = '構造結果: ' + json.dumps(内容.構造成果 and asdict(内容.構造成果), ensure_ascii=False, sort_keys=True, allow_nan=False)
    else:
        状態 = '失敗' if 束.状態 == '失敗' else '保留'
        本文 = 既存多言語値を表現(None, 状態, 束.理由, 束.表現.言語)
    return '\n'.join((本文, *必須付随文(束))), 'text/plain; charset=utf-8'


def 表現を変換(取得: 出力取得結果, 器: 表現器 | None = None) -> 表現結果:
    束 = 取得.束
    try:
        if 束.表現.表現器ID == '標準':
            if 束.表現.表現器版 != 'v1' or 器 is not None:
                raise 表現契約不成立('標準表現契約の版不一致')
            本文, 媒体 = _標準表現(束)
        else:
            if 器 is None or (器.ID, 器.版) != (束.表現.表現器ID, 束.表現.表現器版):
                raise 表現契約不成立('指定表現器が未接続')
            if 束.表現.形式 not in 器.形式 or 束.表現.言語 not in 器.言語:
                raise 表現契約不成立('表現器の契約範囲外')
            # 駆動状態・入力束を渡さず、新しい独立コピーだけ渡す。
            独立 = 封緘する(束.内容を読む())
            値 = 独立.復元()
            呼出束 = deepcopy(束)
            前署名 = 呼出束.署名
            本文 = 器.変換(値, 呼出束)
            if 呼出束.署名 != 前署名 or 封緘する(値).署名 != 独立.署名:
                raise 表現契約不成立('表現器が読取内容を改変した')
            検証値 = 独立.復元()
            検証束 = deepcopy(束)
            合格 = 器.検証(検証値, 本文, 検証束)
            if (type(合格) is not bool or not 合格 or 検証束.署名 != 前署名
                    or 封緘する(検証値).署名 != 独立.署名):
                raise 表現契約不成立('表現検証が不成立または入力を改変した')
            媒体 = 器.媒体型
            # 必須の意味留保は表現器へ委譲せず、不変の外側封筒で保持する。
            if 媒体 != 'text/plain; charset=utf-8':
                raise 表現契約不成立('拡張表現器は検証付きtext/plainに限定。未対応媒体は送達しない')
            if type(本文) is not str or not 本文:
                raise TypeError('表現器は非空strを返す必要がある')
            本文 = '\n'.join((本文, *必須付随文(束)))
        if type(本文) is not str or not 本文:
            raise TypeError('表現は非空のstrが必要')
        if len(本文) > 束.表現.最大文字数 or len(本文.encode('utf-8')) > 束.表現.最大バイト:
            raise 表現契約不成立('表現容量超過。必須内容の切断はしない')
        # JSONはエスケープ後でなく、復号した文字列葉で必須内容を検査する。
        確認本文 = 本文
        if 束.表現.形式 == 'JSON' and 束.表現.表現器ID == '標準':
            実値 = json.loads(本文)
            比較値 = json.loads(json.dumps(構造辞書を作る(束), ensure_ascii=False, allow_nan=False))
            if 実値 != 比較値:
                raise 表現契約不成立('構造出力の往復不一致')
            def 文字葉(値):
                if isinstance(値, str): return [値]
                if isinstance(値, dict): return [s for v in 値.values() for s in 文字葉(v)]
                if isinstance(値, list): return [s for v in 値 for s in 文字葉(v)]
                return []
            葉 = 文字葉(実値)
            if any(not any(x in s for s in 葉) for x in 束.表現.必須内容):
                raise 表現契約不成立('必須内容が表現されていない')
        elif any(x not in 確認本文 for x in 束.表現.必須内容):
            raise 表現契約不成立('必須内容が表現されていない')
        return 表現結果(束.署名, 束.内容署名, 束.表現.署名, 本文, 媒体, True)
    except Exception as 例外:
        # 失敗は返すが、例外本文に混在し得る未採用資料を外へ漏らさない。
        return 表現結果(束.署名, 束.内容署名, 束.表現.署名, '', 'text/plain; charset=utf-8', False,
            (('表現契約不成立:' + str(例外)) if type(例外) is 表現契約不成立 else ('表現不成立:' + type(例外).__name__),))
