"""与えられた資料に対する三種類の領域横断実演。一般能力ベンチではない。

資料読取・形式検査・計算/関係比較・報告草案・原資料検証を別作用として登録し、
呼出順はHDSが要求状態と前提から構成する。外部ネットワークや書込作用は持たない。
"""
from __future__ import annotations
import argparse
import csv
import io
import json
from dataclasses import asdict
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from minidora.HDS実行主体 import HDS実行主体, HDS実行状態, HDS関数作用, HDS作用結果, HDS作用状態, HDS終端
from minidora.統合駆動_v2 import HDS資料, HDS記憶, HDS認識項目, HDS観測器, HDS観測値, HDS草案, HDS検証器, HDS運用政策, 保存する, 復元する


方式群 = ("数量集計", "日程重複", "資料照合")


def 表を読む(本文: str, 必須列: tuple[str, ...]):
    reader = csv.DictReader(io.StringIO(本文))
    if reader.fieldnames is None or len(set(reader.fieldnames)) != len(reader.fieldnames):
        raise ValueError("列名が欠落または重複")
    if not set(必須列) <= set(reader.fieldnames):
        raise ValueError("必要列が不足")
    rows = tuple(tuple(sorted(r.items())) for r in reader)
    if len(rows) > 1024 or not rows:
        raise ValueError("行数が対応範囲外")
    if any(any(k is None or v is None for k, v in r) for r in rows):
        raise ValueError("行の列数が不一致")
    return rows


def 処理する(方式, 行群):
    rows = tuple(dict(r) for r in 行群)
    if 方式 == "数量集計":
        values = [(int(r["数量"]), int(r["単価"])) for r in rows]
        if any(a < 0 or b < 0 for a, b in values):
            raise ValueError("この契約では数量と単価は非負整数")
        return (("合計", sum(a*b for a, b in values)), ("行数", len(rows)))
    if 方式 == "日程重複":
        intervals = [(r["名称"], int(r["開始"]), int(r["終了"])) for r in rows]
        if any(a >= b for _, a, b in intervals):
            raise ValueError("開始は終了より前である必要がある")
        conflicts = tuple(sorted(tuple(sorted((n, m))) for i, (n, a, b) in enumerate(intervals)
                                 for m, c, d in intervals[i+1:] if max(a, c) < min(b, d)))
        return (("重複", conflicts), ("行数", len(rows)))
    if 方式 == "資料照合":
        groups = {}
        for r in rows:
            groups.setdefault(r["資料"], set()).add(r["項目"])
        if len(groups) != 2:
            raise ValueError("比較は二つの資料集合を必要とする")
        left, right = sorted(groups)
        return (("左のみ", tuple(sorted(groups[left]-groups[right]))),
                ("右のみ", tuple(sorted(groups[right]-groups[left]))),
                ("共通", tuple(sorted(groups[left]&groups[right]))))
    raise ValueError("未定義方式")


def 構成する(資料: HDS資料, 方式: str):
    if 方式 not in 方式群:
        raise ValueError("未定義方式")
    columns = {"数量集計": ("数量", "単価"), "日程重複": ("名称", "開始", "終了"), "資料照合": ("資料", "項目")}[方式]
    calls = {"観測": 0, "構造化": 0, "処理": 0, "草案": 0, "検証": 0, "独立": 0}

    def 取得(req, s):
        calls["観測"] += 1
        doc = s.記憶.正本辞書()[資料.ID]
        return HDS観測値(doc.本文, (doc.出典(),), req.対象, req.関係, req.範囲, req.時点, (doc,))

    def 検証(req, v):
        return len(v.資料群) == 1 and v.値 == v.資料群[0].本文 and req.対象 == 資料.ID

    def parse(s):
        calls["構造化"] += 1
        rows = 表を読む(s.認識辞書()["入力本文"].値, columns)
        return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"表構造化済み"}), 成果=(("行群", rows),))

    def compute(s):
        calls["処理"] += 1
        結果 = 処理する(方式, s.成果辞書()["行群"])
        return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"処理済み"}), 成果=(("比較結果", 結果),))

    def draft(s):
        calls["草案"] += 1
        結果 = s.成果辞書()["比較結果"]
        doc = s.記憶.正本辞書()[資料.ID]
        text = json.dumps({"方式": 方式, "結果": dict(結果), "出典": {"ID": doc.ID, "版": doc.版, "SHA256": doc.内容署名}}, ensure_ascii=False, sort_keys=True)
        dep = tuple((k, s.ノード署名(k)) for k in ("認識:入力本文", "成果:比較結果"))
        return HDS作用結果(HDS作用状態.成立, 草案更新=(HDS草案("報告", (("報告", text),), ("原資料再検証",), dep,
                                                追加状態=frozenset({"報告検証済み"})),))

    def validate(s, d):
        calls["検証"] += 1
        doc = s.記憶.正本辞書()[資料.ID]
        expect = dict(処理する(方式, 表を読む(doc.本文, columns)))
        text = json.loads(dict(d.成果)["報告"])
        return text == json.loads(json.dumps({"方式": 方式, "結果": expect,
                         "出典": {"ID": doc.ID, "版": doc.版, "SHA256": doc.内容署名}}, ensure_ascii=False))

    def independent(s):
        calls["独立"] += 1
        return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"独立確認済み"}))

    actions = (
        HDS関数作用("報告形成", draft, 入力状態=("処理済み",), 読取認識=("入力本文",), 読取成果=("比較結果",), 目的依存=("状態:報告検証済み",)),
        HDS関数作用("関係処理", compute, 入力状態=("表構造化済み",), 出力状態=("処理済み",), 読取成果=("行群",), 純粋作用=True),
        HDS関数作用("構造化", parse, 出力状態=("表構造化済み",), 読取認識=("入力本文",), 純粋作用=True),
        HDS関数作用("独立確認", independent, 出力状態=("独立確認済み",), 入力署名=lambda s: "独立確認v1"),
    )
    主体 = HDS実行主体(actions, 最大作用回数=80, 政策=HDS運用政策(初期作用予算=4, 予算増分=4),
        観測器=(HDS観測器("ローカル原本", 取得, 検証),), 検証器=(HDS検証器("原資料再検証", validate),))
    initial = HDS実行状態(目的=(方式,), 要求状態=frozenset({"報告検証済み", "独立確認済み"}),
          要求認識=frozenset({"入力本文"}), 認識=(HDS認識項目("入力本文", 資料.ID, "CSV本文"),), 記憶=HDS記憶((資料,)))
    return 主体, initial, calls


def 実演する():
    fixtures = {
        "数量集計": "数量,単価\n3,7\n2,11\n",
        "日程重複": "名称,開始,終了\nA,9,11\nB,10,12\nC,12,13\n",
        "資料照合": "資料,項目\n甲,A\n甲,B\n乙,B\n乙,C\n",
    }
    reports = []
    for mode, text in fixtures.items():
        doc = HDS資料("人工CSV", "1", text, "同梱の人工入力")
        sub, 状態, calls = 構成する(doc, mode)
        r = sub.実行(状態)
        if r.終端 != HDS終端.採用:
            raise AssertionError((mode, r.終端, r.理由, r.阻害履歴))
        restored = 復元する(保存する(r))
        assert restored == r
        reports.append({"方式": mode, "終端": r.終端.value, "結果": json.loads(r.状態.成果辞書()["報告"]),
                        "実行順": [h.作用ID for h in r.履歴], "計装": asdict(r.計装), "呼出": calls})
    return {"区分": "人工資料による複数領域連結診断。フロンティアLLM比較ではない", "実演": reports}


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--output", type=Path)
    a = p.parse_args()
    結果 = json.dumps(実演する(), ensure_ascii=False, indent=2)
    if a.output:
        a.output.parent.mkdir(parents=True, exist_ok=True)
        a.output.write_text(結果+"\n", encoding="utf-8")
    else:
        print(結果)
