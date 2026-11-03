from __future__ import annotations

from dataclasses import dataclass, replace
from typing import Any


@dataclass(frozen=True, slots=True)
class 局所解釈スナップショット:
    '一つのMINIDORA 実行系内だけで保持する現在解釈の作業状態。'

    版: int = 0
    直前入力: str | None = None
    現在焦点: Any = None
    直前結果: Any = None
    直前IR: Any = None
    直前採否: str | None = None
    未解残差: tuple[tuple[str, str], ...] = ()
    IR履歴: tuple[Any, ...] = ()

    def 辞書化(self) -> dict[str, Any]:
        return {
            "版": self.版,
            "直前入力": self.直前入力,
            "現在焦点": self.現在焦点,
            "直前結果": self.直前結果,
            "直前採否": self.直前採否,
            "未解残差": self.未解残差,
            "IR履歴長": len(self.IR履歴),
        }


class 局所解釈キャッシュ:
    'LLM 実行系の寿命にだけ従う局所作業キャッシュ。\n\n    永続化・端末間同期・人格同一性を担わない。各turnは更新前スナップショットを\n    意思決定の起点とし、turn完了後にだけ次状態へ更新する。\n    '

    def __init__(self) -> None:
        self._現在 = 局所解釈スナップショット()

    @property
    def 現在(self) -> 局所解釈スナップショット:
        return self._現在

    def 起点(self) -> 局所解釈スナップショット:
        return self._現在

    def 初期化(self) -> None:
        self._現在 = 局所解釈スナップショット()

    @staticmethod
    def _採否値(採否状態: Any) -> str:
        value = getattr(採否状態, "value", 採否状態)
        return str(value)

    @staticmethod
    def _残差(ir: Any) -> tuple[tuple[str, str], ...]:
        rows: list[tuple[str, str]] = []
        for item in tuple(getattr(ir, "残差", ()) or ()):
            rows.append((str(getattr(item, "種別", "残差")), str(getattr(item, "理由", ""))))
        return tuple(rows)

    def 更新(self, 入力: str, 採否状態: Any, 値: Any = None, ir: Any = None) -> 局所解釈スナップショット:
        before = self._現在
        status = self._採否値(採否状態)
        history = before.IR履歴 + ((ir,) if ir is not None else ())
        last_ir = ir if ir is not None else before.直前IR
        focus = before.現在焦点
        last_結果 = before.直前結果
        unresolved = before.未解残差

        if status == "合格" and 値 is not None:
            focus = 値
            last_結果 = 値
            unresolved = ()
        elif status == "保留" and ir is not None:
            residuals = self._残差(ir)
            if residuals:
                unresolved = residuals

        self._現在 = replace(
            before,
            版=before.版 + 1,
            直前入力=str(入力),
            現在焦点=focus,
            直前結果=last_結果,
            直前IR=last_ir,
            直前採否=status,
            未解残差=unresolved,
            IR履歴=history,
        )
        return self._現在


__all__ = ["局所解釈スナップショット", "局所解釈キャッシュ"]
