from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Any

from .模型 import MINIDORA模型核, 成立候補, 言語状態, 標準模型核
from .言語基底 import 標準言語基底P
from .言語基底_英語 import 英語語形数


規模測定版 = "v2"

_関係代表表層 = {
    "因果": "causes",
    "増加": "increases",
    "減少": "decreases",
    "阻害": "inhibits",
    "活性化": "activates",
    "生成": "produces",
    "要求": "requires",
    "包含": "contains",
    "使用": "uses",
    "防止": "prevents",
    "相関": "correlates with",
    "結合": "binds to",
    "相互作用": "interacts with",
    "構成": "consists of",
    "所属": "belongs to",
    "位置": "is located in",
    "由来": "derives from",
}


@dataclass(frozen=True, slots=True)
class 規模測定結果:
    版: str
    対象言語体系: tuple[str, ...]
    対象範囲: str
    比較集合: tuple[str, ...]
    状態域規模: dict[str, Any]
    関係域規模: dict[str, Any]
    共有適用規模: dict[str, Any]
    物理規模値: dict[str, Any]
    大規模性状態: str
    理由: tuple[str, ...]

    def 辞書(self) -> dict[str, Any]:
        return asdict(self)


def _内部署名(kernel: MINIDORA模型核, text: str, system: str) -> tuple[object, ...]:
    状態 = kernel.言語対応.内部化(言語状態(text, system))
    return (tuple(sorted(状態.意味語集合)), 状態.構造署名)


def _状態域測定(kernel: MINIDORA模型核) -> dict[str, Any]:
    rows: list[tuple[str, str]] = []
    for i in range(128):
        rows.append(("自然言語:en", f"entity_{i} causes result_{i % 31}"))
        rows.append(("自然言語:ja", f"対象{i} 状態{i % 23} 関係{i % 17}"))
        rows.append(("program:python", f"value_{i} = item_{i % 31} + {i % 13}"))

    signatures = {(system, _内部署名(kernel, text, system)) for system, text in rows}
    long_ok = bool(_内部署名(kernel, "語" * 10000, "自然言語:ja"))

    history = tuple(言語状態(f"履歴{i}", "自然言語:ja") for i in range(256))
    文脈 = kernel.文脈化(言語状態("現在", "自然言語:ja"), history)

    return {
        "試験状態数": len(rows),
        "識別内部状態数": len(signatures),
        "試験言語体系数": 3,
        "一万文字状態受理": long_ok,
        "履歴深さ256受理": len(文脈.履歴) == 256,
        "明示固定文脈長上限": None,
        "観測": "意味集合だけでなく順序・関係構造を保持し、複数言語体系の開放状態を同じ模型核へ写せる",
    }


def _winner(kernel: MINIDORA模型核, current: str, candidates: tuple[str, str], *, history: tuple[str, ...] = ()) -> str | None:
    items = (
        成立候補("A", 言語状態(candidates[0], "自然言語:en")),
        成立候補("B", 言語状態(candidates[1], "自然言語:en")),
    )
    結果 = kernel.評価言語状態(
        言語状態(current, "自然言語:en"),
        items,
        履歴=tuple(言語状態(item, "自然言語:en") for item in history),
    )
    return 結果.最有力候補ID


def _関係域測定(kernel: MINIDORA模型核) -> dict[str, Any]:
    recognized: list[str] = []
    structure_signatures: set[tuple[object, ...]] = set()
    generated = 0

    for kind, phrase in _関係代表表層.items():
        sample = kernel.言語対応.内部化(言語状態(f"A {phrase} B", "自然言語:en"))
        if any(item.種別 == kind for item in sample.関係構造):
            recognized.append(kind)
        for i in range(32):
            generated += 1
            状態 = kernel.言語対応.内部化(
                言語状態(f"entity_{i} {phrase} target_{(i * 7) % 37}", "自然言語:en")
            )
            structure_signatures.add(状態.構造署名)

    direction = _winner(kernel, "A causes B", ("A causes B", "B causes A")) == "A"
    極性 = _winner(kernel, "A causes B", ("A causes B", "A does not cause B")) == "A"

    history_forward = _winner(kernel, "current", ("alpha", "beta"), history=("alpha", "beta"))
    history_reverse = _winner(kernel, "current", ("alpha", "beta"), history=("beta", "alpha"))
    history_order = history_forward == "B" and history_reverse == "A"

    condition_binding = _winner(
        kernel,
        "if catalyst, A causes B",
        ("if catalyst, A causes B", "if inhibitor, A causes B"),
    ) == "A"

    return {
        "模型関係実体数": len(kernel.関係群),
        "意味対応済み関係族数": len(recognized),
        "意味対応済み関係族": tuple(recognized),
        "関係構造生成試験数": generated,
        "識別関係構造数": len(structure_signatures),
        "方向差が成立差へ到達": direction,
        "肯否差が成立差へ到達": 極性,
        "履歴順序差が成立差へ到達": history_order,
        "条件結合差が成立差へ到達": condition_binding,
        "観測": "有限の関係作用素を任意の端点状態へ再利用し、17一般関係族・方向・肯否・履歴位置・条件結合の組合せを成立差へ反映する",
    }


def _共有適用測定(kernel: MINIDORA模型核) -> dict[str, Any]:
    passed = 0
    total = 256
    関係_ids = tuple(id(item) for item in kernel.関係群)
    for i in range(total):
        文脈 = 言語状態(f"topic_{i} causes result_{i % 19}", "自然言語:en")
        candidates = (
            成立候補("shared", 言語状態(f"topic_{i} causes result_{i % 19}", "自然言語:en")),
            成立候補("other", 言語状態(f"result_{i % 19} causes topic_{i}", "自然言語:en")),
        )
        結果 = kernel.評価言語状態(文脈, candidates)
        if 結果.最有力候補ID == "shared":
            passed += 1
        if tuple(id(item) for item in kernel.関係群) != 関係_ids:
            raise AssertionError("共有適用試験中に模型関係実体が交換された")

    return {
        "共有適用試験数": total,
        "同一関係群での成功数": passed,
        "成功率": passed / total,
        "関係実体再利用": passed == total,
        "観測": "同一の一般模型関係群を多数の異なる端点・文脈へ交換なしで再利用できる",
    }


def 規模測定(kernel: MINIDORA模型核 | None = None) -> 規模測定結果:
    模型 = kernel or 標準模型核()
    状態 = _状態域測定(模型)
    関係 = _関係域測定(模型)
    shared = _共有適用測定(模型)
    base_stats = 標準言語基底P.統計()

    状態_ok = (
        状態["識別内部状態数"] == 状態["試験状態数"]
        and 状態["一万文字状態受理"]
        and 状態["履歴深さ256受理"]
    )
    関係_ok = (
        関係["意味対応済み関係族数"] == len(_関係代表表層)
        and 関係["識別関係構造数"] == 関係["関係構造生成試験数"]
        and 関係["方向差が成立差へ到達"]
        and 関係["肯否差が成立差へ到達"]
        and 関係["履歴順序差が成立差へ到達"]
        and 関係["条件結合差が成立差へ到達"]
    )
    shared_ok = shared["関係実体再利用"] and shared["成功率"] == 1.0

    if 状態_ok and 関係_ok and shared_ok:
        status = "局所成立候補"
        reasons = (
            "状態域・関係域・共有適用規模の三面が、明示した比較集合に対して同時に開放的である",
            "関係域は17一般関係族を固定端点表ではなく再利用可能な有向・肯否・履歴・条件関係として多数状態へ適用する",
            "大規模は比較集合依存の相対記述であり、現代ニューラルLLMとの物理規模同等を主張しない",
            "一点閾値ではなく三面の観測結果と物理規模値を併記して判断する",
        )
    else:
        missing: list[str] = []
        if not 状態_ok:
            missing.append("状態域")
        if not 関係_ok:
            missing.append("関係域")
        if not shared_ok:
            missing.append("共有適用")
        status = "未成立"
        reasons = (
            "三つの規模面をまとめて大規模と記せる状態に未到達",
            "不足面:" + ",".join(missing),
            "一点閾値ではなく構造不足を理由に保留する",
        )

    return 規模測定結果(
        版=規模測定版,
        対象言語体系=("自然言語:ja", "自然言語:en", "program:python"),
        対象範囲='MINIDORA v0.4 標準模型核。外部参照R、HDS 構文化器、主体主幹、K3補助、計算実行器は模型核の規模へ加算しない',
        比較集合=(
            "固定ラベル・少数テンプレート・固定FAQ等の狭い言語処理系",
            "上流規定の2026年現代LLM参照群（構造比較。物理尺度の直接同一視はしない）",
        ),
        状態域規模=状態,
        関係域規模=関係,
        共有適用規模=shared,
        物理規模値={
            "模型関係実体数": len(模型.関係群),
            "英語関係語形数": 英語語形数(),
            **base_stats,
        },
        大規模性状態=status,
        理由=reasons,
    )


__all__ = ["規模測定版", "規模測定結果", "規模測定"]
