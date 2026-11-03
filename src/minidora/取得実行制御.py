"""当回の公開取得資源を共有する。検索の必要性・意味・採否は判定しない。"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import math
from threading import RLock
import time
from uuid import uuid4


class 取得資源不足(ValueError):
    def __init__(self, 理由: str, *, 課金結果未確認: bool = False):
        if type(課金結果未確認) is not bool:
            raise TypeError("課金結果未確認はbool")
        super().__init__(理由)
        self.理由 = 理由
        self.課金結果未確認 = 課金結果未確認


@dataclass(frozen=True, slots=True)
class 公開取得上限:
    最大検索回数: int = 2
    最大候補数: int = 6
    最大本文取得数: int = 6
    最大HTTP開始数: int = 30
    最大総バイト数: int = 8_000_000
    全体期限秒: float = 240.0
    最小通信間隔秒: float = 0.25

    def __post_init__(self):
        for 名, 最大 in (("最大検索回数", 8), ("最大候補数", 80),
                         ("最大本文取得数", 32), ("最大HTTP開始数", 128),
                         ("最大総バイト数", 64_000_000)):
            値 = getattr(self, 名)
            if type(値) is not int or not 1 <= 値 <= 最大:
                raise ValueError("公開取得の回数・容量上限が不正")
        for 名, 下限, 最大 in (("全体期限秒", 0, 3600), ("最小通信間隔秒", 0, 60)):
            値 = getattr(self, 名)
            if (type(値) not in (int, float) or not math.isfinite(値)
                    or not 下限 <= 値 <= 最大 or 名 == "全体期限秒" and 値 == 0):
                raise ValueError("公開取得の時間上限が不正")


class 共有取得制御:
    """一要求だけの実行制御。複製しても同じ資源帳を共有し、保存はしない。"""

    _上限名 = {"検索回数": "最大検索回数", "候補数": "最大候補数",
              "本文取得数": "最大本文取得数", "HTTP開始数": "最大HTTP開始数",
              "総バイト数": "最大総バイト数"}

    def __init__(self, 上限: 公開取得上限, *, 停止要求=None, 開始時刻=None, 公開条件=None):
        from .公開送信条件 import 公開送信条件
        if 公開条件 is not None and not isinstance(公開条件, 公開送信条件):
            raise TypeError("共有取得の公開送信条件不正")
        if not isinstance(上限, 公開取得上限):
            raise TypeError("公開取得上限の型不正")
        if 停止要求 is not None and not callable(停止要求):
            raise TypeError("取得の停止要求はcallable")
        開始 = time.monotonic() if 開始時刻 is None else 開始時刻
        if type(開始) not in (int, float) or not math.isfinite(開始):
            raise ValueError("取得開始時刻が不正")
        self.上限 = 上限
        self.公開条件 = 公開条件
        self._停止要求 = 停止要求
        self._期限 = 開始 + 上限.全体期限秒
        self._当回ID = uuid4().hex
        self._次通信 = 開始
        self._使用 = {名: 0 for 名 in self._上限名}
        self._未対応 = set()
        self._ロック = RLock()

    def __deepcopy__(self, memo):
        memo[id(self)] = self
        return self

    def 確認(self):
        if self._停止要求 is not None:
            停止 = self._停止要求()
            if type(停止) is not bool:
                raise 取得資源不足("停止要求型不正")
            if 停止:
                raise 取得資源不足("停止要求")
        if time.monotonic() >= self._期限:
            raise 取得資源不足("取得全体期限")

    def 残時間(self, 最大秒):
        self.確認()
        残り = self._期限 - time.monotonic()
        if 残り <= 0:
            raise 取得資源不足("取得全体期限")
        return min(最大秒, 残り)

    def 残量(self, 名):
        if 名 not in self._上限名:
            raise ValueError("取得資源名が不正")
        with self._ロック:
            return getattr(self.上限, self._上限名[名]) - self._使用[名]

    def 消費(self, 名, 数=1):
        if 名 not in self._上限名 or type(数) is not int or 数 < 1:
            raise ValueError("取得資源の消費型が不正")
        self.確認()
        with self._ロック:
            if 数 > self.残量(名):
                raise 取得資源不足(名 + "上限")
            self._使用[名] += 数

    def 通信を開始(self):
        self.確認()
        with self._ロック:
            if self.残量("総バイト数") <= 0:
                raise 取得資源不足("総バイト数上限")
            self.消費("HTTP開始数")
            予定 = max(time.monotonic(), self._次通信)
            self._次通信 = 予定 + self.上限.最小通信間隔秒
        while (残り := 予定 - time.monotonic()) > 0:
            self.確認()
            time.sleep(min(0.1, 残り))
        self.確認()

    def 受信を記録(self, 数):
        # 停止・期限を読取後に観測しても、既に受信したバイトを消去しない。
        if type(数) is not int or 数 < 1:
            raise ValueError("受信バイト数が不正")
        with self._ロック:
            self._使用["総バイト数"] += 数
            if self._使用["総バイト数"] > self.上限.最大総バイト数:
                raise 取得資源不足("総バイト数上限")

    def 記録(self):
        with self._ロック:
            return {"版": "公開取得資源-v2", "当回ID": self._当回ID, "上限": asdict(self.上限),
                    "使用": dict(self._使用),
                    "制御未対応": sorted(self._未対応),
                    "HTTP計数": "接続開始の予約。DNS・接続失敗も消費し、HTTP成功数ではない",
                    "バイト計数": "受信した応答本文のrawバイト。ヘッダ・TLS通信量は含まない"}

    def 未対応を記録(self, 役割):
        if 役割 not in {"取得器", "検索供給器", "本文供給器"}:
            raise ValueError("取得供給器の役割が不正")
        with self._ロック:
            self._未対応.add(役割)


def 制御を配送する(供給器, 制御, 役割):
    """旧供給器へ新引数を渡した後の再実行はしない。未対応は明示する。"""
    if 制御 is None:
        return 供給器
    if not isinstance(制御, 共有取得制御):
        raise TypeError("取得制御の型不正")
    束縛 = getattr(供給器, "制御を束縛", None)
    if callable(束縛):
        return 束縛(制御)
    制御.未対応を記録(役割)
    return 供給器
