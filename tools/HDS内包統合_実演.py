"""人工入力の観測→計算→草案検証→根拠更新を実行する。外部ベンチではない。"""
from __future__ import annotations
import argparse
from dataclasses import asdict
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))
from minidora.HDS実行主体 import HDS実行主体, HDS作用結果, HDS作用状態, HDS関数作用, HDS終端
from minidora.HDS駆動コア import HDS駆動コア
from minidora.統合駆動_v2 import (
    HDS資料, HDS認識項目, HDS観測器, HDS観測値, HDS検証器, HDS草案,
    HDS運用政策, 保存する, 復元する,
)


def 実演する():
    呼出 = {'独立点検': 0, '数量計算': 0, '草案形成': 0, '草案検証': 0, '観測': 0}
    データ = {'数量': 3, '単位値': 7}
    def 原資料(版):
        return HDS資料('人工入力資料', 版, json.dumps(データ, ensure_ascii=False, sort_keys=True), 'ローカル人工入力', '2026-09-17', ('数量', '単位値'))
    現資料 = [原資料('1')]
    def 取得(要求, 状態):
        呼出['観測'] += 1; 元 = 状態.記憶.正本辞書().get('人工入力資料', 現資料[0]); value = json.loads(元.本文)[要求.対象]
        return HDS観測値(value, (元.出典(),), 要求.対象, 要求.関係, 要求.範囲, 要求.時点, (元,))
    def 観測を検証(要求, 値): return (len(値.資料群) == 1 and type(値.値) is int and json.loads(値.資料群[0].本文).get(要求.対象) == 値.値 and 値.関係 == '整数値')
    def 独立点検(状態): 呼出['独立点検'] += 1; return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({'独立済み'}))
    def 数量計算(状態):
        呼出['数量計算'] += 1; 認識 = 状態.認識辞書(); return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({'計算済み'}), 成果=(('積', 認識['数量'].値 * 認識['単位値'].値),))
    def 草案形成(状態):
        呼出['草案形成'] += 1; 値 = 状態.成果辞書()['積']; 依存 = tuple((k, 状態.ノード署名(k)) for k in ('成果:積', '認識:数量', '認識:単位値'))
        草案 = HDS草案('算出結果', (('回答', f'積={値}'),), ('積検証',), 依存, 追加状態=frozenset({'回答完了'})); return HDS作用結果(HDS作用状態.成立, 草案更新=(草案,))
    def 草案を検証(状態, 草案):
        呼出['草案検証'] += 1; x = 状態.認識辞書(); return dict(草案.成果)['回答'] == f"積={x['数量'].値 * x['単位値'].値}"
    作用群 = (
        HDS関数作用('独立点検', 独立点検, 出力状態=('独立済み',), 入力署名=lambda s: '独立契約-v1'),
        HDS関数作用('数量計算', 数量計算, 出力状態=('計算済み',), 読取認識=('数量', '単位値')),
        HDS関数作用('草案形成', 草案形成, 入力状態=('計算済み',), 読取成果=('積',), 読取認識=('数量', '単位値'), 目的依存=('状態:回答完了',)),
    )
    観測器 = (HDS観測器('ローカル構造資料', 取得, 観測を検証),); 検証器 = (HDS検証器('積検証', 草案を検証),); 政策 = HDS運用政策(初期作用予算=3, 予算増分=3)
    コア = HDS駆動コア(最大作用回数=40, 観測器=観測器, 検証器=検証器, 政策=政策); 主体 = HDS実行主体(作用群, 最大作用回数=40, 観測器=観測器, 検証器=検証器, 政策=政策)
    初期認識 = tuple(HDS認識項目(k, k, '整数値') for k in ('数量', '単位値'))
    初回 = コア.実行('入力資料から積を算出し、検証済み回答を返す', 要求状態=('独立済み', '回答完了'), 追加作用=作用群, 初期認識=初期認識, 要求認識=('数量', '単位値'))
    if 初回.終端 != HDS終端.採用: raise AssertionError(('初回未閉包', 初回.停止種別, 初回.理由, 初回.阻害履歴))
    初回呼出 = dict(呼出)
    現資料[0] = 原資料('2'); 変更1 = HDS作用結果(HDS作用状態.成立, 記憶更新=初回.状態.記憶.更新((現資料[0],))); 読戻し = 復元する(保存する(初回)); 同値更新 = 主体.再開(読戻し, 変更1)
    if 同値更新.終端 != HDS終端.採用: raise AssertionError(('同値更新未閉包', 同値更新.停止種別, 同値更新.理由, 同値更新.阻害履歴))
    同値呼出 = dict(呼出)
    データ['単位値'] = 9; 現資料[0] = 原資料('3'); 変更2 = HDS作用結果(HDS作用状態.成立, 記憶更新=同値更新.状態.記憶.更新((現資料[0],))); 値更新 = 主体.再開(同値更新, 変更2)
    if 値更新.終端 != HDS終端.採用: raise AssertionError(('値更新未閉包', 値更新.停止種別, 値更新.理由, 値更新.阻害履歴))
    assert 呼出['独立点検'] == 1; assert 初回.状態.成果辞書()['回答'] == 同値更新.状態.成果辞書()['回答'] == '積=21'; assert 値更新.状態.成果辞書()['回答'] == '積=27'; assert 呼出['数量計算'] == 呼出['草案検証'] == 3
    def 要約(r, count): return {'終端': r.終端.value, '回答': r.状態.成果辞書().get('回答'), '状態署名': r.状態.状態署名, '呼出累計': count, '計装': asdict(r.計装), '実行順': [x.作用ID for x in r.履歴], '検証票数': len(r.状態.検証票), '未解残差': sorted(r.状態.残差), '再評価待ち': sorted(r.状態.再評価待ち)}
    return {'区分': '人工入力・統合動作確認（一般性能ベンチではない）', '初回': 要約(初回, 初回呼出), '同値根拠更新': 要約(同値更新, 同値呼出), '実値更新': 要約(値更新, dict(呼出)), 'snapshot_json': 保存する(値更新)}


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--output', type=Path, help='実測JSONの出力先'); a = p.parse_args(); r = 実演する(); text = json.dumps(r, ensure_ascii=False, indent=2)
    if a.output: a.output.parent.mkdir(parents=True, exist_ok=True); a.output.write_text(text + '\n', encoding='utf-8')
    else: print(text)
