from __future__ import annotations
from functools import wraps
from types import ModuleType
from .HDS中間表現 import 値状態
from .科学専門能力 import 科学専門能力解決
_阻害状態 = frozenset({値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保})

def _選択肢(question_ir) -> tuple[tuple[str, str, 値状態], ...]:
    rows: list[tuple[str, str, 値状態]] = []
    for coord in question_ir.座標:
        if not str(coord.座標ID).startswith('選択肢:'):
            continue
        rows.append((str(coord.座標ID).split(':', 1)[1], str(coord.内容), coord.値状態))
    return tuple(sorted(rows, key=lambda item: item[0]))

def 科学専門能力を通常MINIDORAへ接続(実行系_モジュール: ModuleType) -> None:
    """健全化済み科学能力を通常MINIDORAの既存能力として接続する。

    HDSへ回答生成・勝者選択を渡さない。科学能力が候補を一意かつ絶対支持できる
    場合だけ通常MINIDORAが直接閉包し、それ以外は従来経路へ完全透過する。
    """
    original = 実行系_モジュール.HDS選択推論実行
    if bool(getattr(original, '_minidora_scientific_能力_v1', False)):
        return

    @wraps(original)
    def wrapped(question_ir, references, *args, **kwargs):
        if 実行系_モジュール.HDS選択問題(question_ir):
            choices = _選択肢(question_ir)
            if len(choices) >= 2 and not any(状態 in _阻害状態 for _, _, 状態 in choices):
                contents = tuple(content for _, content, _ in choices)
                resolved = 科学専門能力解決(str(question_ir.原文), contents)
                if resolved is not None and 0 <= resolved.index < len(choices):
                    label, content, _ = choices[resolved.index]
                    return 実行系_モジュール.HDS選択実行結果(
                        状態='APPROVE',
                        回答ラベル=label,
                        回答内容=content,
                        理由=(
                            'MINIDORA_EXISTING_SCIENTIFIC_能力',
                            'SCIENTIFIC_能力_NO_GUESS_GUARD_PASSED',
                            'SCIENTIFIC_能力_解決器:' + str(resolved.解決器),
                        ),
                        K3結果=None,
                        候補コンパイル数=0,
                        資料コンパイル数=0,
                        資料コンパイル失敗数=0,
                        K追加事実数=0,
                        K証拠事実数=0,
                        K証拠阻害事実数=0,
                        専門作用起動数=1,
                    )
        return original(question_ir, references, *args, **kwargs)

    setattr(wrapped, '_minidora_scientific_能力_v1', True)
    setattr(wrapped, '_minidora_scientific_能力_original', original)
    実行系_モジュール.HDS選択推論実行 = wrapped

__all__ = ['科学専門能力を通常MINIDORAへ接続']
