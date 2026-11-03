from __future__ import annotations
from dataclasses import dataclass, asdict
from datetime import datetime, timezone
from hashlib import sha256
import json
import os
from pathlib import Path
from threading import RLock
from typing import Any
from uuid import uuid4

監査仕様版 = "MINIDORA-PRODUCT-GOVERNANCE-v1"
安全監査仕様版 = "MINIDORA-PRODUCT-GOVERNANCE-v2"


def _安全型を検査(v):
    if hasattr(v, "__dataclass_fields__"):
        return _安全型を検査(asdict(v))
    if hasattr(v, "辞書化"):
        return _安全型を検査(v.辞書化())
    if isinstance(v, dict):
        if any(type(k) is not str for k in v):
            raise TypeError("安全監査の辞書鍵は文字列")
        for x in v.values():
            _安全型を検査(x)
    elif isinstance(v, (list, tuple, set)):
        for x in v:
            _安全型を検査(x)
    elif not (isinstance(v, datetime) or type(v) in (str, int, float, bool) or v is None):
        raise TypeError("安全監査の値の型不正")

def _jsonable(v: Any) -> Any:
    if hasattr(v, "__dataclass_fields__"):
        return _jsonable(asdict(v))
    if hasattr(v, "辞書化"):
        return _jsonable(v.辞書化())
    if isinstance(v, datetime):
        return v.isoformat()
    if isinstance(v, dict):
        return {str(k): _jsonable(x) for k, x in v.items()}
    if isinstance(v, (list, tuple, set)):
        return [_jsonable(x) for x in v]
    if isinstance(v, (str, int, float, bool)) or v is None:
        return v
    return repr(v)

def _canonical(v: Any) -> str:
    return json.dumps(_jsonable(v), ensure_ascii=False, sort_keys=True, separators=(",", ":"))

def _hash(v: Any) -> str:
    return sha256(_canonical(v).encode("utf-8")).hexdigest()

@dataclass(frozen=True, slots=True)
class 監査イベント:
    番号: int
    段階: str
    モジュール: str
    版: str
    入力: Any
    出力: Any
    根拠: tuple[str, ...]
    前ハッシュ: str
    ハッシュ: str

@dataclass(frozen=True, slots=True)
class 監査記録:
    追跡ID: str
    セッションID: str
    開始時刻: datetime
    入力文: str
    経路: str
    イベント: tuple[監査イベント, ...]
    最終応答: str
    状態: str
    前応答ハッシュ: str
    ルートハッシュ: str
    仕様版: str = 監査仕様版

    def 辞書化(self) -> dict[str, Any]:
        return _jsonable(self)

class 監査セッション:
    def __init__(self, 台帳: "監査台帳", 入力文: str, セッションID: str, 前応答ハッシュ: str = "", *, 安全表現=None) -> None:
        if 安全表現 is not None and not callable(安全表現):
            raise TypeError("監査の安全表現はcallable")
        self._表現 = 安全表現 if 安全表現 is not None else lambda v: v
        self.仕様版 = 安全監査仕様版 if 安全表現 is not None else 監査仕様版
        self._台帳 = 台帳
        self.追跡ID = uuid4().hex
        self.セッションID = self._表現(セッションID)
        self.開始時刻 = datetime.now(timezone.utc)
        self.入力文 = self._表現(入力文)
        self.経路 = "未確定"
        self.前応答ハッシュ = 前応答ハッシュ
        self._events: list[監査イベント] = []
        self._prev = "0" * 64

    def 経路設定(self, 経路: str) -> None:
        self.経路 = self._表現(経路)

    def 記録(self, 段階: str, モジュール: str, 版: str, 入力: Any, 出力: Any, 根拠: tuple[str, ...] = ()) -> None:
        if self.仕様版 == 安全監査仕様版:
            _安全型を検査(入力)
            _安全型を検査(出力)
        段階, モジュール, 版 = (self._表現(v) for v in (段階, モジュール, 版))
        入力, 出力 = self._表現(_jsonable(入力)), self._表現(_jsonable(出力))
        根拠 = tuple(self._表現(v) for v in 根拠)
        n = len(self._events) + 1
        material = {
            "spec": self.仕様版, '追跡': self.追跡ID, "n": n,
            "stage": 段階, 'モジュール': モジュール, "version": 版,
            "input": _jsonable(入力), "output": _jsonable(出力),
            '証拠': list(根拠), "prev": self._prev,
        }
        h = _hash(material)
        self._events.append(監査イベント(n, 段階, モジュール, 版, _jsonable(入力), _jsonable(出力), tuple(根拠), self._prev, h))
        self._prev = h

    def 確定(self, 応答: str, 状態: str) -> 監査記録:
        応答, 状態 = self._表現(応答), self._表現(状態)
        root_material = {
            "spec": self.仕様版, '追跡': self.追跡ID, "session": self.セッションID,
            "started": self.開始時刻, "input": self.入力文, '経路': self.経路,
            "response": 応答, "status": 状態, "prev_response": self.前応答ハッシュ,
            "last_event": self._prev,
        }
        record = 監査記録(
            self.追跡ID, self.セッションID, self.開始時刻, self.入力文, self.経路,
            tuple(self._events), 応答, 状態, self.前応答ハッシュ, _hash(root_material), self.仕様版
        )
        self._台帳._保存(record)
        return record

class 監査台帳:
    def __init__(self, jsonl_path: str | Path | None = None) -> None:
        self._records: dict[str, 監査記録] = {}
        self._lock = RLock()
        self._path = Path(jsonl_path) if jsonl_path else None

    def 開始(self, 入力文: str, セッションID: str, 前応答ハッシュ: str = "", *, 安全表現=None) -> 監査セッション:
        return 監査セッション(self, 入力文, セッションID, 前応答ハッシュ, 安全表現=安全表現)

    def _保存(self, record: 監査記録) -> None:
        with self._lock:
            if record.追跡ID in self._records:
                raise RuntimeError("同一追跡IDへの上書きは禁止")
            self._records[record.追跡ID] = record
            if self._path:
                self._path.parent.mkdir(parents=True, exist_ok=True)
                with self._path.open("a", encoding="utf-8", newline="\n") as f:
                    f.write(_canonical(record.辞書化()) + "\n")
                    f.flush(); os.fsync(f.fileno())

    def 取得(self, 追跡_id: str) -> 監査記録 | None:
        return self._records.get(追跡_id)

    def 検証(self, 追跡_id: str) -> bool:
        r = self.取得(追跡_id)
        if not r:
            return False
        if r.仕様版 not in (監査仕様版, 安全監査仕様版):
            return False
        prev = "0" * 64
        for e in r.イベント:
            if e.前ハッシュ != prev:
                return False
            material = {
                "spec": r.仕様版, '追跡': r.追跡ID, "n": e.番号,
                "stage": e.段階, 'モジュール': e.モジュール, "version": e.版,
                "input": e.入力, "output": e.出力, '証拠': list(e.根拠), "prev": e.前ハッシュ,
            }
            if _hash(material) != e.ハッシュ:
                return False
            prev = e.ハッシュ
        root_material = {
            "spec": r.仕様版, '追跡': r.追跡ID, "session": r.セッションID,
            "started": r.開始時刻, "input": r.入力文, '経路': r.経路,
            "response": r.最終応答, "status": r.状態, "prev_response": r.前応答ハッシュ,
            "last_event": prev,
        }
        return _hash(root_material) == r.ルートハッシュ
