from __future__ import annotations

from typing import Iterable

from .HDS中間表現 import HDSIR
from .HDS観測計画 import HDS参照観測要求, HDS参照観測要求群, HDS追加観測要求群
from .HDS参照 import HDS参照予算選択, HDS参照検索, _候補被覆, _query_pools, _round_robin, _縮退仕様, _仕様化, _記録統合
from .参照 import 参照供給器, 参照記録, 参照取得診断

def _候補ラベル群(record: 参照記録) -> frozenset[str]:
    return frozenset(str(value) for key,value in record.条件 if str(key)=='hds_query_選択肢' and str(value))

def HDS追加参照統合上限(既存件数: int, 追加件数: int, 最大件数: int = 32) -> int:
    """追加観測で得た少数資料も捨てず、全体件数だけを上限内へ閉じる。"""
    for 名, 値 in (("既存件数",既存件数),("追加件数",追加件数),("最大件数",最大件数)):
        if type(値) is not int or 値 < 0:
            raise ValueError(名 + "は0以上の整数である必要がある")
    if 最大件数 == 0:
        return 0
    return min(最大件数, 既存件数 + 追加件数)

def HDS候補被覆優先統合(primary: Iterable[参照記録], extra: Iterable[参照記録], expected_labels: Iterable[str], limit: int) -> tuple[参照記録,...]:
    total_limit=max(0,int(limit))
    if total_limit<=0: return ()
    combined=[]; index_by_id={}
    for record in (*tuple(primary),*tuple(extra)):
        rid=str(record.識別子); existing=index_by_id.get(rid)
        if existing is not None: combined[existing]=_記録統合(combined[existing],record); continue
        index_by_id[rid]=len(combined); combined.append(record)
    selected=[]; selected_ids=set()
    for label in sorted({str(x) for x in expected_labels if str(x)}):
        候補=next((r for r in combined if str(r.識別子) not in selected_ids and label in _候補ラベル群(r)),None)
        if 候補 is None: continue
        selected.append(候補); selected_ids.add(str(候補.識別子))
        if len(selected)>=total_limit: return tuple(selected)
    for record in combined:
        rid=str(record.識別子)
        if rid in selected_ids: continue
        selected.append(record); selected_ids.add(rid)
        if len(selected)>=total_limit: break
    return tuple(selected)

def _要求群(ir: HDSIR, 観測要求: Iterable[HDS参照観測要求] | None) -> tuple[HDS参照観測要求,...]:
    return tuple(観測要求) if 観測要求 is not None else HDS参照観測要求群(ir)

def HDS参照検索強化(provider: 参照供給器, ir: HDSIR, *, 上限: int | None=None, 一問合せ上限: int | None=None,
             最大問合せ並列: int | None=None, 最大候補補完回数: int=1,
             観測要求: Iterable[HDS参照観測要求] | None=None,
             診断収集: list[参照取得診断] | None=None) -> tuple[参照記録,...]:
    予算=HDS参照予算選択(ir)
    total_limit=予算.取得上限 if 上限 is None else max(0,int(上限))
    per_query=予算.一問合せ上限 if 一問合せ上限 is None else max(1,int(一問合せ上限))
    parallel=予算.最大問合せ並列 if 最大問合せ並列 is None else max(1,int(最大問合せ並列))
    if total_limit<=0: return ()
    requests=_要求群(ir,観測要求)
    references=HDS参照検索(provider,ir,上限=total_limit,一問合せ上限=per_query,最大問合せ並列=parallel,観測要求=requests,診断収集=診断収集)
    expected={r.候補ラベル for r in requests if r.必須被覆 and r.候補ラベル is not None}
    if not expected: return references
    for _ in range(max(0,int(最大候補補完回数))):
        missing=expected-set(_候補被覆(references))
        if not missing: break
        specs=tuple(spec for spec in _縮退仕様(ir,観測要求=requests) if spec.候補 is not None and spec.候補 in missing)
        if not specs: break
        extra=_round_robin(_query_pools(provider,specs,per_query,max_parallel=parallel,診断収集=診断収集),total_limit)
        if not extra: break
        merged=HDS候補被覆優先統合(references,extra,expected,total_limit)
        before=tuple((r.識別子,r.条件) for r in references); after=tuple((r.識別子,r.条件) for r in merged)
        references=merged
        if after==before: break
    return references

def HDS追加参照検索(provider: 参照供給器, ir: HDSIR, *, 段階: int=1, 最大取得上限: int=32,
             観測要求: Iterable[HDS参照観測要求] | None=None,
             残差群: Iterable[str]=(), 診断収集: list[参照取得診断] | None=None) -> tuple[参照記録,...]:
    """追加Rは段階観測し、最終primary層だけ全観測面を用いた回復へ閉じる。"""
    予算=HDS参照予算選択(ir); factor=max(2,1+int(段階))
    total_limit=min(max(1,int(最大取得上限)),max(予算.取得上限,予算.取得上限*factor))
    per_query=min(total_limit,max(予算.一問合せ上限,予算.一問合せ上限*factor))
    requests=_要求群(ir,観測要求)
    planned=HDS追加観測要求群(requests,残差群=残差群,世代=max(1,int(段階)))
    if planned and all(request.段階 == "primary" for request in planned):
        return HDS参照検索強化(
            provider, ir,
            上限=total_limit,
            一問合せ上限=per_query,
            最大問合せ並列=予算.最大問合せ並列,
            最大候補補完回数=1,
            観測要求=requests,
            診断収集=診断収集,
        )
    specs=tuple(spec for request in planned if (spec := _仕様化(request)) is not None)
    if not specs:
        return ()
    return _round_robin(
        _query_pools(provider,specs,per_query,max_parallel=予算.最大問合せ並列,診断収集=診断収集),
        total_limit,
    )

__all__=["HDS候補被覆優先統合","HDS追加参照統合上限","HDS参照検索強化","HDS追加参照検索"]
