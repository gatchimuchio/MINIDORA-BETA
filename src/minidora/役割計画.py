"""複数の入力役割を持つ作用契約の後方計画。成果型は予定であり実行成功ではない。"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import dataclass
from typing import Callable
from .会話意味 import 意味目的, 意味指紋
from .能力合成 import 合成工程, 合成計画, 素材参照
from .製品版.型 import 能力結果
from .実行回復 import 回復規則

役割計画版 = 'MINIDORA-役割計画-v0.3'

@dataclass(frozen=True, slots=True)
class 役割作用:
    識別子: str
    能力: str
    成果種別: str
    必要役割: Callable[[dict], tuple[tuple[str, 意味目的], ...]]
    設定: Callable[[dict], dict]
    適用: Callable[[dict], bool]
    費用: int = 1
    外部読取: bool = False
    不成立条件: tuple[str, ...] = ()
    保持事項: tuple[str, ...] = ('対象', '単位', '条件', '由来')
    回復: tuple[回復規則, ...] = ()

    def 静的契約(self) -> dict:
        return {
            '識別子': self.識別子,
            '能力': self.能力,
            '成果種別': self.成果種別,
            '費用': self.費用,
            '外部読取': self.外部読取,
            '不成立条件': self.不成立条件,
            '保持事項': self.保持事項,
            '回復': tuple((r.失敗種別, r.対象, r.入力役割, r.対象作用) for r in self.回復),
        }

@dataclass(frozen=True, slots=True)
class 役割被覆項:
    """選択された意味目的が素材又は作用のどちらで閉じたかを監査する。"""
    目的鍵: str
    種別: str
    解決: str
    作用: str = ''
    能力: str = ''
    素材: str = ''
    入力役割: tuple[tuple[str, str], ...] = ()
    契約印: str = ''

@dataclass(frozen=True, slots=True)
class 役割計画結果:
    計画: 合成計画
    資料: dict[str, 能力結果]
    工程作用: tuple[tuple[str, str, str], ...]  # 工程ID・目的鍵・作用ID
    目的印: str
    費用: int
    展開数: int
    外部作用: tuple[str, ...]
    入力役割: tuple[tuple[str, tuple[tuple[str, 素材参照], ...]], ...] = ()
    要求被覆: tuple[役割被覆項, ...] = ()
    作用契約印: str = ''

@dataclass(frozen=True, slots=True)
class _経路:
    目的: 意味目的
    作用: 役割作用 | None
    子: tuple[tuple[str, '_経路'], ...]
    費用: int
    設定資料: dict | None = None
    契約印: str = ''

    def 署名(self):
        return (self.目的.鍵(), self.作用.識別子 if self.作用 else '素材',
                tuple((name,p.署名()) for name,p in self.子))

class 役割計画器:
    def __init__(self, 作用: tuple[役割作用, ...], 登録一覧, *, 最大深さ=12, 最大展開数=512):
        if type(作用) is not tuple or not 1 <= len(作用) <= 128:
            raise ValueError('作用契約の数・型')
        for n,limit in ((最大深さ,16),(最大展開数,4096)):
            if type(n) is not int or not 1 <= n <= limit: raise ValueError('探索上限不正')
        actual={r['名前']:r for r in 登録一覧}
        if len(actual)!=len(登録一覧): raise ValueError('登録能力の重複')
        seen=set()
        for r in 作用:
            if type(r) is not 役割作用 or r.識別子 in seen or r.能力 not in actual:
                raise ValueError('作用契約の重複・未登録能力')
            if any(type(x) is not str or not x or len(x)>128 for x in (r.識別子,r.能力,r.成果種別)):
                raise ValueError('作用契約の識別子・能力・成果種別')
            if not all(callable(x) for x in (r.必要役割,r.設定,r.適用)):
                raise ValueError('作用契約の関数境界')
            if type(r.費用) is not int or not 1 <= r.費用 <= 100:
                raise ValueError('作用費用は正の有界整数')
            if type(r.外部読取) is not bool or actual[r.能力]['外部読取'] != r.外部読取:
                raise ValueError('外部作用の契約不一致')
            for seq,name in ((r.不成立条件,'不成立条件'),(r.保持事項,'保持事項')):
                if type(seq) is not tuple or len(set(seq))!=len(seq) or any(type(x) is not str or not x or len(x)>128 for x in seq):
                    raise ValueError(name+'の型・重複・上限')
            if type(r.回復) is not tuple or any(type(x) is not 回復規則 for x in r.回復):
                raise ValueError('回復規則の型不正')
            if len({x.失敗種別 for x in r.回復}) != len(r.回復):
                raise ValueError('同じ失敗への回復規則が重複')
            for recovery in r.回復: recovery.検証()
            seen.add(r.識別子)
        if any(a not in seen for r in 作用 for recovery in r.回復 for a in recovery.対象作用):
            raise ValueError('回復先の作用が未登録')
        self.作用, self._上限 = 作用, (最大深さ, 最大展開数)
        self.登録印 = 意味指紋(list(登録一覧))
        self.契約印 = 意味指紋([r.静的契約() for r in 作用])

    def 計画する(self, 目的: 意味目的, 素材: dict[str, 能力結果], *, 禁止=(), 外部許可=False):
        if type(目的) is not 意味目的 or type(素材) is not dict or type(外部許可) is not bool:
            raise ValueError('計画要求型不正')
        if type(禁止) is not tuple or len(禁止)>64 or any(type(x) is not tuple or len(x)!=2 for x in 禁止):
            raise ValueError('禁止は目的鍵と作用IDの対')
        for pair in 禁止:
            if any(type(x) is not str or not x for x in pair) or pair[1] not in {r.識別子 for r in self.作用}:
                raise ValueError('禁止対象の型又は作用が不正')
        root=deepcopy(目的); root.鍵()
        banned=set(禁止); expanded=0; memo={}
        def solve(goal, stack):
            nonlocal expanded
            key=goal.鍵()
            if key in stack: raise ValueError('目的依存の循環')
            if len(stack)>=self._上限[0]: raise ValueError('役割計画の深さ上限')
            if key in memo: return memo[key]
            expanded+=1
            if expanded>self._上限[1]: raise ValueError('役割計画の展開上限')
            if goal.種別=='原資料':
                if set(goal.引数)!={'資料'} or goal.引数['資料'] not in 素材:
                    raise ValueError('必要資料がない')
                if not 素材[goal.引数['資料']].成立: raise ValueError('入力素材が未成立')
                return _経路(goal,None,(),0)
            paths=[]
            for rule in self.作用:
                if rule.成果種別!=goal.種別 or (key,rule.識別子) in banned: continue
                if rule.外部読取 and not 外部許可: continue
                applicable=rule.適用(deepcopy(goal.引数))
                if type(applicable) is not bool: raise ValueError('適用条件はbool')
                if not applicable: continue
                役割=rule.必要役割(deepcopy(goal.引数))
                if (type(役割) is not tuple or len(役割)>8 or len({n for n,_ in 役割})!=len(役割)
                        or any(type(item) is not tuple or len(item)!=2 or type(item[0]) is not str
                               or not item[0] or len(item[0])>128 or type(item[1]) is not 意味目的 for item in 役割)):
                    raise ValueError('入力役割の型・重複・上限')
                役割_keys=tuple((name,child.鍵()) for name,child in 役割)
                settings=rule.設定(deepcopy(goal.引数))
                if type(settings) is not dict: raise ValueError('作用設定は資料辞書')
                # 設定と入力役割をここで一度だけ実体化し、emit時に再評価しない。
                契約_seal=意味指紋({
                    '静的':rule.静的契約(), '目的':key, '入力役割':役割_keys, '設定':settings,
                })
                try:
                    children=tuple((name,solve(child,stack+(key,))) for name,child in 役割)
                except _経路なし:
                    continue
                cost=rule.費用+sum(c.費用 for _,c in children)
                paths.append(_経路(goal,rule,children,cost,deepcopy(settings),契約_seal))
            if not paths: raise _経路なし('目的に適合する作用経路がない:'+goal.種別)
            bestcost=min(p.費用 for p in paths)
            best={p.署名():p for p in paths if p.費用==bestcost}
            if len(best)!=1: raise ValueError('同順位の意味経路が複数ある')
            memo[key]=next(iter(best.values())); return memo[key]
        chosen=solve(root,())
        資料={}; steps=[]; 追跡=[]; external=[]; refs={}; 役割_record=[]; coverage=[]
        def emit(path):
            signature=path.署名()
            if signature in refs: return refs[signature]
            key=path.目的.鍵()
            if path.作用 is None:
                name=path.目的.引数['資料']; ident='素材:'+name
                資料[ident]=deepcopy(素材[name]); out=素材参照('入力',ident)
                coverage.append(役割被覆項(key,path.目的.種別,'素材',素材=name))
            else:
                children=tuple((name,emit(p)) for name,p in path.子)
                if len(steps)>=64: raise ValueError('合成工程数上限')
                sid=f'役割工程:{len(steps)+1:04d}'; inst='指示:'+sid; config='設定:'+sid
                資料[inst]=能力結果(True,'要求された成果を得る')
                settings=deepcopy(path.設定資料)
                if type(settings) is not dict: raise ValueError('作用設定の固定失敗')
                資料[config]=能力結果(True,'',データ=settings)
                steps.append(合成工程(sid,(path.作用.能力,),inst,tuple(r for _,r in children),config))
                追跡.append((sid,key,path.作用.識別子))
                役割_record.append((sid,children))
                if path.作用.外部読取: external.append(sid)
                役割_keys=tuple((name,p.目的.鍵()) for name,p in path.子)
                coverage.append(役割被覆項(key,path.目的.種別,'作用',path.作用.識別子,
                                             path.作用.能力,'',役割_keys,path.契約印))
                out=素材参照('工程',sid)
            refs[signature]=out; return out
        final=emit(chosen)
        if final.領域!='工程': raise ValueError('素材を実行成果として採用しない')

        by_key={}
        for item in coverage:
            if item.目的鍵 in by_key and by_key[item.目的鍵] != item:
                raise ValueError('同一目的の被覆が競合')
            by_key[item.目的鍵]=item
        root_key=root.鍵()
        if root_key not in by_key:
            raise ValueError('根目的が被覆されていない')
        for item in coverage:
            if item.解決=='作用':
                if not item.作用 or not item.契約印 or any(child not in by_key for _,child in item.入力役割):
                    raise ValueError('作用被覆の閉包不正')
            elif item.解決=='素材':
                if not item.素材 or item.作用 or item.契約印:
                    raise ValueError('素材被覆の閉包不正')
            else:
                raise ValueError('未知の被覆種別')
        input_ids={r.識別子 for step in steps for r in step.入力 if r.領域=='入力'}
        covered_inputs={'素材:'+x.素材 for x in coverage if x.解決=='素材'}
        if input_ids != covered_inputs:
            raise ValueError('計画入力と要求被覆が不一致')
        契約_seal=意味指紋({
            '静的契約印':self.契約印,
            '実体契約':tuple((x.目的鍵,x.契約印) for x in coverage if x.解決=='作用'),
        })
        return 役割計画結果(合成計画(tuple(steps),(final.識別子,)),資料,tuple(追跡),
                               root_key,chosen.費用,expanded,tuple(external),tuple(役割_record),
                               tuple(coverage),契約_seal)

class _経路なし(ValueError):
    pass
