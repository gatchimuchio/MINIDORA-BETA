from __future__ import annotations
from contextlib import contextmanager
from dataclasses import dataclass, field
from threading import RLock
from .型 import 参照資料

@dataclass(slots=True)
class 会話状態:
    セッションID: str
    履歴: list[tuple[str, str]] = field(default_factory=list)
    直前応答: str = ""
    直前参照: tuple[参照資料, ...] = ()
    直前経路: str = ""
    直前追跡ID: str = ""
    直前監査ハッシュ: str = ""
    現在話題: str = ""
    def 追加(self, 役割: str, text: str, limit: int = 24) -> None:
        self.履歴.append((役割,text))
        if len(self.履歴)>limit: del self.履歴[:-limit]

class 会話状態庫:
    def __init__(self)->None:
        self._items:dict[str,会話状態]={}; self._locks:dict[str,RLock]={}; self._master=RLock()
    def 取得(self,session_id:str)->会話状態:
        sid=session_id.strip() or "default"
        with self._master:
            self._locks.setdefault(sid,RLock())
            return self._items.setdefault(sid,会話状態(sid))
    @contextmanager
    def 排他(self,session_id:str):
        sid=session_id.strip() or "default"
        with self._master: lock=self._locks.setdefault(sid,RLock())
        with lock: yield self.取得(sid)
