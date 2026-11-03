"""目的を維持し、失敗署名から決定した局所回復方針だけを再計画する同期監督。"""
from __future__ import annotations
from dataclasses import dataclass
from copy import deepcopy
from .能力合成 import _結果辞書
from .実行回復 import 回復方針, 回復方針を決定, 失敗を分類
from .会話意味 import 意味指紋
from .要求境界契約 import 要求境界契約印

会話実行監督版 = 'MINIDORA-会話実行監督-v0.2'

@dataclass(frozen=True, slots=True)
class 失敗署名:
    種別: str
    目的印: str
    工程: str
    作用: str
    入力印: str
    理由: str
    再開放: tuple[str,str] | None = None
    分類: str = '未分類'
    発生目的: str = ''
    回復契約: str = ''
    回復方針: str = '停止'
    再試行番号: int = 0

@dataclass(frozen=True, slots=True)
class 監督結果:
    応答: object
    失敗: tuple[失敗署名,...]
    試行: tuple[dict,...]


def _被覆地図(plan):
    rows = getattr(plan, '要求被覆', ())
    if type(rows) is not tuple:
        raise ValueError('要求被覆の型不正')
    out = {}
    for row in rows:
        key = getattr(row, '目的鍵', None)
        if type(key) is not str or not key or key in out:
            raise ValueError('要求被覆の目的鍵が不正')
        out[key] = row
    return out


def _被覆子孫(plan, target: str) -> set[str]:
    by_key = _被覆地図(plan)
    if target not in by_key:
        raise ValueError('回復対象目的が要求被覆にない')
    seen = set()
    stack = [target]
    while stack:
        key = stack.pop()
        if key in seen:
            continue
        seen.add(key)
        row = by_key[key]
        if getattr(row, '解決', '') == '作用':
            for pair in getattr(row, '入力役割', ()):
                if type(pair) is not tuple or len(pair) != 2 or pair[1] not in by_key:
                    raise ValueError('作用被覆の子目的が閉じていない')
                stack.append(pair[1])
    return seen


class 会話実行監督:
    def __init__(self, 計画器, 統合, *, 最大試行=12):
        if type(最大試行) is not int or not 1<=最大試行<=16: raise ValueError('監督試行上限')
        self.計画器,self.統合,self.最大試行=計画器,統合,最大試行

    def _回復契約印(self) -> str:
        return 意味指紋(tuple(
            (rule.識別子, tuple((r.失敗種別, r.対象, r.入力役割, r.対象作用,
                               r.動作(), r.最大再試行) for r in rule.回復))
            for rule in self.計画器.作用
        ))

    def _局所再計画監査(self, previous, current, policy: 回復方針) -> int:
        """回復対象部分木以外の要求被覆を固定する。再試行は全計画を固定する。"""
        if type(policy) is not 回復方針 or not policy.自動実行:
            raise ValueError('自動回復方針が不正')
        if policy.動作 == '同一作用再試行':
            if (previous.要求被覆 != current.要求被覆
                    or previous.作用契約印 != current.作用契約印
                    or previous.工程作用 != current.工程作用):
                raise ValueError('同一作用再試行で計画が変わった')
            return len(previous.要求被覆)
        target = policy.対象目的
        old_map, new_map = _被覆地図(previous), _被覆地図(current)
        old_範囲 = _被覆子孫(previous, target)
        new_範囲 = _被覆子孫(current, target)
        protected = {k:v for k,v in old_map.items() if k not in old_範囲}
        for key, row in protected.items():
            if new_map.get(key) != row:
                raise ValueError('局所再計画が回復対象外の被覆を変更した')
        for key, row in new_map.items():
            if key not in new_範囲 and old_map.get(key) != row:
                raise ValueError('局所再計画が回復対象外へ新しい経路を広げた')
        return len(protected)

    def _署名(self, plan, response, retries):
        run=response.実行
        if run is None or not run.履歴:
            kind='停止' if response.状態=='中止' else '実行環境'
            classification=失敗を分類(kind)
            policy=回復方針を決定(種別=kind,分類=classification,発生目的='',発生作用='',
                                 規則=None,再開放=None)
            return 失敗署名(kind,plan.目的印,'','','',response.理由,
                            分類=classification,回復方針=policy.動作),policy
        last=run.履歴[-1]
        kinds=last.理由.split(':',2)
        kind=('停止' if response.状態=='中止' else
              '実行環境' if last.状態=='失敗' else
              kinds[1] if len(kinds)==3 and kinds[0]=='会話失敗' else '能力不成立')
        classification=失敗を分類(kind)
        info={sid:(key,作用) for sid,key,作用 in plan.工程作用}
        if last.工程 not in info:
            policy=回復方針を決定(種別='実行環境',分類='実行環境',発生目的='',発生作用='',
                                 規則=None,再開放=None)
            return 失敗署名('実行環境',plan.目的印,last.工程,'',last.入力ハッシュ,
                            '実行履歴と計画の対応欠落',分類='実行環境',回復方針=policy.動作),policy
        key,作用=info[last.工程]
        covered={x.目的鍵:x for x in plan.要求被覆 if x.解決=='作用'}
        契約=covered.get(key)
        if 契約 is None or 契約.作用!=作用 or not 契約.契約印:
            policy=回復方針を決定(種別='実行環境',分類='実行環境',発生目的=key,発生作用=作用,
                                 規則=None,再開放=None)
            return 失敗署名('実行環境',plan.目的印,last.工程,作用,last.入力ハッシュ,
                            '実行履歴と作用契約被覆の対応欠落',分類='実行環境',発生目的=key,
                            回復方針=policy.動作),policy
        reopen=None; rule_match=None; 契約_name=''
        rule=next(r for r in self.計画器.作用 if r.識別子==作用)
        for recovery in rule.回復:
            if kind!=recovery.失敗種別: continue
            rule_match=recovery
            if recovery.動作()=='同一作用再試行':
                reopen=(key,作用)
            elif recovery.対象=='自己':
                reopen=(key,作用)
            else:
                役割=dict(dict(plan.入力役割).get(last.工程,()))
                parent=役割.get(recovery.入力役割)
                if parent is not None and parent.領域=='工程' and parent.識別子 in info:
                    pair=info[parent.識別子]
                    if pair[1] in recovery.対象作用: reopen=pair
            契約_name=':'.join((作用,recovery.失敗種別,recovery.対象,recovery.動作(),str(recovery.最大再試行)))
            break
        retry_key=(key,作用,kind)
        policy=回復方針を決定(種別=kind,分類=classification,発生目的=key,発生作用=作用,
            規則=rule_match,再開放=reopen,契約=契約_name,再試行済=retries.get(retry_key,0))
        exposed=(policy.対象目的,policy.対象作用) if policy.対象目的 and policy.対象作用 else None
        return 失敗署名(kind,plan.目的印,last.工程,作用,last.入力ハッシュ,last.理由,
                        exposed,classification,key,policy.契約,policy.動作,policy.再試行番号),policy

    def 実行(self, goal, materials, *, 原文, 外部許可=False, 停止要求=None, 要求起点=None):
        fixed_goal=deepcopy(goal); fixed_materials=deepcopy(materials)
        goal_seal=fixed_goal.鍵()
        material_seal=意味指紋({k:_結果辞書(v) for k,v in fixed_materials.items()})
        rules=self.計画器.作用
        static_契約=getattr(self.計画器,'契約印','')
        if type(static_契約) is not str or not static_契約:
            raise ValueError('作用契約印がない')
        recovery_契約=self._回復契約印()
        if type(recovery_契約) is not str or not recovery_契約:
            raise ValueError('回復方針契約印がない')
        start=self.統合.起点()
        if 要求起点 is not None and start!=要求起点: raise ValueError("解釈中に会話状態が変わった")
        failures=[]; attempts=[]; banned=[]; seen=set(); observed_contracts={}; response=None
        retries={}; pending=None; previous_plan=None
        for _ in range(self.最大試行):
            self.統合._停止(停止要求)
            if start!=self.統合.起点(): raise ValueError('再計画中に採用状態が変わった')
            if 意味指紋(list(self.統合.能力一覧()))!=self.計画器.登録印:
                raise ValueError('作用能力の版・登録が変わった')
            if self.計画器.作用!=rules or getattr(self.計画器,'契約印','')!=static_契約:
                raise ValueError('回復中に作用契約が変わった')
            if self._回復契約印()!=recovery_契約:
                raise ValueError('回復中に回復方針契約が変わった')
            if (fixed_goal.鍵()!=goal_seal or
                    意味指紋({k:_結果辞書(v) for k,v in fixed_materials.items()})!=material_seal):
                raise ValueError('回復中に目的又は資料が変わった')
            try:
                if pending is not None and pending.動作=='同一作用再試行':
                    plan=deepcopy(previous_plan)
                else:
                    plan=self.計画器.計画する(fixed_goal,fixed_materials,禁止=tuple(banned),外部許可=外部許可)
            except ValueError as exc:
                if response is None: raise
                attempts.append({'状態':'計画保留','理由':str(exc),'目的印':goal_seal,
                                 '素材印':material_seal,'外部工程':(),'作用契約印':'','回復方針契約印':recovery_契約,'要求被覆印':'',
                                 '要求境界契約印':'','回復方針':pending.動作 if pending else '初回','再開放':()})
                break
            if plan.目的印!=goal_seal: raise ValueError('再計画が目的を変更した')
            if (type(plan.要求被覆) is not tuple or not plan.要求被覆
                    or goal_seal not in {x.目的鍵 for x in plan.要求被覆}
                    or type(plan.作用契約印) is not str or not plan.作用契約印):
                raise ValueError('再計画の要求被覆又は作用契約が閉じていない')
            coverage_seal=意味指紋(tuple((x.目的鍵,x.種別,x.解決,x.作用,x.素材,x.入力役割,x.契約印)
                                        for x in plan.要求被覆))
            # 同じ目的・同じ作用の役割/設定契約を横滑りさせない。
            # 既存v2の監査を先に維持し、その後で作用選択まで含む局所範囲を締める。
            for item in plan.要求被覆:
                if item.解決!='作用': continue
                pair=(item.目的鍵,item.作用)
                previous=observed_contracts.get(pair)
                if previous is not None and previous!=item.契約印:
                    raise ValueError('再計画で既存作用契約が変わった')
                observed_contracts[pair]=item.契約印
            fixed_coverage=0
            if pending is not None and previous_plan is not None:
                fixed_coverage=self._局所再計画監査(previous_plan,plan,pending)
            packed=self.統合.準備(plan.計画,plan.資料,依頼文=原文)
            if packed.起点!=start: raise ValueError('計画準備中に起点が変わった')
            境界_seal=要求境界契約印(原文=原文,計画印=packed.ハッシュ,
                要求被覆印=coverage_seal,目的印=plan.目的印,素材印=material_seal)
            response=self.統合.実行(packed,外部読取許可=外部許可,停止要求=停止要求)
            attempts.append({'計画印':packed.ハッシュ,'素材印':material_seal,'目的印':plan.目的印,'工程作用':plan.工程作用,
                '状態':response.状態,'外部工程':plan.外部作用,'作用契約印':plan.作用契約印,
                '回復方針契約印':recovery_契約,'要求被覆印':coverage_seal,'要求被覆数':len(plan.要求被覆),
                '要求境界契約印':境界_seal,
                '回復方針':pending.動作 if pending else '初回',
                '再開放':(pending.対象目的,pending.対象作用) if pending else (),
                '固定被覆数':fixed_coverage,
                '実行印':response.実行.ルートハッシュ if response.実行 else '',
                '取得報告':[v.データ for _,v in response.実行.中間結果
                    if v.データ.get('種別')=='取得報告'] if response.実行 else []})
            if response.成立: return 監督結果(response,tuple(failures),tuple(attempts))
            if self.統合.起点()!=start: raise ValueError('失敗した試行が採用状態を変更した')
            signature,policy=self._署名(plan,response,retries);failures.append(signature)
            if not policy.自動実行:
                break
            if policy.動作=='同一作用再試行':
                retry_key=(signature.発生目的,signature.作用,signature.種別)
                retries[retry_key]=policy.再試行番号
            else:
                seal=意味指紋({'目的':signature.発生目的,'作用':signature.作用,
                              '入力':signature.入力印,'種別':signature.種別,'方針':policy.動作,
                              '対象':(policy.対象目的,policy.対象作用)})
                if seal in seen or any(pair in banned for pair in policy.禁止): break
                seen.add(seal)
                banned.extend(policy.禁止)
            previous_plan=plan; pending=policy
        return 監督結果(response,tuple(failures),tuple(attempts))
