"""判定根拠の依存グラフから説明を構成する。回答テンプレートを推論の代替にしない。"""
from __future__ import annotations

from .命題能力接続 import 命題判定整合
from .導出談話 import 導出説明節
from .命題構造 import 命題を復元
from .命題解釈 import 命題を表現


def 命題の表現(value, *, 形式='文章', 手順=False):
    if 形式 not in ('文章', '表') or type(手順) is not bool:
        raise ValueError('命題の説明形式不正')
    if not 命題判定整合(value): raise ValueError('命題判定の再構成不一致')
    return 導出の表現(value.データ, 形式=形式, 手順=手順)


def 導出の表現(d, *, 形式='文章', 手順=False):
    r = d['判定結果']; status = r['判定']
    expression = 命題を表現(命題を復元(r['問い']))
    conclusions = {
        '支持': '提供記載と実装された導出規則から支持されます。',
        '反証': '提供記載から反対命題が導かれ、反証されます。',
        '矛盾': '支持と反証の両方が導かれます。一方を選んで確定しません。',
        '未確定': '支持も反証も導かれていません。偽であるとは判定しません。',
    }
    roots = tuple(p for p in (r['支持'], r['反証']) if p)
    units = [('命題判定', f'「{expression}」は、{conclusions[status]}', roots, ())]
    nodes = r['導出']; order = []; seen = set()
    def visit(pid):
        if pid in seen: return
        seen.add(pid)
        for parent in nodes[pid]['親']: visit(parent)
        order.append(pid)
    for pid in roots: visit(pid)
    numbers = {pid: i + 1 for i, pid in enumerate(order)}
    sources = {row['識別子']: row for row in d['記載']}
    origins = []
    for pid in order:
        p = nodes[pid]; 情報源 = sources.get(p['出典'])
        if 情報源:
            a, b = 情報源['範囲']
            origins.append(f'資料「{情報源["資料"]}」[{a}:{b}]：{情報源["原文"]}')
    if 手順 or 形式 == '表':
        lines = []
        if 形式 == '表': lines.extend(('| 工程 | 作用 | 命題 | 依存工程 |', '|---:|---|---|---|'))
        for pid in order:
            p = nodes[pid]
            text = 命題を表現(命題を復元(p['式'], 内部許可=True))
            parents = '、'.join(str(numbers[x]) for x in p['親']) or '原記載'
            if 形式 == '表':
                safe = text.replace('|', '\\|').replace('\n', ' ')
                lines.append(f'| {numbers[pid]} | {p["作用"]} | {safe} | {parents} |')
            else:
                clause = 導出説明節(numbers[pid], p['作用'], text, tuple(numbers[x] for x in p['親']))
                lines.append(f'{numbers[pid]}. {p["作用"]}：{clause.本文}（依存：{parents}）')
        if not order: lines.append('問いに到達する支持・反証の導出経路はありません。')
        units.append(('導出手順', '\n'.join(lines), roots, ()))
    limits = [r['解釈境界'], '時点・様相の異なる記載を自動的に同一視していません。']
    if any(p['作用'] in ('問いの仮定', '場合の仮定') for p in nodes.values()):
        limits.append('導出中の仮定は条件付きの検討だけに使い、事実として保存していません。')
    return tuple(units), tuple(limits), tuple(dict.fromkeys(origins))
