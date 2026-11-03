from __future__ import annotations

from hashlib import sha256

from .HDS構文化記録 import HDS監査要求, HDS認知世界断片
from .HDS構文化記録_v1_1 import (
    HDSチェックリスト項目,
    HDS失敗署名候補,
    HDS監査参照候補,
)
from .HDS中間表現 import HDSIR, 値状態


_関門_MAP: dict[str, tuple[str, ...]] = {
    "座標固定要求": ("G00",),
    "未定義・未解参照要求": ("G01",),
    "閉包要求": ("G02",),
    "不可能性要求": ("G03",),
    "反論対称性要求": ("G04",),
    "反論強度要求": ("G05",),
    "証拠要求": ("G06",),
    "論証要求": ("G07", "G08"),
    "投影境界要求": ("G09", "G15", "G18", "G19"),
    "可逆性要求": ("G10",),
    "時間帰属要求": ("G11", "G24"),
    "資源要求": ("G12", "G21"),
    "原理探索要求": ("G13",),
    "保持要求": ("G17", "G20"),
    "意味損失要求": ("G18",),
    "暫定性要求": ("G23", "G25"),
    "自己適用要求": ("G26",),
    "最終採否委譲": ("G27",),
}

_STOP_RECOVERY: dict[str, tuple[str, ...]] = {
    "座標固定要求": ("未固定座標が判断に必要ならSUSPEND候補", "観測または明示入力で再開"),
    "未定義・未解参照要求": ("暗黙補完せずSUSPEND候補", "参照先または意味境界を取得して再開"),
    "閉包要求": ("X/R/Mcの必要部分が閉じなければSUSPEND候補", "不足閉包だけを追加観測"),
    "不可能性要求": ("可能性だけでASSERTへ進めない", "反例・不成立条件・対称候補をPROBE"),
    "反論対称性要求": ("片側だけの反論適用で確定しない", "元主張と反論を同型再監査"),
    "反論強度要求": ("弱い一般論だけで強い観測命題を棄却しない", "証拠・具体性・射程を比較"),
    "証拠要求": ("必要証拠が無ければSUSPENDまたはPROBE候補", "独立出典・観測条件・反証を取得"),
    "論証要求": ("循環・飛躍が未解消ならSUSPEND候補", "前提・推論・結論を分離して再監査"),
    "投影境界要求": ('射影を世界本体へ昇格しない', '表現限界・残差・別射影を保持'),
    "可逆性要求": ("不可逆作用はrollback条件なしで自動実行しない", '検査点・撤回条件・回復コストを固定'),
    "時間帰属要求": ("結果帰属が不明なら単一因果へ確定しない", "遅延・外生変化・観測誤差をPROBE"),
    "資源要求": ("保持契約を満たせない場合はHOLD/SUSPEND候補", "資源追加または可逆分割実行"),
    "原理探索要求": ("パターン/機構候補を原理へ自動昇格しない", "反対モデル・反証・摂動・反実仮想へ送る"),
    "保持要求": ("不可逆剪定しない", "全座標・全関係・残差・旧解釈を保存"),
    "意味損失要求": ("無損失を装わない", '残差へ損失と影響を記録'),
    "暫定性要求": ("局所安定をFINALへ昇格しない", "再開放条件を保持"),
    "自己適用要求": ("HDS自身を自己例外化しない", "旧版保持と改訂可能性を確認"),
    "最終採否委譲": ('構文化器自身はCOMMITしない', "判断境界へ委譲"),
}


def HDS関門対応(要求種別: str) -> tuple[str, ...]:
    return _関門_MAP.get(str(要求種別), ())


def _sig_id(kind: str, symptom: str) -> str:
    digest = sha256(f"{kind}\n{symptom}".encode("utf-8")).hexdigest()[:12]
    return f"fs:{digest}"


def HDS失敗署名候補生成(ir: HDSIR, world: HDS認知世界断片) -> tuple[HDS失敗署名候補, ...]:
    signatures: list[HDS失敗署名候補] = []

    if world.未固定座標:
        symptom = "未固定:" + ",".join(world.未固定座標)
        signatures.append(
            HDS失敗署名候補(
                _sig_id("coordinate_unfixed", symptom),
                "coordinate_unfixed",
                symptom,
                "監査対象の基底座標が局所閉包されていない",
                起動条件=world.未固定座標,
                影響範囲=("CognitiveWorld", "論証閉包", "R query"),
                違反前提=("未固定座標を暗黙補完しない",),
                回復=("不足座標を観測または明示する",),
                次探索軸=world.未固定座標,
                再利用チェック=("G00 Coordinate Gate", "G02 Closure Gate"),
            )
        )

    for 残差 in ir.残差:
        if 残差.種別 == '意味_loss':
            failure_class = '意味_loss_failure'
            cause = "有限射影で意味・関係・文脈の一部が失われた"
            gates = ("G18 Semantic Loss Gate",)
        elif "遷移" in 残差.種別:
            failure_class = '関係_failure'
            cause = "状態遷移の端点または条件が未固定"
            gates = ("G00 Coordinate Gate", "G11 Temporal Attribution Gate")
        elif "未解" in 残差.種別 or "未閉包" in 残差.種別:
            failure_class = "closure_failure"
            cause = "参照・関係・閉包条件の一部が未解決"
            gates = ("G01 Open-Term Gate", "G02 Closure Gate")
        else:
            continue
        symptom = f"{残差.種別}:{残差.原文}"
        signatures.append(
            HDS失敗署名候補(
                _sig_id(failure_class, symptom),
                failure_class,
                symptom,
                cause,
                起動条件=(残差.理由,),
                影響範囲=tuple(残差.影響座標),
                回復=tuple(残差.解消条件) or ("追加観測または再射影",),
                次探索軸=('残差',),
                再利用チェック=gates,
            )
        )

    blocked = tuple(coord.座標ID for coord in ir.座標 if coord.値状態 in {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保} and not str(coord.種別).startswith(("監査.", "保持.", "暫定性.", "帰還.")))
    if blocked and not any(sig.失敗分類 == "closure_failure" for sig in signatures):
        symptom = "阻害座標:" + ",".join(blocked)
        signatures.append(
            HDS失敗署名候補(
                _sig_id("closure_failure", symptom),
                "closure_failure",
                symptom,
                "局所判断に必要となり得る座標が未確定状態",
                起動条件=blocked,
                影響範囲=blocked,
                回復=("対象Runで必要な座標だけ追加観測する",),
                次探索軸=("closure",),
                再利用チェック=("G02 Closure Gate",),
            )
        )

    return tuple(signatures)


def HDSチェックリスト生成(requirements: tuple[HDS監査要求, ...], signatures: tuple[HDS失敗署名候補, ...]) -> tuple[HDSチェックリスト項目, ...]:
    out: list[HDSチェックリスト項目] = []
    signature_by_class = {sig.失敗分類: sig for sig in signatures}
    for index, requirement in enumerate(requirements):
        linked: HDS失敗署名候補 | None = None
        if requirement.種別 in {"座標固定要求", "閉包要求", "未定義・未解参照要求"}:
            linked = signature_by_class.get("coordinate_unfixed") or signature_by_class.get("closure_failure")
        elif requirement.種別 == "意味損失要求":
            linked = signature_by_class.get('意味_loss_failure')
        gates = HDS関門対応(requirement.種別)
        question = f"{requirement.種別}: {requirement.理由}"
        out.append(
            HDSチェックリスト項目(
                f"check:{index:03d}",
                linked.署名ID if linked else None,
                question,
                requirement.必要情報,
                gates,
                _STOP_RECOVERY.get(requirement.種別, ("未充足ならSUSPENDまたはPROBE候補",)),
                requirement.次の観測候補 or requirement.必要情報,
            )
        )
    return tuple(out)


def HDS監査参照候補生成(ir: HDSIR, checklist: tuple[HDSチェックリスト項目, ...]) -> tuple[HDS監査参照候補, ...]:
    coords = ir.座標辞書()
    focus = next((str(coord.内容) for coord in ir.座標 if str(coord.種別) == "目的.検索焦点" and coord.値状態 not in {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}), "")
    base = " ".join((focus or ir.正規化文 or ir.原文).split()).strip()
    if not base:
        return ()
    if len(base) > 220:
        base = base[-220:]
    言語 = (ir.入力言語 or "ja").casefold()
    ja = 言語.startswith("ja")
    suffixes: list[tuple[str, str, tuple[str, ...], int]] = []
    for item in checklist:
        gates = set(item.関門対応)
        if "G03" in gates:
            suffixes.extend((("反例" if ja else "counterexample", "impossibility_counterexample", item.関門対応, 100), ("不成立条件" if ja else "failure conditions", "impossibility_conditions", item.関門対応, 95)))
        if "G04" in gates or "G05" in gates:
            suffixes.append(("代替説明" if ja else "alternative explanation", "countermodel", item.関門対応, 85))
        if "G06" in gates:
            suffixes.append(("証拠" if ja else '証拠', '証拠', item.関門対応, 80))
        if "G13" in gates:
            suffixes.append(("機構 境界条件" if ja else "mechanism boundary conditions", "principle_probe", item.関門対応, 75))
    out: list[HDS監査参照候補] = []
    seen: set[str] = set()
    for suffix, kind, gates, 優先度 in sorted(suffixes, key=lambda item: -item[3]):
        query = f"{base} {suffix}".strip()
        key = query.casefold()
        if key in seen:
            continue
        seen.add(key)
        out.append(HDS監査参照候補(query, kind, gates, kind, 優先度))
        if len(out) >= 5:
            break
    return tuple(out)


__all__ = [
    'HDS関門対応',
    "HDS失敗署名候補生成",
    "HDSチェックリスト生成",
    "HDS監査参照候補生成",
]
