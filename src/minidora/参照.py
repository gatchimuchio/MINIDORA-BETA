from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
import re
import time
import unicodedata
from dataclasses import dataclass, replace
from typing import Any, Iterable, Protocol


@dataclass(frozen=True, slots=True)
class 参照記録:
    識別子: str
    対象: str
    内容: str
    由来: str
    供給器: str
    信頼: float = 1.0
    意味キー: str | None = None
    値: Any = None
    時点: str | None = None
    範囲: str | None = None
    条件: tuple[tuple[str, str], ...] = ()
    意味確定: bool = False

    @property
    def 表示値(self) -> Any:
        return self.値 if self.意味キー is not None else self.内容


class 参照供給器(Protocol):
    名称: str

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]: ...


@dataclass(frozen=True, slots=True)
class 参照取得診断:
    問合せ: str
    供給器: str
    状態: str
    取得件数: int
    試行回数: int = 1
    エラー: str | None = None
    子診断: tuple["参照取得診断", ...] = ()


def 参照検索を診断(
    供給器: 参照供給器,
    問合せ: str,
    上限: int = 8,
    *,
    最大試行: int = 3,
    再試行待機秒: float = 0.05,
) -> tuple[tuple[参照記録, ...], 参照取得診断]:
    """正常0件と取得失敗を分別し、一時障害だけ有限再試行する。"""
    if type(最大試行) is not int or 最大試行 < 1:
        raise ValueError("最大試行は1以上の整数である必要がある")
    直接診断 = getattr(供給器, "検索診断", None)
    if callable(直接診断) and bool(getattr(供給器, "診断内再試行", False)):
        記録群, 診断 = 直接診断(問合せ, 上限)
        if not isinstance(診断, 参照取得診断):
            raise TypeError("検索診断は参照取得診断を返す必要がある")
        return tuple(記録群), 診断

    if callable(直接診断):
        最終記録群: tuple[参照記録, ...] = ()
        最終診断: 参照取得診断 | None = None
        for 試行 in range(1, 最大試行 + 1):
            記録群, 診断 = 直接診断(問合せ, 上限)
            if not isinstance(診断, 参照取得診断):
                raise TypeError("検索診断は参照取得診断を返す必要がある")
            最終記録群 = tuple(記録群)
            最終診断 = replace(診断, 試行回数=max(int(診断.試行回数), 試行))
            if 診断.状態 in {"取得", "空"}:
                return 最終記録群, 最終診断
            if 試行 < 最大試行 and 再試行待機秒 > 0:
                time.sleep(float(再試行待機秒) * 試行)
        if 最終診断 is None:
            raise RuntimeError("検索診断が実行されなかった")
        return 最終記録群, 最終診断

    最後のエラー: str | None = None
    for 試行 in range(1, 最大試行 + 1):
        try:
            記録群 = tuple(供給器.検索(問合せ, 上限))
            供給器エラー = getattr(供給器, "最後のエラー", None)
            if 供給器エラー:
                raise RuntimeError(str(供給器エラー))
            return 記録群, 参照取得診断(
                str(問合せ),
                str(getattr(供給器, "名称", type(供給器).__name__)),
                "取得" if 記録群 else "空",
                len(記録群),
                試行,
            )
        except Exception as exc:
            最後のエラー = f"{type(exc).__name__}: {exc}"
            if 試行 < 最大試行 and 再試行待機秒 > 0:
                time.sleep(float(再試行待機秒) * 試行)
    return (), 参照取得診断(
        str(問合せ),
        str(getattr(供給器, "名称", type(供給器).__name__)),
        "失敗",
        0,
        最大試行,
        最後のエラー,
    )


def _検索語(問合せ: str) -> set[str]:
    text = unicodedata.normalize("NFKC", 問合せ).casefold().strip()
    if not text:
        return set()
    text = re.sub(r"[\s、。,.!?！？「」『』（）()【】\[\]：:]+", " ", text)
    text = re.sub(
        r"(?<=[0-9a-z一-龯ぁ-んァ-ン])(?:について|から|まで|より|ので|の|は|が|を|に|で|と|へ|や|も)(?=[0-9a-z一-龯ぁ-んァ-ン])",
        " ",
        text,
    )
    return {part for part in text.split() if part}


def 参照矛盾数(記録群: Iterable[参照記録]) -> int:
    groups: dict[
        tuple[str, str, str | None, str | None, tuple[tuple[str, str], ...]],
        set[str],
    ] = {}
    for record in 記録群:
        if record.意味キー is None or not record.意味確定:
            continue
        key = (
            record.対象,
            record.意味キー,
            record.時点,
            record.範囲,
            tuple(record.条件),
        )
        groups.setdefault(key, set()).add(repr(record.値))
    return sum(1 for values in groups.values() if len(values) > 1)


class 固定参照供給器:
    並列安全 = True

    def __init__(self, 記録群: Iterable[参照記録], 名称: str = "固定資料") -> None:
        self.名称 = 名称
        self._記録群 = tuple(記録群)

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        語 = _検索語(問合せ)
        if not 語:
            return self._記録群[:上限]
        採点済み: list[tuple[int, 参照記録]] = []
        for 記録 in self._記録群:
            意味面 = " ".join(
                str(value)
                for value in (
                    記録.対象,
                    記録.意味キー or "",
                    記録.値 if 記録.意味キー is not None else "",
                    記録.時点 or "",
                    記録.範囲 or "",
                    記録.内容,
                )
            )
            本文 = unicodedata.normalize("NFKC", 意味面).casefold()
            点 = sum(1 for 字句 in 語 if 字句 in 本文)
            if 点:
                採点済み.append((点, 記録))
        採点済み.sort(key=lambda x: (-x[0], x[1].識別子))
        return tuple(record for _, record in 採点済み[:上限])


def _記録品質(record: 参照記録) -> tuple[int, float, int]:
    return (1 if record.意味確定 else 0, float(record.信頼), len(str(record.内容)))


def _同一情報源統合(old: 参照記録, new: 参照記録) -> 参照記録:
    '同一識別子の資料を独立情報源へ増やさず、より強い記録へ統合する。'
    best, other = (new, old) if _記録品質(new) > _記録品質(old) else (old, new)
    conditions = list(best.条件)
    for condition in other.条件:
        if condition not in conditions:
            conditions.append(condition)
    for provider in (old.供給器, new.供給器):
        marker = ("observed_by_provider", str(provider))
        if marker not in conditions:
            conditions.append(marker)
    return replace(best, 条件=tuple(conditions), 信頼=max(float(old.信頼), float(new.信頼)))


def 参照記録群を統合(
    既存: Iterable[参照記録],
    追加: Iterable[参照記録],
    *,
    最大件数: int = 8192,
) -> tuple[参照記録, ...]:
    """観測済み参照を識別子単位で保持し、後続処理から再利用可能な記憶へ統合する。"""
    if type(最大件数) is not int or 最大件数 < 1:
        raise ValueError("参照記憶の最大件数は1以上の整数である必要がある")
    結果: list[参照記録] = []
    位置: dict[str, int] = {}
    for record in (*tuple(既存), *tuple(追加)):
        key = str(record.識別子).strip() or f"anonymous:{record.供給器}:{record.由来}:{len(結果)}"
        if key in 位置:
            結果[位置[key]] = _同一情報源統合(結果[位置[key]], record)
            continue
        if len(結果) >= 最大件数:
            break
        位置[key] = len(結果)
        結果.append(record)
    return tuple(結果)


def 参照記録を経験記憶化(record: 参照記録) -> 参照記録:
    """問固有の検索経路印を落とし、観測内容と意味条件だけを後続経験へ残す。"""
    条件 = tuple(
        (str(key), str(value))
        for key, value in record.条件
        if not str(key).startswith(("hds_query_", "hds_observation_"))
    )
    return replace(record, 条件=条件)


def 参照経験記憶を統合(
    既存: Iterable[参照記録],
    追加: Iterable[参照記録],
    *,
    最大件数: int = 8192,
) -> tuple[参照記録, ...]:
    return 参照記録群を統合(
        tuple(参照記録を経験記憶化(x) for x in 既存),
        tuple(参照記録を経験記憶化(x) for x in 追加),
        最大件数=最大件数,
    )


class 複合参照供給器:
    """複数Providerを並列取得し、Provider順を保ったround-robinで統合する。

    同一識別子は独立資料として二重計上せず、信頼度・本文量の高い記録へ品質統合する。
    外側のquery並列は無効化し、Provider内部並列だけを許可して診断の対応関係を固定する。
    """

    並列安全 = False
    診断内再試行 = True

    def __init__(
        self,
        *供給器群: 参照供給器,
        名称: str = "複合参照",
        並列: bool = True,
        最大並列: int = 4,
        最大再試行: int = 3,
        再試行待機秒: float = 0.05,
    ) -> None:
        self.名称 = 名称
        self._供給器群 = tuple(供給器群)
        self.並列 = bool(並列)
        self.最大並列 = max(1, int(最大並列))
        self.最大再試行 = max(1, int(最大再試行))
        self.再試行待機秒 = max(0.0, float(再試行待機秒))
        self.最後のエラー: tuple[tuple[str, str], ...] = ()

    def _取得(self, 供給器: 参照供給器, 問合せ: str, 上限: int) -> tuple[tuple[参照記録, ...], 参照取得診断]:
        return 参照検索を診断(
            供給器,
            問合せ,
            上限,
            最大試行=self.最大再試行,
            再試行待機秒=self.再試行待機秒,
        )

    def 検索診断(self, 問合せ: str, 上限: int = 8) -> tuple[tuple[参照記録, ...], 参照取得診断]:
        if 上限 <= 0 or not self._供給器群:
            return (), 参照取得診断(str(問合せ), self.名称, "空", 0)

        pools: list[tuple[参照記録, ...]] = []
        診断群: list[参照取得診断] = []
        if self.並列 and len(self._供給器群) > 1:
            workers = min(self.最大並列, len(self._供給器群))
            with ThreadPoolExecutor(max_workers=workers, thread_name_prefix="minidora-r") as executor:
                futures = [executor.submit(self._取得, 供給器, 問合せ, 上限) for 供給器 in self._供給器群]
                for future in futures:
                    try:
                        記録群, 診断 = future.result()
                    except Exception as exc:
                        記録群 = ()
                        診断 = 参照取得診断(
                            str(問合せ), "複合子供給器", "失敗", 0, 1, f"{type(exc).__name__}: {exc}"
                        )
                    pools.append(tuple(記録群))
                    診断群.append(診断)
        else:
            for 供給器 in self._供給器群:
                記録群, 診断 = self._取得(供給器, 問合せ, 上限)
                pools.append(tuple(記録群))
                診断群.append(診断)

        完全失敗診断 = tuple(x for x in 診断群 if x.状態 == "失敗")
        縮退診断 = tuple(x for x in 診断群 if x.状態 in {"失敗", "縮退"})
        self.最後のエラー = tuple((x.供給器, x.エラー or x.状態) for x in 縮退診断)

        結果: list[参照記録] = []
        index_by_id: dict[str, int] = {}
        depth = 0
        while True:
            progressed = False
            for pool in pools:
                if depth >= len(pool):
                    continue
                progressed = True
                record = pool[depth]
                existing = index_by_id.get(record.識別子)
                if existing is not None:
                    結果[existing] = _同一情報源統合(結果[existing], record)
                    continue
                if len(結果) >= 上限:
                    continue
                index_by_id[record.識別子] = len(結果)
                結果.append(record)
            if not progressed:
                break
            depth += 1

        if 診断群 and len(完全失敗診断) == len(診断群):
            状態 = "失敗"
        elif 縮退診断:
            状態 = "縮退"
        else:
            状態 = "取得" if 結果 else "空"
        診断 = 参照取得診断(
            str(問合せ),
            self.名称,
            状態,
            len(結果),
            max((x.試行回数 for x in 診断群), default=1),
            None if not 縮退診断 else "; ".join(x.エラー or x.状態 for x in 縮退診断),
            tuple(診断群),
        )
        return tuple(結果), 診断

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        return self.検索診断(問合せ, 上限)[0]
