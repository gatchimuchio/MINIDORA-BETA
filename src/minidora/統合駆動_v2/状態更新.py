"""通常循環の一括状態更新・意味差・失効伝播。外側の監督ではなく核の更新則。"""
from __future__ import annotations
from dataclasses import replace
from copy import deepcopy
from .値 import 署名
from .認識 import 認識区分, 認識差分
from .依存 import HDS依存辺, 下流集合, 上流集合


def 全依存(状態):
    辺 = set(状態.依存)
    for x in 状態.認識:
        for k in x.依存:
            if k != x.ID:
                辺.add(HDS依存辺("認識:" + k, "認識:" + x.ID))
        for e in (*x.根拠, *x.反証):
            辺.add(HDS依存辺("資料:" + e.資料ID, "認識:" + x.ID))
    for x in 状態.仮説:
        for k in (*x.依存, *(p.観測ID for p in x.予測)):
            辺.add(HDS依存辺("認識:" + k, "仮説:" + x.ID))
    for x in 状態.草案:
        for k, _ in x.依存署名:
            if k != "草案:" + x.ID:
                辺.add(HDS依存辺(k, "草案:" + x.ID))
    return tuple(sorted(辺))


def ノード値(状態, ノード):
    種, _, ID = ノード.partition(":")
    if 種 == "状態":
        return ("存在", ID in 状態.成立状態)
    群 = {"認識": 状態.認識, "仮説": 状態.仮説, "資料": 状態.記憶.正本,
          "草案": 状態.草案, "形成": 状態.形成関係, "枝": 状態.枝}.get(種)
    if 群 is not None:
        for x in 群:
            if x.ID == ID:
                return ("存在", x.意味署名 if 種 == "認識" else x.内容署名 if 種 == "草案" else x)
        return ("不存在",)
    if 種 in ("成果", "主体"):
        d = dict(状態.成果 if 種 == "成果" else 状態.主体状態)
        return ("存在", d[ID]) if ID in d else ("不存在",)
    raise ValueError("未知の依存ノード種別: " + 種)


def ノード署名(状態, ノード):
    辺 = 全依存(状態)
    関連 = {ノード} | set(上流集合({ノード}, 辺))
    接続 = tuple(e for e in 辺 if e.前提 in 関連 and e.後続 in 関連)
    値群 = tuple((n, ノード値(状態, n)) for n in sorted(関連))
    return 署名(("依存意味署名-v3", ノード, 値群, 接続))


def 有効認識(状態, ID: str, _訪問: frozenset[str] = frozenset()) -> bool:
    if ID in _訪問 or "認識:" + ID in 状態.再評価待ち:
        return False
    x = 状態.認識辞書().get(ID)
    if x is None or x.区分 != 認識区分.確定 or x.条件 or x.反証:
        return False
    正本 = 状態.記憶.正本辞書()
    for 根拠 in x.根拠:
        資料 = 正本.get(根拠.資料ID)
        if 資料 is None or 資料.版 != 根拠.版 or 資料.内容署名 != 根拠.内容署名:
            return False
    return all(有効認識(状態, k, _訪問 | {ID}) for k in x.依存)


def ノード有効(状態, ノード: str) -> bool:
    if ノード in 状態.再評価待ち:
        return False
    種, _, ID = ノード.partition(":")
    if 種 == "認識": return 有効認識(状態, ID)
    if 種 == "状態": return ID in 状態.成立状態
    if 種 == "仮説": return any(x.ID == ID and x.区分 not in (認識区分.失効, 認識区分.棄却) for x in 状態.仮説)
    if 種 == "草案": return any(x.ID == ID and x.区分 == "検証済み" for x in 状態.草案)
    if 種 == "形成": return any(x.ID == ID and x.使用可能 for x in 状態.形成関係)
    return ノード値(状態, ノード)[0] == "存在"


def 閉包可能(状態) -> bool:
    if 状態.未達状態 or 状態.残差: return False
    if any(not 有効認識(状態, k) for k in 状態.要求認識): return False
    目標 = {"状態:" + k for k in 状態.要求状態} | {"認識:" + k for k in 状態.要求認識}
    辺 = 全依存(状態)
    関連 = 目標 | set(上流集合(目標, 辺))
    if 関連 & 状態.再評価待ち or any(not ノード有効(状態, k) for k in 関連): return False
    入次数 = {k: 0 for k in 関連}; 隣接 = {k: set() for k in 関連}
    for e in 辺:
        if e.前提 in 関連 and e.後続 in 関連:
            入次数[e.後続] += 1; 隣接[e.前提].add(e.後続)
    待ち = [k for k,n in 入次数.items() if n==0]; 済=0
    while 待ち:
        n=待ち.pop(); 済+=1
        for child in 隣接[n]:
            入次数[child]-=1
            if 入次数[child]==0: 待ち.append(child)
    from .指示接続 import 指示閉包可能
    return 済 == len(関連) and 指示閉包可能(状態)


def _更新群(元, 差, 削除=()):
    d={x.ID:x for x in 元 if x.ID not in 削除}; d.update((x.ID,x) for x in 差)
    return tuple(d[k] for k in sorted(d))


def _変更鍵(a,b):
    return tuple(sorted(k for k in a.keys() | b.keys() if k not in a or k not in b or 署名(a[k]) != 署名(b[k])))


def 状態更新(前, 結果):
    from ..HDS実行主体 import HDS作用結果, HDS作用状態, HDS状態差
    if not isinstance(結果,HDS作用結果): raise TypeError("状態更新にはHDS作用結果が必要")
    if 結果.状態 != HDS作用状態.成立 and (結果.追加状態 or 結果.解消残差): raise ValueError("未成立の作用が成立状態・残差解消を主張している")
    証明=dict(結果.検証依存)
    for n,s in 証明.items():
        if ノード署名(前,n)!=s: raise ValueError("古い/不正な入力署名による再検証: "+n)
    成果=前.成果辞書()
    for k in 結果.成果削除: 成果.pop(k,None)
    成果.update(deepcopy(dict(結果.成果)))
    主体=前.主体辞書(); 主体.update(deepcopy(dict(結果.主体状態差分)))
    新認識=_更新群(前.認識,結果.認識更新,結果.認識削除)
    新辺=tuple(sorted((set(前.依存)-set(結果.依存削除))|set(結果.依存追加)))
    新=replace(前, 成立状態=(前.成立状態-結果.削除状態)|結果.追加状態, 残差=(前.残差-結果.解消残差)|結果.追加残差,
        成果=tuple(sorted(成果.items())), 主体状態=tuple(sorted(主体.items())), 認識=新認識, 依存=新辺,
        仮説=_更新群(前.仮説,結果.仮説更新), 観測要求=_更新群(前.観測要求,結果.観測要求追加),
        記憶=結果.記憶更新 if 結果.記憶更新 is not None else 前.記憶, 枝=_更新群(前.枝,結果.枝更新),
        草案=_更新群(前.草案,結果.草案更新), 形成関係=_更新群(前.形成関係,結果.形成更新),
        検証票=前.検証票+tuple(x for x in 結果.検証票追加 if x not in 前.検証票))
    直接差=認識差分(前.認識,新.認識); 変更資料=_変更鍵(前.記憶.正本辞書(),新.記憶.正本辞書()); 変更成果=_変更鍵(dict(前.成果),dict(新.成果))
    起点={"認識:"+x.ID for x in 直接差}|{"資料:"+k for k in 変更資料}|{"成果:"+k for k in 変更成果}|{"状態:"+k for k in 前.成立状態^新.成立状態}
    起点|={"主体:"+k for k in _変更鍵(dict(前.主体状態),dict(新.主体状態))}|{"枝:"+k for k in _変更鍵({x.ID:x for x in 前.枝},{x.ID:x for x in 新.枝})}
    起点|={"草案:"+k for k in _変更鍵({x.ID:x.内容署名 for x in 前.草案},{x.ID:x.内容署名 for x in 新.草案})}|{"仮説:"+k for k in _変更鍵({x.ID:x for x in 前.仮説},{x.ID:x for x in 新.仮説})}|{"形成:"+k for k in _変更鍵({x.ID:x for x in 前.形成関係},{x.ID:x for x in 新.形成関係})}
    前辺,後辺=set(全依存(前)),set(全依存(新)); 変更辺=前辺^後辺; 辺=tuple(sorted(前辺|後辺)); 影響=set(下流集合(起点,辺))|{e.後続 for e in 変更辺}
    再検証候補=({"認識:"+x.ID for x in 結果.認識更新}|{"成果:"+k for k,_ in 結果.成果}|{"状態:"+k for k in 結果.追加状態}|{"仮説:"+x.ID for x in 結果.仮説更新}|{"草案:"+x.ID for x in 結果.草案更新}|{"枝:"+x.ID for x in 結果.枝更新}|{"形成:"+x.ID for x in 結果.形成更新})
    再評価=set(前.再評価待ち); 失効=set(); 親={}
    for e in 後辺: 親.setdefault(e.後続,set()).add(e.前提)
    認識=新.認識辞書(); 仮説={x.ID:x for x in 新.仮説}; 草案={x.ID:x for x in 新.草案}; 形成={x.ID:x for x in 新.形成関係}; 枝={x.ID:x for x in 新.枝}; 成立=set(新.成立状態); 成果=dict(新.成果); 新直接認識={x.ID:x for x in 結果.認識更新}
    def 再検証可能(n):
        if n not in 再検証候補: return False
        必須親=親.get(n,set())
        if not 必須親: return True
        for p in 必須親:
            if p.startswith("資料:") and n.startswith("認識:"):
                x=新直接認識.get(n.split(":",1)[1])
                if x and any("資料:"+e.資料ID==p for e in (*x.根拠,*x.反証)):
                    正本=新.記憶.正本辞書(); 出典一致=all(e.資料ID in 正本 and 正本[e.資料ID].版==e.版 and 正本[e.資料ID].内容署名==e.内容署名 for e in (*x.根拠,*x.反証))
                    if 出典一致 and (x.区分!=認識区分.確定 or (x.検証契約 and 有効認識(replace(新,再評価待ち=frozenset()),x.ID))): continue
            暫定対象=n.startswith(("仮説:","枝:")) or (n.startswith("認識:") and 新直接認識.get(n.split(":",1)[1]) is not None and 新直接認識[n.split(":",1)[1]].区分!=認識区分.確定)
            if p in 起点 or p not in 証明 or 証明[p]!=ノード署名(新,p) or (not 暫定対象 and not ノード有効(新,p)): return False
        return True
    for n in sorted(影響 | (再検証候補 & 再評価)):
        if 再検証可能(n): 再評価.discard(n); continue
        種,_,ID=n.partition(":"); 存在=False
        if 種=="認識" and ID in 認識: 存在=True; 認識[ID]=認識[ID].失効させる() if 認識[ID].区分!=認識区分.失効 else 認識[ID]
        elif 種=="仮説" and ID in 仮説: 存在=True; 仮説[ID]=replace(仮説[ID],区分=認識区分.失効)
        elif 種=="成果" and ID in 成果: 存在=True; 成果.pop(ID)
        elif 種=="状態" and ID in 成立: 存在=True; 成立.discard(ID)
        elif 種=="草案" and ID in 草案: 存在=True; 草案[ID]=replace(草案[ID],区分="失効")
        elif 種=="枝" and ID in 枝: 存在=True; 枝[ID]=replace(枝[ID],認識=tuple(x.失効させる() if x.区分!=認識区分.失効 else x for x in 枝[ID].認識))
        elif 種=="形成" and ID in 形成: 存在=True; 形成[ID]=形成[ID].再評価へ戻す()
        if 存在 or n in 再評価:
            再評価.add(n)
            if n not in 前.再評価待ち: 失効.add(n)
    新=replace(新,認識=tuple(認識[k] for k in sorted(認識)),仮説=tuple(仮説[k] for k in sorted(仮説)),草案=tuple(草案[k] for k in sorted(草案)),枝=tuple(枝[k] for k in sorted(枝)),形成関係=tuple(形成[k] for k in sorted(形成)),成果=tuple(sorted(成果.items())),成立状態=frozenset(成立),再評価待ち=frozenset(再評価))
    最終認識差=認識差分(前.認識,新.認識); 変化ID={x.ID for x in 最終認識差}; 履歴=前.認識履歴+tuple(x for x in 前.認識 if x.ID in 変化ID)
    新=replace(新,認識履歴=履歴,版=前.版+(新.状態署名!=前.状態署名))
    差=HDS状態差(前.状態署名,新.状態署名,tuple(sorted(新.成立状態-前.成立状態)),tuple(sorted(前.成立状態-新.成立状態)),tuple(sorted(前.残差-新.残差)),tuple(sorted(新.残差-前.残差)),_変更鍵(dict(前.成果),dict(新.成果)),_変更鍵(dict(前.主体状態),dict(新.主体状態)),最終認識差,tuple(sorted(影響|起点)),tuple(sorted(失効)),tuple(sorted(変更辺)),変更資料)
    return 新,差
