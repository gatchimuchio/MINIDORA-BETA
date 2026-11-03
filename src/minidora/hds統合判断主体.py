from __future__ import annotations

from dataclasses import dataclass, replace
from enum import StrEnum
from hashlib import sha256

from .HDS選択実行系 import HDS選択実行結果, HDS選択問題
from .HDS中間表現 import HDSIR, 値状態


class HDS作用種別(StrEnum):
    参照観測 = '参照'
    候補計算 = "EVALUATE"
    確定 = "COMMIT"
    留保 = "SUSPEND"
    停止 = "STOP"


@dataclass(frozen=True, slots=True)
class HDS作用要求:
    作用: HDS作用種別
    理由: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class MINIDORA認知世界:
    '一回のMINIDORA判断Runに限定したCognitiveWorld 射影。'

    run_id: str
    対象: str
    委任目的: str
    HDS_IR: HDSIR
    参照利用可能: bool
    参照必須: bool
    作用予算: int
    状態: str = "OPEN"
    版: int = 0
    参照試行済み: bool = False
    参照数: int = 0
    評価状態: str | None = None
    評価回答ラベル: str | None = None
    評価回答内容: str | None = None
    残差: tuple[str, ...] = ()
    作用履歴: tuple[tuple[str, tuple[str, ...]], ...] = ()
    暫定性: str = "原則暫定"
    再開放条件: tuple[str, ...] = ("新観測", "未解残差", "評価非承認", "委任境界変更")


_BLOCKING = frozenset({値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保})


def _入力阻害理由(ir: HDSIR) -> tuple[str, ...]:
    reasons: list[str] = []
    if not HDS選択問題(ir):
        reasons.append('MINIDORA_HDS_範囲_NOT_選択肢')
    if any(item.種別 == '意味_loss' for item in ir.残差):
        reasons.append('HDS_QUESTION_意味_LOSS')
    for item in ir.座標:
        if item.座標ID.startswith('選択肢:') and item.値状態 in _BLOCKING:
            reasons.append(f"HDS_CHOICE_{item.値状態.value}:{item.座標ID}")
    return tuple(reasons)


class MINIDORAHDS判断主体:
    'MINIDORA領域へ有限射影したHDS Judgement 主体。\n\n    候補生成系はPROPOSEまで。COMMIT/SUSPEND/STOPはこの主体だけが確定する。\n    HDS Framework Kernel全体やAGI全体を実装したとは主張しない。\n    '

    版 = 'v1-bounded-domain-射影'

    def 開始(
        self,
        ir: HDSIR,
        *,
        委任目的: str = "入力に対して根拠を捏造せず、MINIDORAの一回の言語判断を局所閉包する",
        参照利用可能: bool = False,
        参照必須: bool = False,
        作用予算: int = 6,
    ) -> MINIDORA認知世界:
        if 作用予算 < 1:
            raise ValueError("HDS判断主体の作用予算は1以上である必要がある")
        seed = f"{ir.認知世界ID}|{ir.正規化文}|{委任目的}"
        return MINIDORA認知世界(
            run_id=sha256(seed.encode("utf-8")).hexdigest()[:16],
            対象=ir.原文,
            委任目的=委任目的,
            HDS_IR=ir,
            参照利用可能=bool(参照利用可能),
            参照必須=bool(参照必須),
            作用予算=int(作用予算),
        )

    def 次作用(self, 世界: MINIDORA認知世界) -> HDS作用要求:
        if 世界.状態 in {"COMMITTED", "SUSPENDED", "STOPPED"}:
            return HDS作用要求(HDS作用種別.停止, (f"TERMINAL:{世界.状態}",))
        blockers = _入力阻害理由(世界.HDS_IR)
        if blockers:
            return HDS作用要求(HDS作用種別.留保, blockers)
        if len(世界.作用履歴) >= 世界.作用予算:
            return HDS作用要求(HDS作用種別.留保, ('HDS_作用_予算_EXHAUSTED',))
        if 世界.参照必須 and not 世界.参照利用可能 and not 世界.参照試行済み:
            return HDS作用要求(HDS作用種別.留保, ('HDS_REQUIRED_参照_UNAVAILABLE',))
        if 世界.参照利用可能 and not 世界.参照試行済み:
            return HDS作用要求(HDS作用種別.参照観測, ("OBSERVATION_BEFORE_COMMIT", '参照_AVAILABLE'))
        if 世界.評価状態 is None:
            return HDS作用要求(HDS作用種別.候補計算, ('COMPUTE_候補_DIFFERENCE', "NO_SELF_COMMIT"))
        if 世界.評価状態 == "PROPOSE" and 世界.評価回答ラベル is not None and 世界.評価回答内容 is not None:
            return HDS作用要求(
                HDS作用種別.確定,
                ('候補_GENERATION_SEPARATED_FROM_COMMIT', "LOCAL_CLOSURE_SUPPORTED"),
            )
        reasons = tuple(dict.fromkeys(世界.残差 + ("HDS_EVALUATION_NOT_COMMITTABLE",)))
        return HDS作用要求(HDS作用種別.留保, reasons)

    def 参照帰還(self, 世界: MINIDORA認知世界, *, 参照数: int, 理由: tuple[str, ...] = ()) -> MINIDORA認知世界:
        details = tuple(reason for reason in 理由 if reason) or (f"REFERENCE_COUNT:{int(参照数)}",)
        return replace(
            世界,
            版=世界.版 + 1,
            参照試行済み=True,
            参照数=max(0, int(参照数)),
            作用履歴=世界.作用履歴 + ((HDS作用種別.参照観測.value, details),),
        )

    def 評価帰還(self, 世界: MINIDORA認知世界, 結果: HDS選択実行結果) -> MINIDORA認知世界:
        residuals = 世界.残差
        if 結果.状態 != "PROPOSE":
            residuals = tuple(dict.fromkeys(residuals + tuple(結果.理由)))
        return replace(
            世界,
            版=世界.版 + 1,
            評価状態=結果.状態,
            評価回答ラベル=結果.回答ラベル,
            評価回答内容=結果.回答内容,
            残差=residuals,
            作用履歴=世界.作用履歴 + ((HDS作用種別.候補計算.value, tuple(結果.理由)),),
        )

    def 確定(self, 世界: MINIDORA認知世界) -> MINIDORA認知世界:
        request = self.次作用(世界)
        if request.作用 != HDS作用種別.確定:
            raise ValueError(f"HDS判断主体がCOMMIT可能状態ではない: {request.作用.value}")
        return replace(
            世界,
            版=世界.版 + 1,
            状態="COMMITTED",
            作用履歴=世界.作用履歴 + ((HDS作用種別.確定.value, request.理由),),
        )

    def 留保(self, 世界: MINIDORA認知世界, 理由: tuple[str, ...]) -> MINIDORA認知世界:
        reasons = tuple(dict.fromkeys(tuple(理由) + 世界.残差)) or ("HDS_SUSPEND",)
        return replace(
            世界,
            版=世界.版 + 1,
            状態="SUSPENDED",
            残差=reasons,
            作用履歴=世界.作用履歴 + ((HDS作用種別.留保.value, reasons),),
        )

    def 再開放(self, 世界: MINIDORA認知世界, 理由: str) -> MINIDORA認知世界:
        reason = str(理由).strip()
        if not reason:
            raise ValueError("再開放理由は空にできない")
        return replace(
            世界,
            版=世界.版 + 1,
            状態="OPEN",
            評価状態=None,
            評価回答ラベル=None,
            評価回答内容=None,
            残差=tuple(dict.fromkeys(世界.残差 + (f"REOPEN:{reason}",))),
            作用履歴=世界.作用履歴 + (("REOPEN", (reason,)),),
        )


__all__ = ["HDS作用種別", "HDS作用要求", "MINIDORA認知世界", "MINIDORAHDS判断主体"]
