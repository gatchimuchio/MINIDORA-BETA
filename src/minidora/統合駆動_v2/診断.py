"""未分類失敗を観測し、安全な代替経路を残す。推定原因を事実へ昇格しない。"""
from __future__ import annotations
from dataclasses import dataclass
from .政策 import HDS阻害,停止理由
from .値 import 文字,文字列組

@dataclass(frozen=True,slots=True)
class HDS失敗診断:
    作用ID:str;例外型:str;観測記述:str;入力署名:str;原因確定:bool=False;再実行安全:bool=False;不足候補:tuple[str,...]=()
    def __post_init__(self):
        for n in ("作用ID","例外型","観測記述","入力署名"):文字(getattr(self,n),n)
        if type(self.原因確定) is not bool or type(self.再実行安全) is not bool:raise TypeError("診断状態フラグはbool")
        文字列組(self.不足候補)

def 例外を診断(例外,作用,機会,状態):
    spec=getattr(作用,"計画仕様",None);safe=bool(spec is not None and getattr(spec,"純粋",False));detail=f"{type(例外).__name__}: {例外}";candidates=()
    if isinstance(例外,KeyError) and len(例外.args)==1 and isinstance(例外.args[0],str):
        key=例外.args[0];candidates=tuple(n for n,d in (("認識:",状態.認識辞書()),("成果:",状態.成果辞書())) if key not in d);candidates=tuple(n+key for n in candidates)
    if isinstance(例外,PermissionError):reason,safe=停止理由.権限制約,False
    elif isinstance(例外,(TypeError,ValueError,AssertionError)):reason,safe=停止理由.契約違反,False
    else:reason=停止理由.未知失敗
    diag=HDS失敗診断(機会.作用ID,type(例外).__name__,detail,機会.作用入力署名,False,safe,candidates);blocker=HDS阻害(reason,機会.作用ID,detail+("。原因未確定。純粋作用の代替手段を再構成する" if safe else ""),safe);return diag,blocker
