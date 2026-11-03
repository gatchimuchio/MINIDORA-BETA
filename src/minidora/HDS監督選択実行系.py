from __future__ import annotations

from dataclasses import dataclass, replace
from hashlib import sha256
import json
from typing import Callable

from .HDS選択実行系 import HDS選択実行結果, HDS選択推論実行
from .HDS中間表現 import HDSIR
from .HDS観測計画 import HDS参照観測要求群
from .hds介入制御 import (
    HDS介入制御,
    HDS介入記録,
    HDS指令種別,
    HDS監督状態,
    介入観測,
    既存作用,
    既存作用機会,
    既存判定,
    残差種別,
    標準HDS介入制御,
)
from .hds参照拡張 import HDS候補被覆優先統合, HDS追加参照検索
from .模型 import MINIDORA模型核
from .参照 import 参照供給器, 参照記録
from .計算実行器 import 計算実行器


_介入不能理由 = frozenset({
    'HDS_選択肢_SET_INCOMPLETE',
    'HDS_選択肢_LABEL_DUPLICATE',
    'HDS_選択肢_UNRESOLVED',
    'HDS_QUESTION_意味_LOSS',
    'HDS_K_QUESTION_意味_LOSS',
    'HDS_選択肢_COMPILE_FAILED',
    'HDS_選択肢_意味_LOSS',
})


def _hash(value: object) -> str:
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, default=str, separators=(",", ":"))
    return sha256(raw.encode("utf-8")).hexdigest()[:20]


def _choices(ir: HDSIR) -> dict[str, str]:
    return {
        coord.座標ID.split(":", 1)[1]: str(coord.内容)
        for coord in ir.座標
        if coord.座標ID.startswith('選択肢:')
    }


def _approved(結果: HDS選択実行結果) -> bool:
    return bool(結果.状態 == "APPROVE" and 結果.回答ラベル is not None)


def _residuals(
    結果: HDS選択実行結果,
    references: tuple[参照記録, ...],
) -> frozenset[残差種別]:
    """通常MINIDORAの観測結果から、HDSへ渡す異常種別だけを抽出する。

    APPROVE済みの通常推論はHDS介入の対象外とし、診断文字列を理由に再解釈しない。
    """
    if _approved(結果):
        return frozenset()

    joined = "\n".join(str(x) for x in 結果.理由)
    out: set[残差種別] = set()
    if 'QUESTION_意味_LOSS' in joined or 'HDS_K_QUESTION_意味_LOSS' in joined:
        out.add(残差種別.問題意味損失)
    if '選択肢_意味_LOSS' in joined:
        out.add(残差種別.候補意味損失)
    if '資料_COMPILE_PARTIAL' in joined or 結果.資料コンパイル失敗数 > 0:
        out.add(残差種別.資料意味損失)
    if any(字句 in joined for 字句 in (
        'NO_KNOWLEDGE_証拠',
        'NO_候補',
        "MINIDORA_OUTPUT_ABSENT",
        "NO_GUESS",
        '証拠_INSUFFICIENT',
    )):
        out.add(残差種別.観測不足)
    if 'AMBIGUOUS_証拠' in joined or "EXCEPTION_NOT_RESOLVED" in joined:
        out.add(残差種別.候補競合)
    if 'HDS_作用_DELTA_ATTACHED' in joined and 'HDS_作用_DELTA_CONSUMED' not in joined:
        out.add(残差種別.状態差未消費)
    if not references:
        out.add(残差種別.観測不足)
    if 結果.状態 == "FAIL":
        out.add(残差種別.未解残差)
    if not out:
        out.add(残差種別.候補識別不足)
    return frozenset(out)


@dataclass(frozen=True, slots=True)
class HDS監督選択結果:
    選択: HDS選択実行結果
    参照: tuple[参照記録, ...]
    HDS介入数: int
    HDS作用: tuple[str, ...]
    停止理由: tuple[str, ...] = ()


class _Session:
    """通常MINIDORAを単一主体として保持するHDS監督介入セッション。"""

    def __init__(
        self,
        question_ir: HDSIR,
        references: tuple[参照記録, ...],
        *,
        コンパイル,
        基礎能力核,
        模型核: MINIDORA模型核 | None,
        参照供給器: 参照供給器 | None,
        計算実行器_: 計算実行器 | None,
        初期選択: HDS選択実行結果 | None,
        評価実行: Callable[[tuple[参照記録, ...]], HDS選択実行結果] | None = None,
        参照観測要求=None,
        計算計画=None,
    ) -> None:
        self.question_ir = question_ir
        self.search_ir = question_ir
        self.参照観測要求 = tuple(参照観測要求) if 参照観測要求 is not None else HDS参照観測要求群(question_ir)
        self.references = tuple(references)
        self.コンパイル = コンパイル
        self.基礎能力核 = 基礎能力核
        self.模型核 = 模型核
        self.参照供給器 = 参照供給器
        self.計算実行器 = 計算実行器_
        self.選択肢_map = _choices(question_ir)
        self.generation = 0
        self.extra_r_level = 0
        self.compute_done = False
        self._compute_plan_checked = False
        self._compute_plan = 計算計画
        self.評価実行 = 評価実行
        self.initial = 初期選択 if 初期選択 is not None else self._normal()
        self.current = self.initial

    def _normal(
        self,
        *,
        作業: bool = True,
        local: bool = True,
        formal_模型: bool = True,
    ) -> HDS選択実行結果:
        # 追加参照・計算後も同じ統一評価入口へ戻し、初回だけ新経路になる二重構造を防ぐ。
        if self.評価実行 is not None:
            return self.評価実行(self.references)
        return HDS選択推論実行(
            self.question_ir,
            self.references,
            コンパイル=self.コンパイル,
            基礎能力核=None,
            模型核=self.模型核,
            正式模型評価=True,
        )

    def _計算機会(self):
        """上流Kernelが形成済みの計算降下物だけを消費する。原文を再コンパイルしない。"""
        if self._compute_plan_checked:
            return self._compute_plan
        self._compute_plan_checked = True
        plan = self._compute_plan
        if self.計算実行器 is None or plan is None:
            self._compute_plan = None
            return None
        compute_ir = getattr(plan, "計算IR", None)
        if bool(getattr(plan, "参照必須", True)) or not tuple(getattr(compute_ir, "命令列", ())):
            self._compute_plan = None
            return None
        return plan

    def _ref_sig(self) -> str:
        return _hash(tuple((r.識別子, r.信頼, r.条件) for r in self.references))

    def _候補署名(self) -> str:
        結果 = self.current
        return _hash((
            結果.状態,
            bool(結果.回答ラベル),
            tuple(sorted(set(結果.理由))),
            結果.資料コンパイル数,
            結果.資料コンパイル失敗数,
            結果.K証拠事実数,
            結果.検査点再活性数,
            結果.候補横断更新数,
        ))

    def residuals(self) -> frozenset[残差種別]:
        out = set(_residuals(self.current, self.references))
        if not _approved(self.current) and not self.compute_done and self._計算機会() is not None:
            out.add(残差種別.計算要求)
        return frozenset(out)

    def intervention_blockers(self) -> tuple[str, ...]:
        return tuple(reason for reason in self.current.理由 if reason in _介入不能理由)

    def 監督状態(self) -> HDS監督状態:
        結果 = self.current
        approved = _approved(結果)
        failed = 結果.状態 == "FAIL"
        direct = 'DIRECTED_関係_VERIFIED' in 結果.理由
        証拠 = bool(
            結果.K証拠事実数 > 0
            or (結果.K3結果 is not None and 結果.K3結果.根拠事実数 > 0)
            or '証拠_PRESENT' in 結果.理由
        )
        return HDS監督状態(
            既存判定.承認 if approved else 既存判定.失敗 if failed else 既存判定.保留,
            approved,
            direct,
            証拠,
            self._ref_sig(),
            self._候補署名(),
            frozenset() if approved else self.residuals(),
        )

    def offers(self) -> tuple[既存作用機会, ...]:
        residuals = self.residuals()
        base_sig = f"g{self.generation}:{self._ref_sig()}:{self._候補署名()}"
        offers: list[既存作用機会] = []

        if 残差種別.計算要求 in residuals and self._計算機会() is not None:
            offers.append(既存作用機会(
                既存作用.計算実行,
                frozenset({残差種別.計算要求, 残差種別.観測不足, 残差種別.候補識別不足}),
                f"compute:{base_sig}",
                1,
                True,
                ("GENERIC_COMPUTE_IR_AVAILABLE",),
            ))

        if self.参照供給器 is not None and residuals.intersection({
            残差種別.観測不足,
            残差種別.資料意味損失,
            残差種別.候補識別不足,
            残差種別.候補競合,
        }):
            offers.append(既存作用機会(
                既存作用.参照取得,
                frozenset({
                    残差種別.観測不足,
                    残差種別.資料意味損失,
                    残差種別.候補識別不足,
                    残差種別.候補競合,
                }),
                f"reference:{base_sig}:level{self.extra_r_level + 1}",
                4,
                True,
                ('NORMAL_MINIDORA_参照_EXPANSION_AVAILABLE',),
            ))
        return tuple(offers)

    def 作用を実行(self, 作用: 既存作用) -> bool:
        before = (
            self._ref_sig(),
            self._候補署名(),
            tuple(sorted(x.value for x in self.residuals())),
        )

        if 作用 == 既存作用.計算実行:
            plan = self._計算機会()
            if plan is None or self.計算実行器 is None:
                return False
            try:
                executed = self.計算実行器.計算実行(plan.計算IR, dict(plan.初期状態))
            except (ValueError, TypeError, ZeroDivisionError):
                return False
            output = executed.出力
            if output is None:
                return False
            record = 参照記録(
                識別子="compute:" + _hash((plan.計算IR.名称, plan.計算IR.版, plan.初期状態, output)),
                対象=self.question_ir.認知世界ID or "計算対象",
                内容=f"計算結果 {output}",
                由来="MINIDORA汎用計算実行",
                供給器="MINIDORA計算実行器",
                信頼=1.0,
                意味キー="計算結果",
                値=output,
                条件=(("hds_query_kind", "compute"),),
                意味確定=True,
            )
            if any(item.識別子 == record.識別子 for item in self.references):
                return False
            self.references = (*self.references, record)
            self.compute_done = True
            self.generation += 1
            self.current = self._normal()
        elif 作用 == 既存作用.参照取得:
            if self.参照供給器 is None:
                return False
            self.extra_r_level += 1
            observed = HDS追加参照検索(
                self.参照供給器,
                self.search_ir,
                段階=self.extra_r_level,
                観測要求=self.参照観測要求,
            )
            limit = max(len(self.references), len(observed))
            merged = HDS候補被覆優先統合(
                self.references,
                observed,
                tuple(self.選択肢_map),
                limit,
            )
            if tuple((x.識別子, x.条件) for x in merged) == tuple(
                (x.識別子, x.条件) for x in self.references
            ):
                return False
            self.references = merged
            self.generation += 1
            self.current = self._normal()
        else:
            return False

        after = (
            self._ref_sig(),
            self._候補署名(),
            tuple(sorted(x.value for x in self.residuals())),
        )
        return before != after

    def 最終結果(
        self,
        *,
        records: tuple[HDS介入記録, ...],
        stop_reasons: tuple[str, ...],
    ) -> HDS選択実行結果:
        # HDS介入がなかった場合、通常MINIDORA結果を1bitも再解釈しない。
        if not records:
            return self.initial

        reasons = [
            *self.current.理由,
            "HDS_SUPERVISORY_INTERVENTION",
            f"HDS_SUPERVISORY_INTERVENTIONS:{len(records)}",
        ]
        reasons.extend('HDS_INTERVENTION_作用:' + row.作用.value for row in records)
        reasons.extend(stop_reasons)
        return replace(
            self.current,
            理由=tuple(dict.fromkeys(reasons)),
        )


def HDS監督選択実行(
    question_ir: HDSIR,
    references: tuple[参照記録, ...],
    *,
    コンパイル,
    基礎能力核=None,
    模型核: MINIDORA模型核 | None = None,
    参照供給器: 参照供給器 | None = None,
    計算実行器_: 計算実行器 | None = None,
    HDS制御: HDS介入制御 | None = None,
    HDS介入予算: int = 6,
    初期選択: HDS選択実行結果 | None = None,
    評価実行: Callable[[tuple[参照記録, ...]], HDS選択実行結果] | None = None,
    参照観測要求=None,
    計算計画=None,
) -> HDS監督選択結果:
    """HDSをMINIDORAフィードバックループの監督介入層として実行する。

    通常MINIDORAが閉包した場合は完全透過する。HDSは未閉包・競合・観測不足などの
    異常時だけ既存作用を起動し、作用後は必ず通常MINIDORAへ制御を戻す。
    """
    session = _Session(
        question_ir,
        references,
        コンパイル=コンパイル,
        基礎能力核=基礎能力核,
        模型核=模型核,
        参照供給器=参照供給器,
        計算実行器_=計算実行器_,
        初期選択=初期選択,
        評価実行=評価実行,
        参照観測要求=参照観測要求,
        計算計画=計算計画,
    )

    if HDS制御 is None or _approved(session.initial):
        return HDS監督選択結果(session.initial, session.references, 0, (), ())

    records: list[HDS介入記録] = []
    stop_reasons: tuple[str, ...] = session.intervention_blockers()

    while not stop_reasons:
        状態 = session.監督状態()
        if 状態.既存判定 == 既存判定.承認 and 状態.出力存在:
            break

        observation = 介入観測(
            状態,
            session.offers(),
            tuple(records),
            max(0, int(HDS介入予算) - len(records)),
        )
        directive = HDS制御.判定(observation)
        if directive.種別 == HDS指令種別.不介入:
            break
        if directive.種別 == HDS指令種別.停止要求 or directive.作用 is None:
            stop_reasons = directive.理由 or ("HDS_SUPERVISORY_STOP_REQUEST",)
            break

        offer = next(
            (
                item
                for item in observation.作用機会
                if item.作用 == directive.作用
                and set(directive.対象残差).issubset(item.解消対象)
            ),
            None,
        )
        if offer is None:
            stop_reasons = ("HDS_DIRECTIVE_NOT_BACKED_BY_EXISTING_OFFER",)
            break

        progressed = session.作用を実行(directive.作用)
        records.append(
            HDS介入記録(
                directive.作用,
                offer.作用入力署名,
                directive.対象残差,
                progressed,
            )
        )

        if _approved(session.current):
            break
        blockers = session.intervention_blockers()
        if blockers:
            stop_reasons = blockers
            break

    selection = session.最終結果(records=tuple(records), stop_reasons=stop_reasons)
    return HDS監督選択結果(
        selection,
        session.references,
        len(records),
        tuple(row.作用.value for row in records),
        stop_reasons,
    )


__all__ = ["HDS監督選択結果", "HDS監督選択実行", "標準HDS介入制御"]
