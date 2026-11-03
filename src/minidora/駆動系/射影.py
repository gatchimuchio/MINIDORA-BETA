"""駆動系の射影。局所結果を由来・依存とともに全体状態へ反映する。"""
from __future__ import annotations
from dataclasses import replace
from ..統合駆動_v2.依存 import HDS依存辺
from ..コア.状態操作 import 状態差を受理


def 作用結果を射影(前, 結果, 読取, 産出):
    """採用権限は元の状態更新則に置き、別の世界状態や検証器を新設しない。"""
    from ..HDS実行主体 import HDS作用状態
    証明 = dict(結果.検証依存)
    for k in 読取:
        現署名 = 前.ノード署名(k)
        if k in 証明 and 証明[k] != 現署名:
            raise ValueError("作用結果の読取証明が現在入力と不一致")
        証明[k] = 現署名
    辺 = set(結果.依存追加)
    if 結果.状態 == HDS作用状態.成立:
        # 同一作用内でread-modify-writeしたノードは、旧版の読取値を
        # 同時産出物の親へ自動依存として残さない。残すと作用自身の
        # 更新直後に出力状態が旧入力依存として失効し、後続再評価より
        # 同じ作用の再実行が先行する。読取証明は前状態署名として保持し、
        # 後続作用は更新後ノード署名から依存を形成する。
        原子的更新ノード = 読取 & 産出
        自動依存元 = 読取 - 原子的更新ノード
        辺 |= {HDS依存辺(a, b) for a in 自動依存元 for b in 産出 if a != b}
    結果 = replace(結果, 依存追加=tuple(sorted(辺)), 検証依存=tuple(sorted(証明.items())))
    新状態, 差 = 状態差を受理(前, 結果)
    return 新状態, 差, 結果


def 関係結果を射影(要求,結果,追加契約=()):
    """導出証明を照合して目的へ返す。候補名・得点・関係名接頭辞で保証を決めない。"""
    from .契約 import 関係変換結果,関係回答,関係出力束,関係保証,演算予算,関係資源超過
    from .取得 import 関係を取得
    from .構造 import 関係を束縛,関係を具体化
    from .契約 import 構造要求,構造変換結果,構造出力束
    if isinstance(要求,構造要求):
        if not isinstance(結果,構造変換結果) or 結果.要求署名!=要求.署名 or 結果.操作!=要求.操作:
            raise ValueError('構造射影の目的・操作が不一致')
        # 構造演算の結果は観測事実とは異なる型で返す。暗黙の世界知識化を禁止する。
        return 構造出力束(要求.ID,要求.署名,'成立' if 結果.完了 else '停止' if '明示停止' in 結果.理由 else '予算枯渇',結果 if 結果.完了 else None,結果.理由,結果.照合数)
    if not isinstance(結果,関係変換結果) or 結果.要求署名!=要求.署名:
        raise ValueError('射影先の目的・入力版が異なる')
    expected=関係を取得(要求,追加契約)
    if 結果.取得!=expected: raise ValueError('取得元・使用契約が変更された')
    if not 結果.完了:
        status='停止' if 結果.停止理由.startswith('明示停止:') else '予算枯渇'
        return 関係出力束(要求.ID,要求.署名,status,(),未充足=(結果.停止理由,),照合数=結果.照合数)
    演算資源=演算予算(要求.資源)
    演算資源.照合数=結果.照合数
    rules={x.ID:x for x in expected.変換}
    seeds={}
    for 観測証拠 in expected.証拠: seeds.setdefault(観測証拠.節.署名,[]).append(観測証拠)
    facts={key:xs[0].節 for key,xs in seeds.items()}
    records={}
    for item in 結果.導出:
        rule=rules.get(item.契約ID)
        if rule is None or rule.署名!=item.契約署名: raise ValueError('未知・変更された変換契約')
        if len(item.前提署名)!=len(rule.前提): raise ValueError('前提数不一致')
        env=dict(item.束縛)
        if len(env)!=len(item.束縛): raise ValueError('束縛鍵重複')
        if set(env)!=set(v for p in rule.前提 for v in p.変数群): raise ValueError('変数集合不一致')
        if any(v.変数 for v in env.values()): raise ValueError('導出が具体化されていない')
        if 関係を具体化(rule.結論,env)!=item.節: raise ValueError('結論の証明不一致')
        if tuple(関係を具体化(p,env).署名 for p in rule.前提)!=item.前提署名: raise ValueError('前提の証明不一致')
        if item.根拠!=rule.由来: raise ValueError('変換由来の偽装')
        facts[item.節.署名]=item.節
        records.setdefault(item.節.署名,[]).append(item)
    # 証明の循環だけで成立させない。別経路による根拠付き証明は残す。
    for key in records:
        records[key].sort(key=lambda x:(x.仮説,x.深さ,not bool(rules[x.契約ID].依存契約),x.契約ID))
    memo={}
    def prove(key,visiting=frozenset(),allow_hyp=False,blocked=frozenset()):
        演算資源.消費()
        if key in visiting or key in blocked: return None
        cache=(key,allow_hyp,blocked)
        if cache in memo: return memo[cache]
        if key in seeds:
            e=sorted(seeds[key],key=lambda x:x.ID)[0]
            return set(e.根拠),set(),{key},False
        for record in records.get(key,()):
            rule=rules[record.契約ID]
            own_hyp=rule.保証==関係保証.仮説
            if own_hyp and not allow_hyp: continue
            roots=set(rule.由来);used={rule.ID};導出追跡={key};hyp=own_hyp
            valid=True
            for parent in record.前提署名:
                p=prove(parent,visiting|{key},allow_hyp,blocked)
                if p is None: valid=False;break
                roots.update(p[0]);used.update(p[1]);導出追跡.update(p[2]);hyp=hyp or p[3]
            if valid:
                value=(roots,used,導出追跡,hyp)
                memo[cache]=value
                return value
        return None
    try:
        valid={k:prove(k) for k in facts}
        valid={k:v for k,v in valid.items() if v is not None}
        conflicts=frozenset(k for k in valid if replace(facts[k],肯定=not facts[k].肯定).署名 in valid)
        answers=[];hypotheses=[];goal_conflict=False;conflict_macros=set()
        for key,atom in sorted(facts.items()):
            演算資源.消費()
            env=関係を束縛(要求.問い,atom)
            if env is None: continue
            raw=valid.get(key)
            proof=prove(key,blocked=conflicts) or prove(key,allow_hyp=True,blocked=conflicts)
            if proof is None:
                if raw is not None and conflicts & raw[2]:
                    goal_conflict=True
                    conflict_macros.update(k for k in raw[1] if rules[k].依存契約)
                continue
            guarantee=関係保証.仮説 if proof[3] else 関係保証.導出 if proof[1] else seeds[key][0].保証
            answer=関係回答(atom,tuple(sorted(env.items())),tuple(sorted(proof[0])),tuple(sorted(proof[1])),guarantee)
            (hypotheses if proof[3] else answers).append(answer)
        if goal_conflict:
            return 関係出力束(要求.ID,要求.署名,'競合',(),tuple(hypotheses),
                ('回答の依存関係に支持・反証が併存',),演算資源.照合数,tuple(sorted(conflict_macros)))
        status='成立' if answers else '未観測'
        macros=tuple(sorted({r for a in answers for r in a.使用契約 if rules[r].依存契約}))
        return 関係出力束(要求.ID,要求.署名,status,tuple(answers),tuple(hypotheses),
                         () if answers else ('目的関係を成立させる証拠または条件が不足',),演算資源.照合数,macros)
    except 関係資源超過:
        return 関係出力束(要求.ID,要求.署名,'予算枯渇',(),未充足=('導出検証の共有照合予算',),照合数=演算資源.照合数)
