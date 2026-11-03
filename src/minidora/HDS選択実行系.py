from __future__ import annotations

from dataclasses import replace
from functools import wraps

from . import HDS選択実行系_v24 as _基準
from .模型核局所観測 import MINIDORA局所観測view

# 24点系で監査済みの公開/内部APIをそのまま再公開する。
# private helperも他の現行moduleが明示importしているため、dunder以外は保持する。
for _name in dir(_基準):
    if not _name.startswith("__"):
        globals()[_name] = getattr(_基準, _name)

_基準選択推論 = _基準.HDS選択推論実行


@wraps(_基準選択推論)
def HDS選択推論実行(*args, **kwargs):
    '24点系formal 模型核を壊さず、未閉包時だけ局所観測viewを一度再照合する。\n\n    不変条件:\n    - 既存APPROVEは同一objectを完全透過する。\n    - 追加資料取得、gold、外部評価固有規則を使わない。\n    - 同一情報源 identityを保持する。\n    - SUSPENDから新たにAPPROVEへ閉包できた時だけ新結果を採用する。\n    '
    initial = _基準選択推論(*args, **kwargs)

    if len(args) < 2:
        return initial
    question_ir = args[0]
    references = tuple(args[1])

    attached_模型_模型核 = kwargs.get("模型核") or getattr(kwargs.get("基礎能力核"), '_minidora_模型_模型核', None)
    formal = bool(kwargs.get("正式模型評価", False) or attached_模型_模型核 is not None)
    local_enabled = kwargs.get("局所再照合", True) is not False

    # 既に閉包した24点系の結果には一切触れない。
    if not formal or not local_enabled or not references:
        return initial
    if initial.状態 == "APPROVE" and initial.回答ラベル is not None:
        return initial

    local_references, changed = MINIDORA局所観測view(
        question_ir,
        references,
        上限=max(0, int(kwargs.get("最大局所Window数", 12))),
    )
    if changed <= 0:
        return initial

    recheck_kwargs = dict(kwargs)
    recheck_kwargs["局所再照合"] = False
    recheck_args = (question_ir, local_references, *args[2:])
    rechecked = _基準選択推論(*recheck_args, **recheck_kwargs)

    # 追加閉包だけを採用する。閉じなければ24点系の元結果を完全維持する。
    if not (
        rechecked.状態 == "APPROVE"
        and rechecked.回答ラベル is not None
        and rechecked.回答内容 is not None
    ):
        return initial

    reasons = tuple(dict.fromkeys((
        *tuple(rechecked.理由),
        "FORMAL_LOCAL_OBSERVATION_VIEW",
        "FORMAL_LOCAL_VIEW_RECHECK_SELECTED",
        f"FORMAL_LOCAL_VIEW_SOURCE_COUNT:{changed}",
    )))
    return replace(
        rechecked,
        理由=reasons,
        局所Window数=changed,
        局所再照合数=1,
    )


__all__ = tuple(
    name
    for name in dir(_基準)
    if not name.startswith("_")
) + ("HDS選択推論実行",)
