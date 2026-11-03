"""最終採否を通過した結果から出力契約を形成する。実行・学習・再評価は起動しない。"""
from __future__ import annotations
from ..共通契約.出力 import 出力束, 表現契約, 出力政策
from ..共通契約.封緘 import 封緘する
from ..コア.内容計画 import 内容計画, 内容計画を検査
from ..コア.値 import 署名
from ..HDS実行主体 import HDS実行結果, HDS終端
from ..統合駆動_v2.目的保持 import 目的契約署名
from ..統合駆動_v2.政策 import 停止理由
from .契約 import 関係要求, 構造要求, 関係出力束, 構造出力束
from .接続 import 要求成果, 関係結果を読む


def _入力対応を確認(結果, 束, 案件ID):
    from ..入力系.契約 import 入力束 as 入力束型
    if not isinstance(束, 入力束型) or not 束.指示として使用可能:
        raise ValueError('対応する指示入力束が必要')
    if 案件ID is not None and 案件ID != 束.原本.案件ID:
        raise ValueError('出力案件と入力案件が不一致')
    主体 = dict(結果.状態.主体状態)
    要求 = dict(結果.状態.成果).get(要求成果)
    if isinstance(要求, 関係要求):
        if (要求.ID != '入力:' + 束.原本.案件ID + ':' + 束.原本.入力ID
                or 要求.版 != str(束.原本.版) + ':' + 束.全域署名
                or 要求.目的 != 束.原本.本文):
            raise ValueError('関係結果と入力正本が不一致')
    elif 主体.get('HDS入力受理署名') != 束.全域署名:
        raise ValueError('駆動結果に対応する受理済み入力がない')
    return 束.原本.案件ID, 束.全域署名


def _入力表現に合わせる(元, 指定):
    from dataclasses import replace
    必須 = {}
    if 元.出力言語 is not None:
        必須['言語'] = 元.出力言語
    for 項 in 元.要求:
        if 項.種別 == '出力言語':
            鍵, 値 = '言語', 項.値
        elif 項.種別 == '形式' and 項.値 in ('JSON', '文章'):
            鍵, 値 = '形式', 'JSON' if 項.値 == 'JSON' else 'テキスト'
        elif 項.種別 == '詳細度' and 項.値 in ('詳細', '簡潔'):
            鍵, 値 = '詳細', 項.値 == '詳細'
        else:
            raise ValueError('未対応の表現要求を黙って削除しない: ' + 項.種別)
        if 鍵 in 必須 and 必須[鍵] != 値:
            raise ValueError('入力の表現要求が競合')
        必須[鍵] = 値
    if 指定 is None:
        return replace(表現契約(), **必須)
    if any(getattr(指定, k) != v for k, v in 必須.items()):
        raise ValueError('明示表現契約が入力要求に反する')
    return 指定


def 結果から出力を構成(結果: HDS実行結果, *, 案件ID: str | None = None,
        出力ID: str = '回答', 判断版: int = 1, 表現: 表現契約 | None = None,
        成果名: str | None = None, 内容言語: str = 'ja', 入力束=None,
        政策: 出力政策 | None = None) -> 出力束:
    if not isinstance(結果, HDS実行結果):
        raise TypeError('最終HDS実行結果が必要')
    if 結果.終端 == HDS終端.実行中:
        raise ValueError('実行中の内容を採用結果として返さない')
    制限 = 政策 if 政策 is not None else 出力政策()
    if not isinstance(制限, 出力政策):
        raise TypeError('出力政策型が必要')
    if 表現 is not None and not isinstance(表現, 表現契約):
        raise TypeError('表現契約型が必要')
    入力署名 = ''
    if 入力束 is not None:
        案件ID, 入力署名 = _入力対応を確認(結果, 入力束, 案件ID)
        表現 = _入力表現に合わせる(入力束.コア入力.表現制約, 表現)
    成果 = 結果.状態.成果辞書()
    if 入力束 is None and 'HDSコア入力' in 成果:
        # 入力束の省略を、結果に保持された元指示の表現制約の迂回へ使わない。
        from ..HDSコア入力 import HDSコア入力束
        元入力 = 成果['HDSコア入力']
        if not isinstance(元入力, HDSコア入力束):
            raise TypeError('駆動結果の意味正本型が不正')
        表現 = _入力表現に合わせる(元入力.表現制約, 表現)
        主体 = dict(結果.状態.主体状態)
        入力署名 = 主体.get('HDS入力受理署名', '')
        入力案件 = 主体.get('HDS入力案件')
        if 入力案件 and 案件ID is not None and 案件ID != 入力案件[0]:
            raise ValueError('保持された入力案件と出力案件が不一致')
    契約 = 表現 if 表現 is not None else 表現契約()
    要求 = 成果.get(要求成果)
    if 案件ID is None:
        if isinstance(要求, (関係要求, 構造要求)):
            案件ID = 要求.ID
        else:
            入力案件 = dict(結果.状態.主体状態).get('HDS入力案件')
            if 入力案件:
                案件ID = 入力案件[0]
            else:
                raise ValueError('一般出力には案件IDを明示する')
    if 結果.終端 == HDS終端.採用 and not 結果.状態.閉包済み:
        raise ValueError('採用状態と閉包条件の不一致')
    状態 = '成立' if 結果.終端 == HDS終端.採用 else '失敗' if 結果.終端 == HDS終端.失敗 else '保留'
    if 状態 == '保留':
        状態 = {停止理由.明示停止:'停止', 停止理由.予算枯渇:'予算枯渇',
            停止理由.証拠競合:'競合', 停止理由.観測不足:'観測待ち'}.get(結果.停止種別, '保留')
    理由 = tuple(結果.理由) + ((結果.停止種別.value,) if 結果.停止種別 is not None else ())
    未充足 = tuple(sorted(結果.状態.未達状態 | 結果.状態.残差))
    未充足 += tuple(x.問合せ for x in 結果.観測待ち)
    種別, 内容 = '診断', None
    根拠, 条件, 留保 = (), (), ()
    保証 = ('指定された駆動検証契約内の採用結果',) if 状態 == '成立' else ('内容未採用。確定回答ではない',)
    if isinstance(要求, (関係要求, 構造要求)):
        if 成果名 is not None:
            raise ValueError('関係結果で任意の内部成果を外部へ選択できない')
        内容 = 関係結果を読む(結果)
        if isinstance(内容, 関係出力束):
            種別 = '関係'
            根拠 = tuple(dict.fromkeys(r for a in (*内容.回答, *内容.仮説) for r in a.根拠))
            条件 = tuple(dict.fromkeys(c for a in 内容.回答 for c in a.節.条件))
            if 内容.仮説:
                留保 = ('仮説欄は未検証であり、回答とは別である',)
            if 状態 == '成立':
                保証 = tuple(dict.fromkeys(a.保証.value for a in 内容.回答))
        else:
            種別 = '構造'
            保証 = (内容.保証,)
        未充足 += 内容.未充足
    elif 状態 == '成立':
        if type(成果名) is not str or not 成果名 or 成果名 not in 成果:
            raise ValueError('一般出力には既存成果の名前を明示する。全成果や先頭候補を自動出力しない')
        if '成果:' + 成果名 in 結果.状態.再評価待ち:
            raise ValueError('失効した成果は出力できない')
        内容 = 成果[成果名]
        if isinstance(内容, 内容計画):
            if not 内容計画を検査(内容):
                raise ValueError('内容計画が不整合')
            種別 = '内容計画'
            根拠 = tuple(dict.fromkeys((*内容.由来, *(r for a in 内容.単位 for r in a.根拠))))
            条件 = tuple(dict.fromkeys(c for a in 内容.単位 for c in a.条件))
            留保 = 内容.留保
        else:
            種別 = '値'
            def 非言語か(値):
                if 値 is None or type(値) in (int, float, bool): return True
                if type(値) in (tuple, list): return all(非言語か(x) for x in 値)
                return False
            if 非言語か(内容):
                内容言語 = '非言語'
    if 状態 != '成立' and not 理由 and not 未充足:
        理由 = ('駆動系で要求成立を確認できなかった',)
    目的署名 = 目的契約署名(結果.状態)
    # 署名のみ。結果全体・内部資料・学習正本を出力へ移さない。
    結果署名 = 署名((結果.終端, 結果.状態.状態署名, 結果.理由, 結果.停止種別,
        結果.状態.検証票, 結果.観測待ち))
    return 出力束(案件ID, 出力ID, 判断版, 目的署名, 結果署名, 状態, 種別,
        封緘する(内容, 最大要素=制限.最大内容要素, 最大バイト=制限.最大内容バイト),
        契約, 内容言語, 入力署名, tuple(dict.fromkeys(理由)), 根拠, 条件, 留保,
        保証, tuple(dict.fromkeys(未充足)))
