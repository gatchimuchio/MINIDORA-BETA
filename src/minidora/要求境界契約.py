'模型核外周で、要求被覆と実行計画印を同一監査契約へ結合する。'
from __future__ import annotations
from dataclasses import fields, is_dataclass
from datetime import datetime
from enum import Enum
from hashlib import sha256
import json
import math

要求境界契約版 = 'MINIDORA-要求境界契約-v0.1'


def _正規値(value, depth=0):
    if depth > 32:
        raise ValueError('要求被覆の入れ子上限')
    if isinstance(value, Enum):
        return _正規値(value.value, depth+1)
    if value is None or type(value) in (str,int,bool):
        return value
    if type(value) is float:
        if not math.isfinite(value): raise ValueError('要求被覆に非有限float')
        return value
    if isinstance(value, datetime): return value.isoformat()
    if is_dataclass(value) and not isinstance(value,type):
        return {f.name:_正規値(getattr(value,f.name),depth+1) for f in fields(value)}
    if type(value) in (tuple,list): return [_正規値(v,depth+1) for v in value]
    if type(value) is dict and all(type(k) is str for k in value):
        return {k:_正規値(value[k],depth+1) for k in sorted(value)}
    raise ValueError('要求被覆に記録不能値')


def _印(value):
    raw=json.dumps(_正規値(value),ensure_ascii=True,sort_keys=True,
                   separators=(',',':'),allow_nan=False).encode('utf-8')
    if len(raw)>2_000_000: raise ValueError('要求被覆の記録上限')
    return sha256(raw).hexdigest()


def 被覆台帳印(rows) -> str:
    if type(rows) is not tuple or not rows:
        raise ValueError('要求被覆が空')
    return _印(rows)


def 要求境界契約印(*, 原文: str, 計画印: str, 要求被覆印: str,
               目的印: str='', 素材印: str='') -> str:
    if type(原文) is not str or not 原文.strip() or len(原文)>8192:
        raise ValueError('要求境界の原文不正')
    for name,value in (('計画印',計画印),('要求被覆印',要求被覆印),('目的印',目的印),('素材印',素材印)):
        if type(value) is not str or (name in ('計画印','要求被覆印') and not value):
            raise ValueError('要求境界の'+name+'不正')
    return _印({
        '原文印':sha256(原文.encode('utf-8')).hexdigest(),
        '計画印':計画印,
        '要求被覆印':要求被覆印,
        '目的印':目的印,
        '素材印':素材印,
    })
