'明示された能力計画を、資料参照と依存関係を保って実行する。\n\n自然言語解釈、HDSの採否、模型核、永続会話状態の代替ではない。\n登録したPythonコードの隔離実行器でもない。公開契約は設計/37を参照。\n'
from __future__ import annotations

from collections.abc import Callable, Iterable, Mapping
from copy import deepcopy
from dataclasses import asdict, dataclass, replace
from datetime import datetime
from graphlib import CycleError, TopologicalSorter
import hashlib
import json
import math
from typing import Literal

from .製品版.型 import 能力結果, 参照資料
from .製品版.能力契約 import 能力モジュール, 能力文脈

能力合成版 = "MINIDORA-能力合成-v0.1"


@dataclass(frozen=True, slots=True)
class 素材参照:
    領域: Literal["入力", "工程"]
    識別子: str


@dataclass(frozen=True, slots=True)
class 合成工程:
    識別子: str
    能力候補: tuple[str, ...]
    指示参照: str
    入力: tuple[素材参照, ...] = ()
    設定参照: str | None = None


@dataclass(frozen=True, slots=True)
class 合成計画:
    工程: tuple[合成工程, ...]
    出力工程: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class 登録能力:
    モジュール: 能力モジュール
    外部読取: bool = False


@dataclass(frozen=True, slots=True)
class 合成記録:
    工程: str
    能力: str
    版: str
    状態: str
    理由: str
    実行済: bool
    入力ハッシュ: str
    出力ハッシュ: str
    前ハッシュ: str
    ハッシュ: str = ""


@dataclass(frozen=True, slots=True)
class 合成結果:
    状態: str
    出力: tuple[tuple[str, 能力結果], ...]
    中間結果: tuple[tuple[str, 能力結果], ...]
    履歴: tuple[合成記録, ...]
    理由: str
    開始ハッシュ: str
    ルートハッシュ: str = ""

    @property
    def 成立(self) -> bool:
        return self.状態 == "合格"

    @property
    def 試行数(self) -> int:
        return len(self.履歴)

    @property
    def 実行数(self) -> int:
        return sum(r.実行済 for r in self.履歴)

    def 監査整合(self) -> bool:
        """記録内の整合だけを検査する。出典の真正性・回答の正しさは証明しない。"""
        try:
            前 = self.開始ハッシュ
            for 記録 in self.履歴:
                if 記録.前ハッシュ != 前 or 記録.ハッシュ != _記録ハッシュ(記録):
                    return False
                前 = 記録.ハッシュ
            return self.ルートハッシュ == _終端ハッシュ(self)
        except (TypeError, ValueError, RecursionError):
            return False


class _打切り(Exception):
    def __init__(self, 状態: str, 理由: str):
        super().__init__(理由)
        self.状態 = 状態


def _識別子検証(値: object) -> None:
    if not isinstance(値, str) or not 値.strip() or len(値) > 256:
        raise ValueError("識別子不正")


def _JSON検証(値: object, 深さ: int = 0) -> None:
    if 深さ > 32:
        raise ValueError('資料入れ子上限')
    if 値 is None or type(値) in (str, bool, int):
        return
    if type(値) is float and math.isfinite(値):
        return
    if type(値) in (tuple, list):
        for 要素 in 値:
            _JSON検証(要素, 深さ + 1)
        return
    if type(値) is dict and all(type(k) is str for k in 値):
        for 要素 in 値.values():
            _JSON検証(要素, 深さ + 1)
        return
    raise ValueError('資料は有限数・文字列キーを持つJSON互換値に限定')


def _符号化(値: object) -> bytes:
    _JSON検証(値)
    return json.dumps(値, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _ハッシュ(値: object) -> str:
    return hashlib.sha256(_符号化(値)).hexdigest()


def _結果辞書(結果: 能力結果) -> dict:
    if not isinstance(結果, 能力結果) or type(結果.成立) is not bool:
        raise ValueError("能力結果型不正")
    if not isinstance(結果.本文, str) or not isinstance(結果.保留理由, str):
        raise ValueError("能力結果本文型不正")
    if 結果.成立 and 結果.保留理由:
        raise ValueError("成立した能力結果に保留理由を併存させない")
    if type(結果.根拠) is not tuple or not all(isinstance(s, str) for s in 結果.根拠):
        raise ValueError("能力結果根拠型不正")
    if type(結果.参照) is not tuple or type(結果.データ) is not dict:
        raise ValueError('能力結果資料型不正')
    参照 = []
    for 資料 in 結果.参照:
        if not isinstance(資料, 参照資料):
            raise ValueError("参照資料型不正")
        _識別子検証(資料.識別子)
        if not all(isinstance(s, str) for s in (資料.題名, 資料.出典, 資料.URL, 資料.本文)):
            raise ValueError("参照本文型不正")
        if 資料.公開時刻 is not None and not isinstance(資料.公開時刻, datetime):
            raise ValueError("参照時刻型不正")
        参照.append(資料.辞書化())
    値 = {"成立": 結果.成立, "本文": 結果.本文, "根拠": 結果.根拠,
          "参照": 参照, "データ": 結果.データ, "保留理由": 結果.保留理由}
    _JSON検証(値)
    return 値


def _参照結合(資料群: Iterable[参照資料]) -> tuple[参照資料, ...]:
    既出: dict[str, 参照資料] = {}
    for 資料 in 資料群:
        if 資料.識別子 in 既出 and 既出[資料.識別子] != 資料:
            raise ValueError("参照識別子衝突")
        既出[資料.識別子] = 資料
    return tuple(既出.values())


def _文脈辞書(文脈: 能力文脈) -> dict:
    if not isinstance(文脈, 能力文脈):
        raise ValueError("能力文脈型不正")
    if not isinstance(文脈.入力文, str) or not isinstance(文脈.直前応答, str):
        raise ValueError("文脈本文型不正")
    _識別子検証(文脈.セッションID)
    if type(文脈.履歴) is not tuple or any(
        type(h) is not tuple or len(h) != 2 or not all(isinstance(s, str) for s in h)
        for h in 文脈.履歴
    ):
        raise ValueError("文脈履歴型不正")
    if 文脈.補助 is not None and type(文脈.補助) is not dict:
        raise ValueError("文脈補助型不正")
    return {"入力文": 文脈.入力文, "セッションID": 文脈.セッションID,
            "直前": _結果辞書(能力結果(True, 文脈.直前応答, 参照=文脈.直前参照)),
            "履歴": 文脈.履歴, "補助": 文脈.補助}


def _記録ハッシュ(記録: 合成記録) -> str:
    return _ハッシュ(asdict(replace(記録, ハッシュ="")))


def _終端ハッシュ(結果: 合成結果) -> str:
    return _ハッシュ({
        "開始": 結果.開始ハッシュ,
        "末尾": 結果.履歴[-1].ハッシュ if 結果.履歴 else 結果.開始ハッシュ,
        "状態": 結果.状態, "理由": 結果.理由,
        "出力": [(k, _結果辞書(v)) for k, v in 結果.出力],
        "中間結果": [(k, _結果辞書(v)) for k, v in 結果.中間結果],
    })


class 能力合成器:
    """登録済み能力だけを実行する、明示計画の依存解決・合成実行器。"""

    def __init__(self, 能力: Iterable[登録能力], *, 最大工程数: int = 64,
                 最大試行数: int = 128, 資料バイト上限: int = 2_000_000):
        for 値 in (最大工程数, 最大試行数, 資料バイト上限):
            if type(値) is not int or 値 <= 0:
                raise ValueError("上限は正の整数")
        self._上限 = (最大工程数, 最大試行数, 資料バイト上限)
        self._能力: dict[str, tuple[登録能力, str]] = {}
        for 登録 in 能力:
            if not isinstance(登録, 登録能力) or type(登録.外部読取) is not bool:
                raise ValueError("能力登録型不正")
            本体 = 登録.モジュール
            _識別子検証(本体.名前)
            _識別子検証(本体.版)
            if 本体.名前 in self._能力:
                raise ValueError("能力名重複")
            if not callable(本体.判定) or not callable(本体.実行):
                raise ValueError("能力呼出契約不正")
            self._能力[本体.名前] = (登録, 本体.版)

    def _サイズ検証(self, 値: object) -> None:
        if len(_符号化(値)) > self._上限[2]:
            raise ValueError("資料バイト上限")

    def _準備(self, 計画: 合成計画, 資料: Mapping[str, 能力結果],
              外部読取許可: bool) -> tuple[dict[str, 合成工程], tuple[str, ...]]:
        if not isinstance(計画, 合成計画) or type(計画.工程) is not tuple:
            raise ValueError("合成計画型不正")
        if type(外部読取許可) is not bool:
            raise ValueError("外部読取許可型不正")
        if not 0 < len(計画.工程) <= self._上限[0]:
            raise ValueError("工程数範囲外")
        工程 = {}
        for 項 in 計画.工程:
            if not isinstance(項, 合成工程):
                raise ValueError("合成工程型不正")
            _識別子検証(項.識別子)
            if 項.識別子 in 工程:
                raise ValueError("工程ID重複")
            工程[項.識別子] = 項
        if type(計画.出力工程) is not tuple or not 計画.出力工程:
            raise ValueError("出力工程未指定")
        for 出力 in 計画.出力工程:
            _識別子検証(出力)
            if 出力 not in 工程:
                raise ValueError("出力工程欠落")
        if len(set(計画.出力工程)) != len(計画.出力工程):
            raise ValueError("出力工程重複")

        def 入力検証(識別子: str) -> None:
            _識別子検証(識別子)
            if 識別子 not in 資料:
                raise _打切り("保留", f"入力Data欠落:{識別子}")
            if not 資料[識別子].成立:
                raise _打切り("保留", f"入力Data未成立:{識別子}")

        依存 = {}
        for ID, 項 in sorted(工程.items()):
            if type(項.能力候補) is not tuple or not 項.能力候補:
                raise ValueError("能力候補未指定")
            for 名前 in 項.能力候補:
                _識別子検証(名前)
                if 名前 not in self._能力:
                    raise _打切り("保留", f"未登録能力:{名前}")
                登録, 版 = self._能力[名前]
                if 登録.モジュール.名前 != 名前 or 登録.モジュール.版 != 版:
                    raise ValueError("登録後能力名版変更")
                if 登録.外部読取 and not 外部読取許可:
                    raise _打切り("保留", f"外部読取未許可:{名前}")
            if len(set(項.能力候補)) != len(項.能力候補):
                raise ValueError("能力候補重複")
            入力検証(項.指示参照)
            if 項.設定参照 is not None:
                入力検証(項.設定参照)
            if type(項.入力) is not tuple:
                raise ValueError("素材参照型不正")
            親 = set()
            for 参照 in 項.入力:
                if not isinstance(参照, 素材参照):
                    raise ValueError("素材参照型不正")
                _識別子検証(参照.識別子)
                if 参照.領域 == "入力":
                    入力検証(参照.識別子)
                elif 参照.領域 == "工程" and 参照.識別子 in 工程:
                    親.add(参照.識別子)
                else:
                    raise ValueError("素材参照先不正")
            依存[ID] = tuple(sorted(親))
        try:
            順序 = tuple(TopologicalSorter(依存).static_order())
        except CycleError as exc:
            raise ValueError("依存循環") from exc
        到達, 探索 = set(), list(計画.出力工程)
        while 探索:
            ID = 探索.pop()
            if ID not in 到達:
                到達.add(ID)
                探索.extend(依存[ID])
        if 到達 != set(工程):
            raise ValueError("出力に寄与しない工程")
        return 工程, 順序

    def 実行(self, 計画: 合成計画, 初期資料: Mapping[str, 能力結果], *,
             文脈: 能力文脈 | None = None, 外部読取許可: bool = False,
             停止要求: Callable[[], bool] | None = None) -> 合成結果:
        完了: dict[str, 能力結果] = {}
        記録: list[合成記録] = []
        開始 = _ハッシュ({"版": 能力合成版, "状態": "準備前"})
        前 = 開始

        def 終了(状態: str, 理由: str = "") -> 合成結果:
            出力 = tuple((k, 完了[k]) for k in 計画.出力工程) if 状態 == "合格" else ()
            結果 = 合成結果(状態, deepcopy(出力), deepcopy(tuple(完了.items())),
                            tuple(記録), 理由, 開始)
            return replace(結果, ルートハッシュ=_終端ハッシュ(結果))

        def 停止確認() -> None:
            if 停止要求 is not None:
                try:
                    値 = 停止要求()
                except Exception as exc:
                    raise _打切り("失敗", f"停止判定失敗:{type(exc).__name__}") from exc
                if type(値) is not bool:
                    raise _打切り("失敗", "停止要求型不正")
                if 値:
                    raise _打切り("中止", "停止要求")

        try:
            if not isinstance(初期資料, Mapping):
                raise ValueError('初期資料型不正')
            for ID, 値 in 初期資料.items():
                _識別子検証(ID)
                _結果辞書(値)
                _参照結合(値.参照)
            資料 = deepcopy(dict(初期資料))
            基点 = deepcopy(文脈 if 文脈 is not None else 能力文脈("", "能力合成"))
            self._サイズ検証(_文脈辞書(基点))
            工程, 順序 = self._準備(計画, 資料, 外部読取許可)
            宣言 = {"版": 能力合成版, "計画": asdict(計画), "文脈": _文脈辞書(基点),
                    '初期資料': {k: _結果辞書(v) for k, v in 資料.items()},
                    "能力": [(k, v, r.外部読取) for k, (r, v) in sorted(self._能力.items())],
                    "外部読取許可": 外部読取許可, "上限": self._上限}
            self._サイズ検証(宣言)
            開始 = 前 = _ハッシュ(宣言)
            for ID in 順序:
                停止確認()
                項 = 工程[ID]
                素材 = [(r, 資料[r.識別子] if r.領域 == "入力" else 完了[r.識別子]) for r in 項.入力]
                参照 = _参照結合(r for _, 値 in 素材 for r in 値.参照)
                補助 = deepcopy(基点.補助 or {})
                補助["合成入力"] = tuple({"参照": asdict(r), "結果": _結果辞書(v)} for r, v in 素材)
                補助["合成設定"] = deepcopy(資料[項.設定参照].データ) if 項.設定参照 else {}
                局所 = replace(基点, 入力文=資料[項.指示参照].本文,
                               直前応答="\n\n".join(v.本文 for _, v in 素材),
                               直前参照=参照, 補助=補助)
                self._サイズ検証(_文脈辞書(局所))
                入力hash = _ハッシュ(_文脈辞書(局所))
                工程成功, 工程異常 = False, False
                for 名前 in 項.能力候補:
                    停止確認()
                    if len(記録) >= self._上限[1]:
                        raise _打切り("保留", "能力試行数上限")
                    登録, 版 = self._能力[名前]
                    状態, 理由, 実行済, 出力hash, 採用 = "非該当", "", False, "", None
                    制御停止 = False
                    try:
                        if 登録.モジュール.名前 != 名前 or 登録.モジュール.版 != 版:
                            raise ValueError("登録後能力名版変更")
                        信頼 = 登録.モジュール.判定(deepcopy(局所))
                        if type(信頼) not in (int, float) or not math.isfinite(信頼) or not 0 <= 信頼 <= 1:
                            raise ValueError("能力判定値不正")
                        停止確認()
                        if 信頼 > 0:
                            実行済 = True
                            候補 = deepcopy(登録.モジュール.実行(deepcopy(局所)))
                            if 登録.モジュール.名前 != 名前 or 登録.モジュール.版 != 版:
                                raise ValueError("実行中能力名版変更")
                            self._サイズ検証(_結果辞書(候補))
                            出力hash = _ハッシュ(_結果辞書(候補))
                            停止確認()
                            if 候補.成立:
                                採用 = replace(候補, 参照=_参照結合((*参照, *候補.参照)))
                                self._サイズ検証(_結果辞書(採用))
                                出力hash = _ハッシュ(_結果辞書(採用))
                                状態 = "合格"
                            else:
                                状態, 理由 = "保留", 候補.保留理由 or 'モジュール不成立'
                    except _打切り as exc:
                        状態, 理由 = exc.状態, str(exc)
                        制御停止 = True
                    except Exception as exc:
                        状態, 理由 = "失敗", f"能力呼出・結果契約違反:{type(exc).__name__}"
                    一件 = 合成記録(ID, 名前, 版, 状態, 理由, 実行済, 入力hash, 出力hash, 前)
                    一件 = replace(一件, ハッシュ=_記録ハッシュ(一件))
                    記録.append(一件)
                    前 = 一件.ハッシュ
                    if 制御停止:
                        return 終了(状態, 理由)
                    工程異常 |= 状態 == "失敗"
                    if 採用 is not None:
                        完了[ID] = 採用
                        工程成功 = True
                        break
                if not 工程成功:
                    return 終了("失敗" if 工程異常 else "保留", f"工程未完了:{ID}")
            停止確認()
            return 終了("合格")
        except _打切り as exc:
            return 終了(exc.状態, str(exc))
        except (TypeError, ValueError) as exc:
            return 終了("失敗", f"合成契約違反:{exc}")
        except Exception as exc:
            return 終了("失敗", f"合成契約・制御失敗:{type(exc).__name__}")
