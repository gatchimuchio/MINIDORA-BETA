from __future__ import annotations

from dataclasses import asdict, dataclass, field, replace
from typing import Any, Mapping

from .採否 import 実行状態


@dataclass(frozen=True, slots=True)
class 主体状態:
    """MINIDORAが次の判断へ持ち越す、明示的で監査可能な主体状態。"""

    主体ID: str = "MINIDORA"
    現在目的: tuple[str, ...] = ()
    判断基準: tuple[str, ...] = ()
    立場: tuple[tuple[str, str], ...] = ()
    選好: tuple[tuple[str, str], ...] = ()
    約束: tuple[str, ...] = ()
    仮説: tuple[str, ...] = ()
    未解残差: tuple[str, ...] = ()
    版: int = 0

    def 辞書化(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True, slots=True)
class 主体更新提案:
    """専門処理やLayer-0が主体主幹へ返す差分。主体状態を直接書き換えない。"""

    変更: Mapping[str, Any] = field(default_factory=dict)
    理由: tuple[str, ...] = ()
    根拠: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        object.__setattr__(self, "変更", dict(self.変更))


@dataclass(frozen=True, slots=True)
class 主体整合結果:
    状態: 実行状態
    理由: tuple[str, ...]
    適用差分: tuple[tuple[str, Any, Any], ...] = ()
    更新後: 主体状態 | None = None


@dataclass(frozen=True, slots=True)
class 主体更新記録:
    旧版: int
    新版: int
    差分: tuple[tuple[str, Any, Any], ...]
    理由: tuple[str, ...]
    根拠: tuple[str, ...]


class 主体主幹:
    'K3由来の動的能力処理から独立した、迂回不能な主体状態の読取・更新関門。'

    _更新可能項目 = {
        "現在目的",
        "判断基準",
        "立場",
        "選好",
        "約束",
        "仮説",
        "未解残差",
    }

    def __init__(self, 初期状態: 主体状態 | None = None) -> None:
        self._状態 = 初期状態 or 主体状態()
        self._履歴: list[主体更新記録] = []

    @property
    def 現在(self) -> 主体状態:
        return self._状態

    @property
    def 履歴(self) -> tuple[主体更新記録, ...]:
        return tuple(self._履歴)

    def 状態辞書(self) -> dict[str, Any]:
        return self._状態.辞書化()

    def _正規化(self, key: str, value: Any) -> Any:
        if key in {"現在目的", "判断基準", "約束", "仮説", "未解残差"}:
            if isinstance(value, str):
                return (value,)
            return tuple(str(x) for x in value)
        if key in {"立場", "選好"}:
            if isinstance(value, Mapping):
                return tuple(sorted((str(k), str(v)) for k, v in value.items()))
            return tuple((str(k), str(v)) for k, v in value)
        return value

    def 評価更新(self, 提案: 主体更新提案 | None) -> 主体整合結果:
        if 提案 is None or not 提案.変更:
            return 主体整合結果(実行状態.合格, ("主体更新なし",), (), self._状態)

        禁止 = set(提案.変更) - self._更新可能項目
        if 禁止:
            return 主体整合結果(
                実行状態.失敗,
                (f"主体主幹の更新境界外: {','.join(sorted(禁止))}",),
                (),
                self._状態,
            )

        normalized = {key: self._正規化(key, value) for key, value in 提案.変更.items()}
        差分 = tuple(
            (key, getattr(self._状態, key), value)
            for key, value in sorted(normalized.items())
            if getattr(self._状態, key) != value
        )
        if not 差分:
            return 主体整合結果(実行状態.合格, ("主体状態に実差分なし",), (), self._状態)

        if not 提案.理由:
            return 主体整合結果(
                実行状態.保留,
                ("理由なし主体変更", "旧状態を維持"),
                差分,
                self._状態,
            )

        更新値 = {key: value for key, _, value in 差分}
        旧状態 = self._状態
        新状態 = replace(旧状態, **更新値, 版=旧状態.版 + 1)
        記録 = 主体更新記録(
            旧版=旧状態.版,
            新版=新状態.版,
            差分=差分,
            理由=tuple(提案.理由),
            根拠=tuple(提案.根拠),
        )
        self._状態 = 新状態
        self._履歴.append(記録)
        return 主体整合結果(実行状態.合格, ("理由付き主体更新",), 差分, 新状態)

    def 非適用結果(self, 理由: str) -> 主体整合結果:
        return 主体整合結果(実行状態.非適用, (理由,), (), self._状態)
