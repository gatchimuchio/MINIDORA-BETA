"""実データ型・生成器・状態更新境界で検証する。通常循環の代用品は置かない。"""
import unittest
from dataclasses import replace
from minidora.HDS実行主体 import HDS実行状態, HDS作用結果, HDS作用状態, HDS関数作用
from minidora.統合駆動_v2.記憶 import HDS資料, HDS記憶
from minidora.統合駆動_v2.認識 import HDS認識項目, 認識区分
from minidora.統合駆動_v2.意味構成 import HDS命題, HDS関係規則, 関係仮説構成作用, _ID
from minidora.統合駆動_v2.状態更新 import 状態更新, 有効認識
from minidora.統合駆動_v2.目的保持 import 目的を観測, 目的進展を判定, 作用が目的経路に属する


def 仮説入力():
    raw = HDS資料('実験記録', 'v1', '観測値と試験用の明示規則', '試験生成データ')
    wet = HDS認識項目('湿潤', '地面', '濡れ', True, 認識区分.確定,
                      (raw.出典(),), 検証契約='試験/実測')
    rain = HDS命題('空', '降雨', True)
    sprinkler = HDS命題('散水器', '作動', True)
    observed = HDS命題('地面', '濡れ', True)
    rules = (HDS関係規則('雨規則', (rain,), observed, (raw.出典(),), 排他群='説明'),
             HDS関係規則('散水規則', (sprinkler,), observed, (raw.出典(),), 排他群='説明'))
    target = _ID(rain, (wet,))
    state = HDS実行状態(目的=('地面の濡れの原因を識別する',), 要求認識=frozenset({target}),
                       認識=(wet,), 記憶=HDS記憶((raw,)))
    return state, rules, target


class 目的証拠試験(unittest.TestCase):
    def test_二段仮説で目的を失わない(self):
        state, rules, target = 仮説入力()
        action = 関係仮説構成作用(rules, 128)
        first = action.機会(state)
        self.assertTrue(作用が目的経路に属する(state, first, 認識有効判定=有効認識))
        prepared, _ = 状態更新(state, action.実行(state))
        self.assertFalse(prepared.仮説)
        second = action.機会(prepared)
        self.assertIsNotNone(second)
        self.assertTrue(作用が目的経路に属する(prepared, second, 認識有効判定=有効認識))
        reflected, _ = 状態更新(prepared, action.実行(prepared))
        self.assertEqual(len(reflected.仮説), 2)
        self.assertFalse(有効認識(reflected, target))
        self.assertFalse(reflected.閉包済み)

    def test_仮説生成は同一入力で再利用し入力変更では再利用しない(self):
        state, rules, _ = 仮説入力()
        action = 関係仮説構成作用(rules, 128)
        action.機会(state)
        result = action.実行(state)
        self.assertEqual(action.生成実行数, 1)
        next_state, _ = 状態更新(state, result)
        action.機会(next_state)
        self.assertEqual(action.生成実行数, 2)

    def test_進展は残差件数でなく同一性を見る(self):
        state = HDS実行状態(目的=('回答する',), 残差=frozenset({'未確認A'}))
        after = replace(state, 残差=frozenset({'追加確認B', '追加確認C'}))
        a, b = 目的を観測(state, 有効認識), 目的を観測(after, 有効認識)
        ok, _ = 目的進展を判定(前観測=a, 後観測=b, 最良直接尺度=a.直接尺度)
        self.assertTrue(ok)

    def test_目的署名が途中変更を拒否する(self):
        s = HDS実行状態(目的=('元の目的',))
        original = 目的を観測(s, 有効認識)
        with self.assertRaises(ValueError):
            目的を観測(replace(s, 目的=('別目的',)), 有効認識, 契約署名=original.契約署名)

    def test_中間証拠は延長し単なる反復は延長しない(self):
        s = HDS実行状態(目的=('回答する',), 要求状態=frozenset({'回答'}))
        necessary = frozenset({'成果:数量', '状態:回答'})
        prepared = replace(s, 成果=(('数量', 3),))
        a = 目的を観測(s, 有効認識, 必要ノード=necessary)
        b = 目的を観測(prepared, 有効認識, 必要ノード=necessary)
        ledger = set()
        ok, _ = 目的進展を判定(前観測=a, 後観測=b, 最良直接尺度=a.直接尺度, 進展台帳=ledger)
        self.assertTrue(ok)
        ok, _ = 目的進展を判定(前観測=a, 後観測=b, 最良直接尺度=a.直接尺度, 進展台帳=ledger)
        self.assertFalse(ok, '同じ中間成果の作り直しを新しい進展にしない')
        counter = replace(s, 主体状態=(('counter', 1),))
        c = 目的を観測(counter, 有効認識, 必要ノード=necessary)
        ok, _ = 目的進展を判定(前観測=a, 後観測=c, 最良直接尺度=a.直接尺度)
        self.assertFalse(ok)

    def test_欠落成果では意味署名を呼ばない(self):
        called = []
        def signer(state):
            called.append(True)
            return str(state.成果辞書()['未取得'])
        a = HDS関数作用('消費', lambda s: HDS作用結果(HDS作用状態.成立),
                       読取成果=('未取得',), 意味入力署名=signer)
        self.assertIsNotNone(a.機会(HDS実行状態()))
        self.assertFalse(called)

if __name__ == '__main__': unittest.main()
