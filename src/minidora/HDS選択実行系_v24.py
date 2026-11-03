from __future__ import annotations

from collections.abc import Callable, Mapping, Sequence
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from typing import TYPE_CHECKING

from .HDS適合器 import HDS独立コンパイル
from .HDS候補意味正本 import HDS候補意味IR辞書
from .HDS選択仮説 import HDS候補代入仮説群
from .HDS中間表現 import HDSIR, 値状態
from .HDS模型射影 import HDSMINIDORA模型評価
from .hds入力参照境界 import HDS入力資料本文, HDS入力資料整列
from .HDS実行系射影 import (
    HDSK資料射影, HDSK候補代入可能, HDSK候補射影, HDSK質問射影, HDS模型候補代入可能,
)
from .参照 import 参照記録
from .模型 import MINIDORA模型核, 模型結果

if TYPE_CHECKING:
    from .K3機能 import K3相当能力核
    from .K3_HDSネイティブ import HDSK3結果


HDSコンパイル関数 = Callable[[str], HDSIR]
_BLOCKING = {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}
_QUERY_PROVENANCE_KEYS = {'hds_query_選択肢', "hds_query_kind"}


@dataclass(frozen=True, slots=True)
class HDS選択実行結果:
    状態: str
    回答ラベル: str | None
    回答内容: str | None
    理由: tuple[str, ...]
    K3結果: HDSK3結果 | None
    候補コンパイル数: int
    資料コンパイル数: int
    資料コンパイル失敗数: int
    K追加事実数: int
    K証拠事実数: int
    K証拠阻害事実数: int
    コンパイル並列: bool = False
    コンパイル最大並列: int = 1
    作業関係生成数: int = 0
    作業関係再利用数: int = 0
    作業関係K昇格数: int = 0
    作業関係再検証後破棄数: int = 0
    検査点数: int = 0
    検査点再活性数: int = 0
    大域再照合数: int = 0
    候補横断更新数: int = 0
    専門作用起動数: int = 0
    遍歴後SUSPEND数: int = 0
    一時証拠数: int = 0
    局所Window数: int = 0
    局所Windowコンパイル数: int = 0
    局所Windowコンパイル失敗数: int = 0
    局所Window追加事実数: int = 0
    局所再照合数: int = 0
    MINIDORA模型結果: 模型結果 | None = None


def HDS選択問題(ir: HDSIR) -> bool:
    if ir.手順 is not None:
        return False
    return sum(1 for coord in ir.座標 if coord.座標ID.startswith('選択肢:')) >= 2


def _choices(ir: HDSIR) -> tuple[tuple[str, str, 値状態], ...]:
    rows: list[tuple[str, str, 値状態]] = []
    for coord in ir.座標:
        if not coord.座標ID.startswith('選択肢:'):
            continue
        rows.append((coord.座標ID.split(":", 1)[1], str(coord.内容), coord.値状態))
    return tuple(sorted(rows, key=lambda item: item[0]))


def _suspend(
    reason: str,
    *,
    候補_count: int = 0,
    資料_fail: int = 0,
    parallel: bool = False,
    workers: int = 1,
) -> HDS選択実行結果:
    return HDS選択実行結果(
        "SUSPEND", None, None, (reason,), None,
        候補_count, 0, 資料_fail, 0, 0, 0,
        parallel, workers,
    )


def _コンパイラ実体(compile_fn: HDSコンパイル関数):
    'bound methodから公開HDS 構文化器実体を取り出す。推測で生成しない。'
    owner = getattr(compile_fn, "__self__", None)
    if owner is None:
        return None
    構文化器 = getattr(owner, "HDSコンパイラ", None)
    if 構文化器 is not None:
        return 構文化器
    if callable(getattr(owner, "詳細コンパイル", None)):
        return owner
    return None


def _独立コンパイル入口(compile_fn: HDSコンパイル関数) -> tuple[HDSコンパイル関数, bool]:
    owner = getattr(compile_fn, "__self__", None)
    構文化器 = getattr(owner, "HDSコンパイラ", None)
    if 構文化器 is None:
        return compile_fn, bool(getattr(compile_fn, "並列安全", False))
    return (
        lambda text: HDS独立コンパイル(構文化器, text),
        bool(getattr(構文化器, "並列安全", False)),
    )


def _一括コンパイル(
    compile_fn,
    texts: Sequence[str],
    *,
    parallel: bool,
    max_workers: int,
) -> tuple[object | Exception, ...]:
    if not texts:
        return ()
    if not parallel or len(texts) <= 1 or max_workers <= 1:
        out: list[object | Exception] = []
        for text in texts:
            try:
                out.append(compile_fn(text))
            except Exception as exc:
                out.append(exc)
        return tuple(out)

    workers = min(max(1, int(max_workers)), len(texts))
    with ThreadPoolExecutor(max_workers=workers, thread_name_prefix="minidora-hds") as executor:
        futures = [executor.submit(compile_fn, text) for text in texts]
        out: list[object | Exception] = []
        for future in futures:
            try:
                out.append(future.result())
            except Exception as exc:
                out.append(exc)
        return tuple(out)


def _参照作用差分群(
    compile_fn: HDSコンパイル関数,
    references: Sequence[参照記録],
    *,
    parallel: bool,
    max_workers: int,
) -> tuple[tuple[object, ...], int]:
    '通常資料意味コンパイル成功済みの参照だけを詳細コンパイルし、作用差分構造を回収する。'
    構文化器 = _コンパイラ実体(compile_fn)
    detailed = getattr(構文化器, "詳細コンパイル", None)
    if not callable(detailed) or not references:
        return (), 0

    payloads = _一括コンパイル(
        lambda text: detailed(text).作用差分構造,
        [HDS入力資料本文(record) for record in references],
        parallel=parallel,
        max_workers=max_workers,
    )
    structures: list[object] = []
    failed = 0
    for payload in payloads:
        if isinstance(payload, Exception):
            failed += 1
            continue
        if getattr(payload, "状態差", ()) or getattr(payload, "後続利用", ()):
            structures.append(payload)
    return tuple(structures), failed


def _専門作用起動数(結果: 模型結果) -> int:
    '構文化器作用差分を実際に消費した証拠件数だけを数える。'
    total = 0
    for row in 結果.候補差:
        for contribution in row.寄与:
            if contribution.関係名.startswith("候補共同参照:状態差連結"):
                total += len(tuple(contribution.根拠))
    return total


def _参照provenance(record: 参照記録) -> tuple[str, ...]:
    markers: list[str] = []
    for key, value in record.条件:
        k = str(key)
        if k not in _QUERY_PROVENANCE_KEYS:
            continue
        if k == 'hds_query_選択肢':
            markers.append('query_選択肢:' + str(value))
        elif k == "hds_query_kind":
            markers.append("query_kind:" + str(value))
    return tuple((record.供給器, record.由来, record.識別子, *dict.fromkeys(markers)))


def _直接関係で再判定(
    judgment_ir: HDSIR,
    verification_候補_irs: dict[str, HDSIR],
    作業: K3相当能力核,
    k3: HDSK3結果,
) -> HDSK3結果:
    '旧v0.3 K3 補助器診断だけに使う。正式回答を上書きしない。'
    from .選択意図 import HDS選択意図判定
    from .HDS直接関係検証 import HDS直接関係検証
    from .K3機能 import 意味Frame
    from .K3_HDSネイティブ import HDSK3結果
    if HDS選択意図判定(judgment_ir.原文).種別 == "EXCEPTION":
        return k3
    direct, _diagnostics = HDS直接関係検証(作業, verification_候補_irs)
    if direct is None:
        return k3

    frame = 意味Frame(
        kind="question",
        intent="knowledge_query",
        raw=judgment_ir.原文,
        predicate='HDS_選択肢_selection',
        args=(None,),
        tags=("HDS-IR", '選択肢', 'directed_関係_verification'),
        言語=getattr(judgment_ir, "入力言語", "en") or "en",
    )
    decision = 作業.J.decide(frame, (direct,))
    if decision.status != "APPROVE" or decision.selected_候補 is None:
        return k3

    selected = decision.selected_候補.answer
    candidates = (direct, *tuple(候補 for 候補 in k3.候補 if 候補.answer != selected))
    reasons = tuple((*decision.reason_codes, 'DIRECTED_関係_VERIFIED'))
    return HDSK3結果(
        decision.status, selected, decision, candidates, len(set(direct.proof_fact_ids)), reasons,
        k3.努力水準, k3.探索深さ上限, k3.証拠上限, k3.候補診断,
    )


def _検証候補群(
    k_question_ir: HDSIR,
    full_candidates: dict[str, HDSIR],
    k_candidates: dict[str, HDSIR],
) -> dict[str, HDSIR]:
    substitutable = {
        label: k_candidates[label]
        for label, full_ir in full_candidates.items()
        if label in k_candidates and HDSK候補代入可能(full_ir)
    }
    substituted = HDS候補代入仮説群(k_question_ir, substitutable)
    return {label: substituted.get(label, 候補_ir) for label, 候補_ir in k_candidates.items()}


def _正式模型候補群(
    k_question_ir: HDSIR,
    full_candidates: dict[str, HDSIR],
    k_candidates: dict[str, HDSIR],
) -> dict[str, HDSIR]:
    '正式模型だけで使う質問型aware候補代入。旧補助器経路へは流さない。'
    substitutable = {
        label: k_candidates[label]
        for label, full_ir in full_candidates.items()
        if label in k_candidates and HDS模型候補代入可能(k_question_ir, full_ir)
    }
    substituted = HDS候補代入仮説群(k_question_ir, substitutable)
    return {label: substituted.get(label, 候補_ir) for label, 候補_ir in k_candidates.items()}


def _候補強度(結果: HDSK3結果) -> tuple[int, int, float, int, float]:
    diagnostics = tuple(結果.候補診断)
    if not diagnostics:
        return (0, 0, 0.0, 0, 0.0)
    ordered = sorted(diagnostics, key=lambda item: (-item.合計得点, item.候補))
    top = next((item for item in diagnostics if item.候補 == 結果.回答ラベル), ordered[0])
    second = max((item.合計得点 for item in diagnostics if item.候補 != top.候補), default=0.0)
    margin = top.合計得点 - second
    return (top.独立出典数, top.識別一致出典数, margin, top.根拠事実数, top.合計得点)


def _再作用結果選択(initial: HDSK3結果, rechecked: HDSK3結果) -> HDSK3結果:
    '旧補助器内の診断比較。正式MINIDORA模型結果の採否には使わない。'
    if 'DIRECTED_関係_VERIFIED' in initial.理由:
        return initial
    if rechecked.状態 != "APPROVE" or rechecked.回答ラベル is None:
        return initial
    if initial.状態 != "APPROVE" or initial.回答ラベル is None:
        return rechecked
    if rechecked.回答ラベル == initial.回答ラベル:
        return rechecked
    if 'DIRECTED_関係_VERIFIED' in rechecked.理由:
        return rechecked
    return rechecked if _候補強度(rechecked) > _候補強度(initial) else initial


def HDS選択推論実行(
    question_ir: HDSIR,
    references: tuple[参照記録, ...],
    *,
    コンパイル: HDSコンパイル関数,
    基礎能力核: K3相当能力核,
    候補意味IR: Mapping[str, HDSIR] | None = None,
    努力: str | None = None,
    最大コンパイル並列: int = 4,
    作業再作用: bool = True,
    局所再照合: bool = True,
    最大局所Window数: int = 12,
    模型核: MINIDORA模型核 | None = None,
    正式模型評価: bool = False,
) -> HDS選択実行結果:
    """選択問題をR→HDS→MINIDORA C→HDS J、または旧互換経路で評価する。"""
    choices = _choices(question_ir)
    if len(choices) < 2:
        return _suspend('HDS_選択肢_SET_INCOMPLETE')
    labels = [label for label, _, _ in choices]
    if len(set(labels)) != len(labels):
        return _suspend('HDS_選択肢_LABEL_DUPLICATE')
    if any(状態 in _BLOCKING for _, _, 状態 in choices):
        return _suspend('HDS_選択肢_UNRESOLVED')
    if any(残差.種別 == '意味_loss' for 残差 in question_ir.残差):
        return _suspend('HDS_QUESTION_意味_LOSS')

    compile_isolated, parallel_safe = _独立コンパイル入口(コンパイル)
    worker_count = min(max(1, int(最大コンパイル並列)), max(1, len(choices), len(references))) if parallel_safe else 1

    正本候補 = HDS候補意味IR辞書(候補意味IR, labels)
    if 正本候補 is None:
        選択肢_payloads = _一括コンパイル(
            compile_isolated, [content for _, content, _ in choices], parallel=parallel_safe, max_workers=worker_count,
        )
        候補_irs: dict[str, HDSIR] = {}
        for (label, _, _), compiled in zip(choices, 選択肢_payloads):
            if isinstance(compiled, Exception):
                return _suspend('HDS_選択肢_COMPILE_FAILED', 候補_count=len(候補_irs), parallel=parallel_safe, workers=worker_count)
            if any(残差.種別 == '意味_loss' for 残差 in compiled.残差):
                return _suspend('HDS_選択肢_意味_LOSS', 候補_count=len(候補_irs) + 1, parallel=parallel_safe, workers=worker_count)
            候補_irs[label] = compiled
    else:
        候補_irs = 正本候補
        for label in labels:
            compiled = 候補_irs[label]
            if any(残差.種別 == '意味_loss' for 残差 in compiled.残差):
                return _suspend('HDS_選択肢_意味_LOSS', 候補_count=len(候補_irs), parallel=parallel_safe, workers=worker_count)

    k_question_ir = HDSK質問射影(question_ir)
    if any(残差.種別 == '意味_loss' for 残差 in k_question_ir.残差):
        return _suspend('HDS_K_QUESTION_意味_LOSS', 候補_count=len(候補_irs), parallel=parallel_safe, workers=worker_count)

    k_候補_irs = {label: HDSK候補射影(候補_ir) for label, 候補_ir in 候補_irs.items()}
    attached_模型_模型核 = 模型核 or getattr(基礎能力核, '_minidora_模型_模型核', None)
    use_formal_模型 = bool(正式模型評価 or attached_模型_模型核 is not None)
    evaluation_候補_irs = (
        _正式模型候補群(k_question_ir, 候補_irs, k_候補_irs)
        if use_formal_模型
        else _検証候補群(k_question_ir, 候補_irs, k_候補_irs)
    )

    資料_payloads = _一括コンパイル(
        compile_isolated, [HDS入力資料本文(record) for record in references], parallel=parallel_safe, max_workers=worker_count,
    )
    資料_bundle = HDS入力資料整列(references, 資料_payloads, HDSK資料射影)
    資料_irs = list(資料_bundle.IR群)
    資料_compiled = len(資料_bundle.IR群)
    資料_failed = 資料_bundle.失敗数

    選択肢_map = {label: content for label, content, _ in choices}

    if use_formal_模型:
        if attached_模型_模型核 is None:
            return _suspend(
                'MINIDORA_模型_模型核_NOT_CONFIGURED',
                候補_count=len(候補_irs),
                資料_fail=資料_failed,
                parallel=parallel_safe,
                workers=worker_count,
            )
        作用_structures, 作用_failed = _参照作用差分群(
            コンパイル,
            資料_bundle.成功記録群,
            parallel=parallel_safe,
            max_workers=worker_count,
        )
        formal = HDSMINIDORA模型評価(
            k_question_ir,
            evaluation_候補_irs,
            tuple(資料_irs),
            模型核=attached_模型_模型核,
            参照識別子=資料_bundle.出典ID群,
            参照信頼=資料_bundle.信頼群,
            作用差分構造群=作用_structures,
        )
        content = 選択肢_map.get(formal.回答ラベル) if formal.回答ラベル is not None else None
        reasons = list(formal.理由)
        reasons.append('FORMAL_模型_模型核_WITH_HDS_J')
        if 資料_failed:
            reasons.append(f"資料_COMPILE_PARTIAL:{資料_failed}")
        if 作用_failed:
            reasons.append(f"ACTION_DELTA_COMPILE_PARTIAL:{作用_failed}")
        stats = formal.模型結果.統計
        specialist_count = _専門作用起動数(formal.模型結果)
        return HDS選択実行結果(
            formal.状態, formal.回答ラベル, content, tuple(dict.fromkeys(reasons)), None,
            len(候補_irs), 資料_compiled, 資料_failed, 0, 0, 0, parallel_safe, worker_count,
            0, 0, 0, 0,
            len(formal.模型結果.検査点),
            int(stats.検査点再活性数),
            int(stats.大域再照合数),
            int(stats.候補横断更新数),
            specialist_count,
            int(formal.状態 != "APPROVE"),
            0, 0, 0, 0, 0, 0,
            formal.模型結果,
        )

    if 基礎能力核 is None:
        return _suspend(
            'LEGACY_補助器_NOT_CONFIGURED',
            候補_count=len(候補_irs),
            資料_fail=資料_failed,
            parallel=parallel_safe,
            workers=worker_count,
        )

    from .HDS資料K import HDSIR知識適合器, HDS証拠状態複製
    from .HDS作業状態 import (
        HDS一時証拠統合,
        HDS作業状態構築,
        HDS候補共同状態更新,
        HDS寄与関門再照合,
    )
    from .hds局所再照合 import HDS局所Window候補
    from .K3_HDSネイティブ import HDSIRネイティブ適合器

    verification_候補_irs = evaluation_候補_irs
    作業模型核 = 基礎能力核.clone()
    HDS証拠状態複製(基礎能力核, 作業模型核)
    ingest = HDSIR知識適合器(作業模型核)
    added = 0
    証拠 = 0
    blocked = 0
    for record, compiled in zip(references, 資料_payloads):
        if isinstance(compiled, Exception):
            continue
        結果 = ingest.投入(HDSK資料射影(compiled), provenance=_参照provenance(record), 信頼係数=record.信頼)
        added += 結果.追加事実数
        証拠 += 結果.証拠事実数
        blocked += 結果.証拠阻害事実数

    作業 = HDS作業状態構築(作業模型核)
    initial = HDSIRネイティブ適合器(作業模型核).実行(k_question_ir, 候補IR=verification_候補_irs, 努力=努力)
    initial = _直接関係で再判定(question_ir, verification_候補_irs, 作業模型核, initial)
    HDS候補共同状態更新(作業, initial.候補診断, 段階='候補_INITIAL')

    temporary = HDS寄与関門再照合(作業) if 作業再作用 else ()
    local_windows = ()
    if 局所再照合 and 'DIRECTED_関係_VERIFIED' not in initial.理由:
        local_windows = HDS局所Window候補(question_ir, references, 上限=max(0, int(最大局所Window数)))

    local_compiled = 0
    local_failed = 0
    local_added = 0
    local_reconciliations = 0
    legacy_k3 = initial
    legacy_rechecked_selected = False
    legacy_rechecked_override = False

    if temporary or local_windows:
        rechecked_模型核 = 作業模型核.clone()
        HDS証拠状態複製(作業模型核, rechecked_模型核)
        changed = 0
        if temporary:
            changed += HDS一時証拠統合(rechecked_模型核, temporary)
        if local_windows:
            local_payloads = _一括コンパイル(
                compile_isolated, [row.内容 for row in local_windows], parallel=parallel_safe, max_workers=worker_count,
            )
            local_ingest = HDSIR知識適合器(rechecked_模型核)
            for row, compiled in zip(local_windows, local_payloads):
                if isinstance(compiled, Exception):
                    local_failed += 1
                    continue
                結果 = local_ingest.投入(HDSK資料射影(compiled), provenance=_参照provenance(row.参照), 信頼係数=row.参照.信頼)
                local_compiled += 1
                local_added += 結果.追加事実数
                changed += 結果.証拠事実数

        if changed > 0:
            作業.統計.検査点再活性数 += 1
            作業.統計.大域再照合数 += 1
            local_reconciliations = 1 if local_compiled > 0 else 0
            rechecked = HDSIRネイティブ適合器(rechecked_模型核).実行(k_question_ir, 候補IR=verification_候補_irs, 努力=努力)
            rechecked = _直接関係で再判定(question_ir, verification_候補_irs, rechecked_模型核, rechecked)
            HDS候補共同状態更新(作業, rechecked.候補診断, 段階='候補_RECHECK')
            selected = _再作用結果選択(initial, rechecked)
            legacy_rechecked_selected = selected is rechecked
            legacy_rechecked_override = bool(
                legacy_rechecked_selected and initial.回答ラベル is not None and rechecked.回答ラベル is not None
                and initial.回答ラベル != rechecked.回答ラベル
            )
            if not legacy_rechecked_selected and rechecked.回答ラベル != initial.回答ラベル:
                作業.統計.作業関係再検証後破棄数 += len(temporary)
            legacy_k3 = selected

    if legacy_k3.状態 != "APPROVE":
        作業.統計.遍歴後SUSPEND数 = 1
    content = 選択肢_map.get(legacy_k3.回答ラベル) if legacy_k3.回答ラベル is not None else None
    reasons = list(legacy_k3.理由)
    reasons.append("LEGACY_V03_SELECTION_PATH")
    if temporary:
        reasons.append('LEGACY_作業_RECHECK')
    if local_compiled:
        reasons.append("LEGACY_LOCAL_WINDOW_RECHECK")
    if legacy_rechecked_selected:
        reasons.append("LEGACY_RECHECK_SELECTED")
    if legacy_rechecked_override:
        reasons.append("LEGACY_RECHECK_OVERRIDE")
    if 資料_failed:
        reasons.append(f"資料_COMPILE_PARTIAL:{資料_failed}")
    if local_failed:
        reasons.append(f"LOCAL_WINDOW_COMPILE_PARTIAL:{local_failed}")

    stats = 作業.統計
    return HDS選択実行結果(
        legacy_k3.状態, legacy_k3.回答ラベル, content, tuple(dict.fromkeys(reasons)), legacy_k3,
        len(候補_irs), 資料_compiled, 資料_failed, added, 証拠, blocked, parallel_safe, worker_count,
        stats.作業関係生成数, stats.作業関係再利用数, stats.作業関係K昇格数, stats.作業関係再検証後破棄数,
        stats.検査点数, stats.検査点再活性数, stats.大域再照合数, stats.候補横断更新数,
        stats.専門作用起動数, stats.遍歴後SUSPEND数, stats.一時証拠数, len(local_windows), local_compiled,
        local_failed, local_added, local_reconciliations, None,
    )


__all__ = ["HDS選択実行結果", "HDS選択問題", "HDS選択推論実行"]
