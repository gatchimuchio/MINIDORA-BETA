from __future__ import annotations

from dataclasses import asdict
from hashlib import sha256
import json
from typing import Iterable

from .HDS構文化記録_v1_1 import HDS失敗署名候補, HDS失敗署名状態
from .HDS構文化記録_v1_2 import (
    HDS失敗観測,
    HDS失敗署名BankSnapshot,
    HDS失敗署名記録,
    HDS改善対象,
    HDS抽出規則改善候補,
)


def _unique(values: Iterable[str]) -> tuple[str, ...]:
    out: list[str] = []
    seen: set[str] = set()
    for raw in values:
        value = " ".join(str(raw).split()).strip()
        if not value:
            continue
        key = value.casefold()
        if key in seen:
            continue
        seen.add(key)
        out.append(value)
    return tuple(out)


def _stable_id(prefix: str, *parts: str) -> str:
    digest = sha256("\n".join(parts).encode("utf-8")).hexdigest()[:16]
    return f"{prefix}:{digest}"


def _signature_key(候補: HDS失敗署名候補) -> tuple[str, str]:
    return 候補.失敗分類.strip(), " ".join(候補.構造原因.split()).strip()


def _改善対象(候補: HDS失敗署名記録) -> HDS改善対象:
    mapping = {
        "coordinate_unfixed": HDS改善対象.座標生成規則,
        "closure_failure": HDS改善対象.座標生成規則,
        '関係_failure': HDS改善対象.作用素集合,
        '意味_loss_failure': HDS改善対象.保持構造,
    }
    return mapping.get(候補.失敗分類, HDS改善対象.Checklist)


def _改善提案(record: HDS失敗署名記録, target: HDS改善対象) -> str:
    if target == HDS改善対象.座標生成規則:
        return "同型失敗が反復した座標・端点・閉包条件について、既存正例を壊さない抽出規則候補を追加する"
    if target == HDS改善対象.作用素集合:
        return "同型の関係・遷移失敗を表現できる作用素または関係抽出候補を追加し、有向性と条件を保持する"
    if target == HDS改善対象.保持構造:
        return '意味損失が反復する構造を不可逆に捨てず、残差・由来・再開放条件として保持する'
    if target == HDS改善対象.Domain適合器:
        return '領域固有の反復失敗をDomain 適合器候補へ局所化し、共通構文化器へ無条件に混入しない'
    if target == HDS改善対象.Framework射影:
        return 'Framework 射影で失われる関係を明示し、別射影または留保経路を追加する'
    return "反復失敗から監査質問・必要証拠・停止/回復規則をChecklist候補として更新する"


def _tuple_fields(payload: dict[str, object], names: tuple[str, ...]) -> dict[str, object]:
    out = dict(payload)
    for name in names:
        if name in out and not isinstance(out[name], tuple):
            out[name] = tuple(out[name] or ())
    return out


class HDS失敗署名Bank:
    'Failure Signatureを明示的に蓄積する公開Bank。\n\n    global singletonを持たない。Bankを使う呼出側が明示的に同一instanceまたはSnapshotを渡す。\n    構文化器実装への自動適用は行わない。\n    '

    版 = "v1.2"
    昇格最小独立Run数 = 2

    def __init__(self, snapshot: HDS失敗署名BankSnapshot | None = None) -> None:
        snapshot = snapshot or HDS失敗署名BankSnapshot(self.版, 0)
        self._observations: dict[str, HDS失敗観測] = {item.観測ID: item for item in snapshot.観測履歴}
        self._records: dict[tuple[str, str], HDS失敗署名記録] = {
            (item.失敗分類, item.構造原因): item for item in snapshot.署名
        }
        self._improvements: dict[str, HDS抽出規則改善候補] = {item.候補ID: item for item in snapshot.改善候補}

    def 観測(self, candidates: Iterable[HDS失敗署名候補], *, Run参照: str) -> HDS失敗署名BankSnapshot:
        run = " ".join(str(Run参照).split()).strip()
        if not run:
            raise ValueError("Run参照は空にできません")

        for 候補 in candidates:
            key = _signature_key(候補)
            obs_id = _stable_id("obs", run, 候補.署名ID, key[0], key[1], 候補.症状)
            if obs_id in self._observations:
                continue
            observation = HDS失敗観測(
                obs_id,
                run,
                候補.署名ID,
                候補.失敗分類,
                候補.症状,
                候補.構造原因,
                _unique(候補.起動条件),
                _unique(候補.影響範囲),
            )
            self._observations[obs_id] = observation
            self._records[key] = self._更新記録(候補)

        self._改善候補再生成()
        return self.snapshot()

    def _更新記録(self, 候補: HDS失敗署名候補) -> HDS失敗署名記録:
        key = _signature_key(候補)
        previous = self._records.get(key)
        related = [obs for obs in self._observations.values() if (obs.失敗分類, obs.構造原因) == key]
        runs = _unique(obs.Run参照 for obs in related)
        symptoms = _unique(obs.症状 for obs in related)
        候補_ids = _unique(obs.候補署名ID for obs in related)
        condition_sets = [set(obs.起動条件) for obs in related]
        common = tuple(sorted(set.intersection(*condition_sets))) if condition_sets else ()
        all_conditions = set().union(*(set(obs.起動条件) for obs in related)) if related else set()
        local = tuple(sorted(all_conditions - set(common)))
        状態 = HDS失敗署名状態.有効 if len(runs) >= self.昇格最小独立Run数 else HDS失敗署名状態.候補
        signature_id = previous.署名ID if previous else _stable_id("signature", key[0], key[1])

        return HDS失敗署名記録(
            signature_id,
            候補.失敗分類,
            候補.構造原因,
            common,
            local,
            symptoms,
            runs,
            _unique([*(previous.影響範囲 if previous else ()), *候補.影響範囲]),
            _unique([*(previous.非影響範囲 if previous else ()), *候補.非影響範囲]),
            _unique([*(previous.違反前提 if previous else ()), *候補.違反前提]),
            _unique([*(previous.回復 if previous else ()), *候補.回復]),
            _unique([*(previous.次探索軸 if previous else ()), *候補.次探索軸]),
            _unique([*(previous.再利用チェック if previous else ()), *候補.再利用チェック]),
            len(related),
            len(runs),
            状態,
            候補_ids,
        )

    def _改善候補再生成(self) -> None:
        improvements: dict[str, HDS抽出規則改善候補] = {}
        for record in self._records.values():
            if record.状態 != HDS失敗署名状態.有効:
                continue
            target = _改善対象(record)
            候補_id = _stable_id("improvement", record.署名ID, target.value)
            improvements[候補_id] = HDS抽出規則改善候補(
                候補_id,
                target,
                (record.署名ID,),
                record.構造原因,
                _改善提案(record, target),
                _unique([*record.症状履歴, *record.共通起動条件, *record.局所起動条件]),
                record.反復回数,
                record.独立Run数,
                HDS失敗署名状態.候補,
                True,
                True,
            )
        self._improvements = improvements

    def snapshot(self) -> HDS失敗署名BankSnapshot:
        observations = tuple(sorted(self._observations.values(), key=lambda item: item.観測ID))
        records = tuple(sorted(self._records.values(), key=lambda item: item.署名ID))
        improvements = tuple(sorted(self._improvements.values(), key=lambda item: item.候補ID))
        return HDS失敗署名BankSnapshot(
            self.版,
            len(observations),
            observations,
            records,
            improvements,
            True,
            True,
        )

    def JSON化(self) -> str:
        return json.dumps(asdict(self.snapshot()), ensure_ascii=False, sort_keys=True, separators=(",", ":"), default=str)

    @classmethod
    def JSONから復元(cls, payload: str) -> "HDS失敗署名Bank":
        raw = json.loads(payload)
        observations = tuple(
            HDS失敗観測(
                **_tuple_fields(item, ("起動条件", "適用範囲"))
            )
            for item in raw.get("観測履歴", ())
        )
        records = tuple(
            HDS失敗署名記録(
                **{
                    **_tuple_fields(
                        item,
                        (
                            "共通起動条件", "局所起動条件", "症状履歴", "Run履歴", "影響範囲",
                            "非影響範囲", "違反前提", "回復", "次探索軸", "再利用チェック", "由来候補ID",
                        ),
                    ),
                    "状態": HDS失敗署名状態(item.get("状態", HDS失敗署名状態.候補)),
                }
            )
            for item in raw.get("署名", ())
        )
        improvements = tuple(
            HDS抽出規則改善候補(
                **{
                    **_tuple_fields(item, ("失敗署名参照", "根拠", "昇格条件", "再開放条件")),
                    "改善対象": HDS改善対象(item["改善対象"]),
                    "状態": HDS失敗署名状態(item.get("状態", HDS失敗署名状態.候補)),
                }
            )
            for item in raw.get("改善候補", ())
        )
        snapshot = HDS失敗署名BankSnapshot(
            str(raw.get("版", cls.版)),
            int(raw.get("観測数", len(observations))),
            observations,
            records,
            improvements,
            bool(raw.get("旧記録保持", True)),
            bool(raw.get("自動自己改変禁止", True)),
        )
        return cls(snapshot)


__all__ = ["HDS失敗署名Bank"]
