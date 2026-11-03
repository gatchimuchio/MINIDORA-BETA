from __future__ import annotations

from dataclasses import dataclass
from typing import Callable


@dataclass(frozen=True, slots=True)
class HDS非退行判定:
    """既知の基準出力を下限として、拡張結果の採否を分離する。

    基準が既に承認済みなら、その結果を完全保持する。
    基準が未承認の場合だけ拡張候補を検討し、明示的な追加採用証明が無ければ基準を維持する。
    gold、ベンチマーク固有規則、候補正解情報には依存しない。
    """

    出力: object
    基準結果: object
    拡張結果: object | None
    基準固定: bool
    拡張採用: bool
    理由: tuple[str, ...]


def HDS非退行包絡(
    基準結果: object,
    *,
    基準承認判定: Callable[[object], bool],
    拡張実行: Callable[[], object] | None = None,
    拡張承認判定: Callable[[object], bool] | None = None,
    拡張採用証明: Callable[[object, object], bool] | None = None,
) -> HDS非退行判定:
    """基準承認を壊さず、証明された拡張だけを採用する非退行包絡。

    規則:
    - 基準承認済み: 拡張処理を実行せず基準結果をそのまま返す。
    - 基準未承認: 拡張処理を実行できる。
    - 拡張採用: `拡張承認判定` と `拡張採用証明` の両方が真の場合だけ。
    - 証明が無い拡張はshadow結果として保持できるが、外向き出力は基準のまま。
    """

    if not callable(基準承認判定):
        raise TypeError("基準承認判定はcallableである必要がある")
    if bool(基準承認判定(基準結果)):
        return HDS非退行判定(
            基準結果,
            基準結果,
            None,
            True,
            False,
            ("HDS_BASELINE_APPROVAL_LOCKED",),
        )

    if 拡張実行 is None:
        return HDS非退行判定(
            基準結果,
            基準結果,
            None,
            True,
            False,
            ("HDS_BASELINE_UNAPPROVED_NO_EXTENSION",),
        )
    if not callable(拡張実行):
        raise TypeError("拡張実行はcallableである必要がある")

    拡張結果 = 拡張実行()
    承認 = bool(拡張承認判定(拡張結果)) if callable(拡張承認判定) else False
    証明 = bool(拡張採用証明(基準結果, 拡張結果)) if callable(拡張採用証明) else False
    if 承認 and 証明:
        return HDS非退行判定(
            拡張結果,
            基準結果,
            拡張結果,
            False,
            True,
            ("HDS_EXTENSION_ADOPTED_WITH_PROOF",),
        )

    理由 = ["HDS_BASELINE_FLOOR_PRESERVED"]
    if not 承認:
        理由.append("HDS_EXTENSION_NOT_APPROVED")
    if not 証明:
        理由.append("HDS_EXTENSION_PROOF_MISSING")
    return HDS非退行判定(
        基準結果,
        基準結果,
        拡張結果,
        True,
        False,
        tuple(理由),
    )


def HDS証拠優越包絡(
    基準結果: object,
    拡張結果: object,
    *,
    基準承認判定: Callable[[object], bool],
    拡張承認判定: Callable[[object], bool],
    証拠優越証明: Callable[[object, object], bool],
) -> HDS非退行判定:
    """承認済み基準を床として保持し、明示的な証拠優越時だけ更新する。

    従来の `HDS非退行包絡` は変更しない。承認済み基準の再検証を明示的に要求する
    呼出側だけが本関数を使い、拡張承認と証拠優越証明の双方が真の時だけ更新する。
    """
    for 名, 関数 in (
        ("基準承認判定", 基準承認判定),
        ("拡張承認判定", 拡張承認判定),
        ("証拠優越証明", 証拠優越証明),
    ):
        if not callable(関数):
            raise TypeError(名 + "はcallableである必要がある")

    if not bool(基準承認判定(基準結果)):
        return HDS非退行包絡(
            基準結果,
            基準承認判定=基準承認判定,
            拡張実行=lambda: 拡張結果,
            拡張承認判定=拡張承認判定,
            拡張採用証明=証拠優越証明,
        )

    拡張承認 = bool(拡張承認判定(拡張結果))
    優越証明 = bool(証拠優越証明(基準結果, 拡張結果)) if 拡張承認 else False
    if 拡張承認 and 優越証明:
        return HDS非退行判定(
            拡張結果,
            基準結果,
            拡張結果,
            False,
            True,
            ("HDS_APPROVED_BASELINE_REPLACED_WITH_SUPERIOR_EVIDENCE",),
        )

    理由 = ["HDS_APPROVED_BASELINE_PRESERVED"]
    if not 拡張承認:
        理由.append("HDS_EXTENSION_NOT_APPROVED")
    if not 優越証明:
        理由.append("HDS_SUPERIOR_EVIDENCE_PROOF_MISSING")
    return HDS非退行判定(
        基準結果,
        基準結果,
        拡張結果,
        True,
        False,
        tuple(理由),
    )


__all__ = ["HDS非退行判定", "HDS非退行包絡", "HDS証拠優越包絡"]
