"""通常の成功履歴から経験を形成し、純粋作用に限り自動再実行で検証する。

副作用を持つ作用・署名不一致・再現不成立を成功則へ昇格しない。
単一トレースの暗記を汎用知識の獲得とは呼ばない。
"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import replace
from .値 import 署名
from .形成 import HDS経験, HDS形成採用状態, 経験から形成, 再実行で検証
from .状態更新 import 状態更新, 閉包可能, 有効認識
from .政策 import HDS阻害, 停止理由


def 自動形成文脈(状態, 作用群):
    specs = tuple(sorted((a.作用ID, a.計画仕様.版) for a in 作用群 if getattr(a, "計画仕様", None) is not None))
    return 状態.主体辞書().get("形成文脈署名") or 署名((状態.目的, 状態.要求状態, 状態.要求認識, specs))


class 自動経験形成作用:
    作用ID = "内的/経験形成"

    def __init__(self, 初期, 履歴, 作用群, 最終検証器=()):
        self.初期, self.履歴 = deepcopy(初期), tuple(履歴)
        self.作用 = {a.作用ID: a for a in 作用群}
        self.検証 = tuple(最終検証器)
        self.再現回数 = 0
        self.再現成功 = False

    def _経験(self, 状態):
        from ..HDS実行主体 import HDS作用状態
        履歴列 = tuple(x for x in self.履歴 if x.作用ID != "内的/目的検証")
        if not 履歴列 or any(x.作用ID not in self.作用 or x.作用状態 != HDS作用状態.成立 or x.阻害 for x in 履歴列):
            return None
        specs = [getattr(self.作用[x.作用ID], "計画仕様", None) for x in 履歴列]
        if any(s is None or not s.純粋 for s in specs):
            return None
        文脈署名 = 自動形成文脈(self.初期, tuple(self.作用.values()))
        根拠署名 = 署名((self.初期.状態署名, 状態.状態署名, 履歴列))
        return HDS経験("実経験:" + 根拠署名[:24], self.初期.成立状態, self.初期.要求状態,
                       tuple(x.作用ID for x in 履歴列), True, 根拠署名, 文脈署名,
                       作用契約=tuple(sorted({(s.作用ID, s.版) for s in specs})))

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        e = self._経験(状態)
        if e is None:
            return None
        formed = 経験から形成(e)
        existing = next((x for x in 状態.形成関係 if x.ID == formed.ID), None)
        if existing is not None and existing.採用状態 == HDS形成採用状態.棄却 and existing.作用契約 == formed.作用契約:
            return None
        if existing is not None and e.根拠署名 in existing.由来:
            return None
        pure = all(self.作用[k].計画仕様.純粋 for k in e.作用列)
        cost = 1 + (sum(self.作用[k].計画仕様.資源負荷 for k in e.作用列) + len(self.検証) if pure else 0)
        return HDS作用機会(self.作用ID, 署名(e), 資源負荷=cost, 種別="経験形成")

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態, HDS作用記録
        e = self._経験(状態)
        if e is None:
            raise ValueError("形成対象の完全な実経験がない")
        形成関係 = 経験から形成(e)
        old = next((x for x in 状態.形成関係 if x.ID == 形成関係.ID), None)
        if old:
            由来 = tuple(sorted(set(old.由来) | set(形成関係.由来)))
            if old.作用契約 == 形成関係.作用契約:
                形成関係 = replace(
                    形成関係,
                    由来=由来,
                    反例=old.反例,
                    検証契約=old.検証契約,
                    版=old.版 + 1,
                    未解決反例=old.未解決反例,
                    除外反例=old.除外反例,
                    採用状態=old.採用状態,
                    復帰検証契約=old.復帰検証契約,
                    審査履歴=old.審査履歴,
                )
            else:
                # 作用契約版が変わった場合は同一手順名でも新しい形成系譜として再検証する。
                形成関係 = replace(形成関係, 由来=由来, 版=old.版 + 1)
        pure = all(self.作用[k].計画仕様.純粋 for k in e.作用列)
        notes = ["実履歴から条件付き手順を形成"]
        if pure:
            current, failure = deepcopy(self.初期), ""
            try:
                for k in e.作用列:
                    a = self.作用[k]
                    入力複製 = deepcopy(current)
                    sig = 入力複製.状態署名
                    offer = a.機会(入力複製)
                    if offer is None or not offer.入力状態 <= current.成立状態 or any(not 有効認識(current, n) for n in offer.読取認識):
                        raise ValueError("再実行前提が不成立")
                    out = a.実行(入力複製)
                    self.再現回数 += 1
                    if 入力複製.状態署名 != sig or out.状態 != HDS作用状態.成立 or out.阻害 or out.停止要求:
                        raise ValueError("再実行が成立しないか入力を変更した")
                    from .依存 import HDS依存辺
                    read = {"状態:"+n for n in offer.入力状態} | {"認識:"+n for n in (*offer.読取認識, *offer.未確定読取)} | {"成果:"+n for n in offer.読取成果}
                    produced = {"認識:"+x.ID for x in out.認識更新} | {"成果:"+n for n, _ in out.成果} | {"状態:"+n for n in out.追加状態}
                    edges = set(out.依存追加) | {HDS依存辺(x,y) for x in read for y in produced if x != y}
                    out = replace(out, 依存追加=tuple(sorted(edges)), 検証依存=tuple(sorted({**dict(out.検証依存), **{n: current.ノード署名(n) for n in read}}.items())))
                    current, _ = 状態更新(current, out)
                if not 閉包可能(current):
                    raise ValueError("再実行で目的未達")
                def 結果署名(s):
                    return 署名((s.成果, s.主体状態, s.成立状態, s.残差,
                                 tuple((x.ID,x.意味署名) for x in s.認識), s.草案, s.記憶.正本署名))
                if 結果署名(current) != 結果署名(状態):
                    raise ValueError("再実行結果が元実測と不一致")
                for v in self.検証:
                    検証候補 = deepcopy(current)
                    before = 検証候補.状態署名
                    valid = v.検証(検証候補, None)
                    if valid is not True or 検証候補.状態署名 != before:
                        raise ValueError("再実行の最終検証が不成立")
                self.再現成功 = True
            except Exception as exc:
                failure = f"{type(exc).__name__}: {exc}"
            measured = replace(e, ID=e.ID + "/再現", 成功=not failure,
                               根拠署名=署名(("再実行", current.状態署名, self.再現回数, failure)),
                               失敗署名=署名(failure) if failure else "")
            形成関係 = 再実行で検証(形成関係, measured, "純粋作用の実再現-v3")
            notes.append("再実行検証成立" if not failure else "再実行検証不成立: " + failure)
        else:
            notes.append("副作用安全性の宣言がないため再実行しない。未検証形成として保持")
        return HDS作用結果(HDS作用状態.成立, 形成更新=(形成関係,), 理由=tuple(notes))
