from __future__ import annotations

from hashlib import sha256
from typing import Callable, Sequence

from .HDS実行主体 import HDS作用機会, HDS作用結果, HDS作用状態, HDS実行状態
from .参照 import 参照供給器, 参照記録
from .計算中間表現 import 計算中間表現, 計算実行結果
from .計算実行器 import 計算実行器
from .統合駆動_v2.計画 import HDS作用仕様


class HDS参照取得作用:
    """外部参照RをHDSの観測作用として接続する。

    参照が取得できたことと、問いに対して意味的に十分であることは分離する。
    本作用は資料取得だけを成立状態にし、証拠十分性・最終採用は後続HDS作用へ残す。

    作用入力署名は実際の検索入力だけから作る。無関係なHDS残差や成果が増えても、
    同じ供給器・同じ問合せ・同じ取得段階を再実行しない。段階的取得は`取得段階`を変える。
    """

    def __init__(
        self,
        供給器: 参照供給器,
        問合せ: str | None = None,
        *,
        問合せ生成: Callable[[HDS実行状態], str] | None = None,
        入力状態: Sequence[str] = (),
        出力状態: str = "参照取得済み",
        解消対象: Sequence[str] = ("観測不足",),
        取得上限: int = 8,
        取得段階: str = "標準",
        作用ID: str = "参照取得",
        資源負荷: int = 4,
        目的依存: Sequence[str] = (),
    ) -> None:
        if not callable(getattr(供給器, "検索", None)):
            raise TypeError("HDS参照取得作用には検索可能な参照供給器が必要")
        if 問合せ is not None and 問合せ生成 is not None:
            raise ValueError("固定問合せと問合せ生成は同時指定できない")
        if 問合せ is None and 問合せ生成 is None:
            raise ValueError("固定問合せまたは問合せ生成が必要")
        if type(取得上限) is not int or not 1 <= 取得上限 <= 64:
            raise ValueError("参照取得上限は1..64の整数である必要がある")
        if not isinstance(取得段階, str) or not 取得段階.strip():
            raise ValueError("参照取得段階は空にできない")
        if not isinstance(作用ID, str) or not 作用ID.strip():
            raise ValueError("参照取得作用IDは空にできない")
        self.供給器 = 供給器
        self.問合せ = str(問合せ) if 問合せ is not None else None
        self.問合せ生成 = 問合せ生成
        self.入力状態 = frozenset(str(x) for x in 入力状態)
        self.出力状態 = str(出力状態)
        self.解消対象 = frozenset(str(x) for x in 解消対象)
        self.取得上限 = 取得上限
        self.取得段階 = 取得段階.strip()
        self.資源負荷 = max(0, int(資源負荷))
        self.作用ID = 作用ID.strip()
        self.目的依存 = tuple(str(x) for x in 目的依存)
        if any(not x.startswith(("状態:", "残差:", "認識:")) for x in self.目的依存):
            raise ValueError("参照取得の目的依存は状態:/残差:/認識:で修飾する")
        self.計画仕様 = HDS作用仕様(
            self.作用ID, self.入力状態, frozenset({self.出力状態}),
            解消残差=self.解消対象, 資源負荷=self.資源負荷,
            版="参照取得:" + self.取得段階,
        )

    def _問合せ(self, 状態: HDS実行状態) -> str:
        値 = self.問合せ if self.問合せ生成 is None else self.問合せ生成(状態)
        if not isinstance(値, str) or not 値.strip():
            raise ValueError("参照問合せは空でない文字列である必要がある")
        return 値.strip()

    def _入力署名(self, 問合せ: str) -> str:
        供給器名 = str(getattr(self.供給器, "名称", type(self.供給器).__qualname__))
        材料 = repr((
            self.作用ID,
            self.取得段階,
            問合せ,
            self.取得上限,
            type(self.供給器).__module__,
            type(self.供給器).__qualname__,
            供給器名,
        )).encode("utf-8")
        return sha256(材料).hexdigest()

    def 機会(self, 状態: HDS実行状態) -> HDS作用機会 | None:
        if not self.入力状態.issubset(状態.成立状態):
            return None
        if self.出力状態 in 状態.成立状態:
            return None
        try:
            問合せ = self._問合せ(状態)
        except Exception:
            return None
        return HDS作用機会(
            self.作用ID,
            self._入力署名(問合せ),
            self.入力状態,
            frozenset({self.出力状態}),
            self.解消対象,
            self.資源負荷,
            0.0,
            True,
            ("REFERENCE_PROVIDER_AS_HDS_OBSERVATION", f"取得段階:{self.取得段階}"),
            目的依存=self.目的依存,
        )

    def 実行(self, 状態: HDS実行状態) -> HDS作用結果:
        try:
            問合せ = self._問合せ(状態)
            記録群 = tuple(self.供給器.検索(問合せ, self.取得上限))
        except Exception as exc:
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({f"参照取得失敗:{type(exc).__name__}"}),
                理由=("REFERENCE_FETCH_FAILED", type(exc).__name__),
            )
        if any(not isinstance(記録, 参照記録) for 記録 in 記録群):
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({"参照取得結果型不正"}),
                理由=("REFERENCE_FETCH_RETURN_TYPE_INVALID",),
            )
        if not 記録群:
            return HDS作用結果(
                HDS作用状態.保留,
                追加残差=frozenset({f"参照不足:{問合せ}"}),
                成果=(("参照記録", ()),),
                理由=("REFERENCE_NOT_FOUND",),
            )
        return HDS作用結果(
            HDS作用状態.成立,
            追加状態=frozenset({self.出力状態}),
            解消残差=self.解消対象,
            成果=(("参照記録", 記録群),),
            理由=("REFERENCE_FETCHED", f"件数:{len(記録群)}", f"取得段階:{self.取得段階}"),
        )


class HDS計算実行作用:
    """既存の決定論的計算実行器をHDS作用として接続する。

    計算成功は局所成果であり、全目的のCOMMITではない。結果値と計算履歴をHDS成果へ帰還する。
    作用入力署名は実際の計算IRと初期状態だけから作り、無関係なHDS状態変化で再実行しない。
    """

    def __init__(
        self,
        実行器: 計算実行器,
        計算IR: 計算中間表現 | None = None,
        *,
        計算入力生成: Callable[[HDS実行状態], tuple[計算中間表現, dict]] | None = None,
        初期状態: dict | None = None,
        入力状態: Sequence[str] = (),
        出力状態: str = "計算実行済み",
        解消対象: Sequence[str] = ("計算要求",),
        作用ID: str = "計算実行",
        資源負荷: int = 1,
        目的依存: Sequence[str] = (),
    ) -> None:
        if not callable(getattr(実行器, "計算実行", None)):
            raise TypeError("HDS計算実行作用には計算実行器が必要")
        if 計算IR is not None and 計算入力生成 is not None:
            raise ValueError("固定計算IRと計算入力生成は同時指定できない")
        if 計算IR is None and 計算入力生成 is None:
            raise ValueError("固定計算IRまたは計算入力生成が必要")
        if 計算IR is not None and not isinstance(計算IR, 計算中間表現):
            raise TypeError("固定計算IRの型が不正")
        if not isinstance(作用ID, str) or not 作用ID.strip():
            raise ValueError("計算実行作用IDは空にできない")
        self.実行器 = 実行器
        self.計算IR = 計算IR
        self.計算入力生成 = 計算入力生成
        self.初期状態 = dict(初期状態 or {})
        self.入力状態 = frozenset(str(x) for x in 入力状態)
        self.出力状態 = str(出力状態)
        self.解消対象 = frozenset(str(x) for x in 解消対象)
        self.資源負荷 = max(0, int(資源負荷))
        self.作用ID = 作用ID.strip()
        self.目的依存 = tuple(str(x) for x in 目的依存)
        if any(not x.startswith(("状態:", "残差:", "認識:")) for x in self.目的依存):
            raise ValueError("計算実行の目的依存は状態:/残差:/認識:で修飾する")
        self.計画仕様 = HDS作用仕様(
            self.作用ID, self.入力状態, frozenset({self.出力状態}),
            解消残差=self.解消対象, 資源負荷=self.資源負荷,
            版="計算実行:v1",
        )

    def _計算入力(self, 状態: HDS実行状態) -> tuple[計算中間表現, dict]:
        if self.計算入力生成 is None:
            assert self.計算IR is not None
            return self.計算IR, dict(self.初期状態)
        値 = self.計算入力生成(状態)
        if not isinstance(値, tuple) or len(値) != 2:
            raise TypeError("計算入力生成は(計算中間表現, dict)を返す必要がある")
        中間表現, 初期状態 = 値
        if not isinstance(中間表現, 計算中間表現) or not isinstance(初期状態, dict):
            raise TypeError("計算入力生成の返却型が不正")
        return 中間表現, dict(初期状態)

    def _入力署名(self, 中間表現: 計算中間表現, 初期状態: dict) -> str:
        材料 = repr((
            self.作用ID,
            中間表現,
            tuple(sorted(初期状態.items(), key=lambda 行: str(行[0]))),
            type(self.実行器).__module__,
            type(self.実行器).__qualname__,
        )).encode("utf-8")
        return sha256(材料).hexdigest()

    def 機会(self, 状態: HDS実行状態) -> HDS作用機会 | None:
        if not self.入力状態.issubset(状態.成立状態):
            return None
        if self.出力状態 in 状態.成立状態:
            return None
        try:
            中間表現, 初期状態 = self._計算入力(状態)
        except Exception:
            return None
        return HDS作用機会(
            self.作用ID,
            self._入力署名(中間表現, 初期状態),
            self.入力状態,
            frozenset({self.出力状態}),
            self.解消対象,
            self.資源負荷,
            0.0,
            True,
            ("COMPUTE_EXECUTOR_AS_HDS_ACTION",),
            目的依存=self.目的依存,
        )

    def 実行(self, 状態: HDS実行状態) -> HDS作用結果:
        try:
            中間表現, 初期状態 = self._計算入力(状態)
            結果 = self.実行器.計算実行(中間表現, 初期状態)
        except Exception as exc:
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({f"計算実行失敗:{type(exc).__name__}"}),
                理由=("COMPUTE_EXECUTION_FAILED", type(exc).__name__),
            )
        if not isinstance(結果, 計算実行結果):
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({"計算実行結果型不正"}),
                理由=("COMPUTE_RETURN_TYPE_INVALID",),
            )
        if 結果.出力 is None:
            return HDS作用結果(
                HDS作用状態.保留,
                追加残差=frozenset({"計算結果未形成"}),
                成果=(("計算実行結果", 結果),),
                理由=("COMPUTE_OUTPUT_ABSENT",),
            )
        return HDS作用結果(
            HDS作用状態.成立,
            追加状態=frozenset({self.出力状態}),
            解消残差=self.解消対象,
            成果=(("計算実行結果", 結果), ("計算結果", 結果.出力)),
            理由=("COMPUTE_EXECUTED",),
        )


__all__ = ["HDS参照取得作用", "HDS計算実行作用"]
