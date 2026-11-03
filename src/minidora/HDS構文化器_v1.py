from __future__ import annotations

from dataclasses import replace
from typing import Sequence

from .選択意図 import HDS選択意図判定
from .HDS適合器 import HDS文脈
from .HDS構文化器 import 公開HDS基礎コンパイラ as _基礎HDSコンパイラ
from .HDS構文化器 import 公開HDSコンパイラ方針
from .HDS構文化作用差分 import HDS作用差分構造生成
from .HDS構文化監査中間表現 import HDS監査参照IR射影
from .HDS構文化動態 import HDS状態遷移IR射影, HDS状態遷移抽出
from .HDS構文化失敗 import HDSチェックリスト生成, HDS失敗署名候補生成, HDS監査参照候補生成
from .HDS構文化失敗集 import HDS失敗署名Bank
from .HDS構文化前処理 import 公開HDSフロントエンド射影, 公開HDS詳細成果
from .HDS構文化履歴 import HDS認知世界差分IR射影, HDS認知世界差分生成
from .HDS構文化処理系列_v1_4 import (
    HDSカーネル束,
    HDSコンパイル束,
    HDS選択コンパイル束,
    HDS意味IR化,
    HDS意味専用計画器,
    HDS計算コンパイル成果,
    HDS計算降下バックエンド,
)
from .HDS構文化記録 import HDS構文化器成果
from .HDS構文化記録_v1_2 import HDS失敗署名BankSnapshot, HDS抽出規則改善候補
from .HDS構文化記録_v1_3 import HDS作用差分構造
from .HDS構文化暗黙知 import HDS暗黙知IR射影, HDS暗黙知抽出
from .HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from .HDSコア入力 import HDSコア入力束
from .HDSコア入力射影 import HDSコア入力へ
from .HDS観測計画 import HDS参照観測要求群
from .HDS候補検証契約 import HDS候補検証契約群
from .HDS数量計算契約 import HDS数量計算契約を形成, HDS数量法則観測要求群
from .HDS言語協調 import HDS英語AND展開
from .HDS言語関係 import HDS英語基底関係射影
from .HDS言語範囲 import HDS英語関係範囲射影
from .HDS言語意味橋渡し import HDS英日意味射影
from .HDS意味主題射影 import HDS問い主題射影
from .HDS会話意味射影 import HDS会話意味を射影, 会話制御文を分ける, 会話入力区分を保持, 会話射影の対象
from .HDS会話文節 import HDS会話文節を形成
from .意味字句 import 意味語
from .言語 import 言語計画
from .言語基底 import 言語基底P, 標準言語基底P


class _意味基礎HDSコンパイラ(_基礎HDSコンパイラ):
    '旧基礎構文化器の意味抽出だけを利用し、計算計画を無作用化する内部前段。'

    def __init__(self, 親: "公開HDSコンパイラ") -> None:
        super().__init__(親.方針)
        self._親 = 親
        self._legacy = HDS意味専用計画器()

    def _入力言語(self, text: str) -> str:
        return self._親._入力言語(text)


class 公開HDSコンパイラ(_基礎HDSコンパイラ):
    """MINIDORAの唯一の公開Compiler Kernel。

    原文の意味決定は ``コンパイル束`` / ``問題コンパイル束`` で一度だけ行い、
    Core入力・R観測要求・計算P・作用差分は同じ意味IRから形成する。
    下流はKernel成果を消費し、同一原文を意味決定のために再コンパイルしない。
    """

    構造版 = "v1.3"
    処理系列版 = "v2.0"
    コア入力版 = "HDS-コア入力-v1"
    規定言語 = "日本語"
    基底言語 = "日本語"
    基底言語コード = "ja"

    def __init__(self, 方針: 公開HDSコンパイラ方針 | None = None, 言語基底P_: 言語基底P | None = None) -> None:
        super().__init__(方針)
        self.言語基底P = 言語基底P_ or 標準言語基底P
        self._計算計画器 = self._legacy
        self._意味基礎 = _意味基礎HDSコンパイラ(self)
        self._計算降下 = HDS計算降下バックエンド()

    def _入力言語(self, text: str) -> str:
        return self.言語基底P.入力言語判定(text)

    def _完成(self, base: HDSIR, *, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None) -> HDS構文化器成果:
        文節 = None
        if base.実行核.作用 != "HDS_選択肢_selection" and 会話射影の対象(base):
            文節 = HDS会話文節を形成(base, 文脈=文脈)
        base = 文節 if 文節 is not None else HDS会話意味を射影(base, 文脈=文脈)
        base = HDS英日意味射影(base)
        base = HDS英語基底関係射影(base, self.言語基底P)
        base = HDS英語AND展開(base)
        base = HDS英語関係範囲射影(base)
        base = HDS問い主題射影(base, 上限=self.方針.最大主題語数)

        first = 公開HDSフロントエンド射影(base)
        関係図 = HDS状態遷移抽出(first.IR.正規化文 or first.IR.原文)
        作用_delta = HDS作用差分構造生成(関係図)
        ir = HDS状態遷移IR射影(first.IR, 関係図)
        tacit = HDS暗黙知抽出(ir.正規化文 or ir.原文)
        ir = HDS暗黙知IR射影(ir, tacit)

        refreshed = 公開HDS詳細成果(ir)
        signatures = HDS失敗署名候補生成(refreshed.IR, refreshed.認知世界)
        checklist = HDSチェックリスト生成(refreshed.監査要求, signatures)
        audit_queries = HDS監査参照候補生成(refreshed.IR, checklist)
        audit_ir = HDS監査参照IR射影(refreshed.IR, audit_queries)
        world_diff = HDS認知世界差分生成(audit_ir, HDS履歴)
        final_ir = HDS認知世界差分IR射影(audit_ir, world_diff)
        return replace(
            refreshed,
            IR=final_ir,
            状態遷移=関係図,
            暗黙知構造=tacit,
            失敗署名候補=signatures,
            チェックリスト=checklist,
            認知世界差分=world_diff,
            監査参照候補=audit_queries,
            作用差分構造=作用_delta,
        )

    def _意味基礎IR(
        self,
        入力: str,
        *,
        前回結果: object = None,
        HDS履歴: tuple[HDSIR, ...] = (),
        文脈: HDS文脈 | None = None,
    ) -> tuple[HDSIR, 言語計画]:
        制御文, 提示本文, 本文範囲 = 会話制御文を分ける(str(入力))
        normalized = self._正規化(制御文)
        文脈_focus = getattr(文脈, "現在焦点", None) if 文脈 is not None else 前回結果
        plan = self._計算計画器.計画(normalized, 文脈参照=文脈_focus)
        base = self._意味基礎.コンパイル(
            制御文,
            前回結果=前回結果,
            HDS履歴=HDS履歴,
            文脈=文脈,
        )
        if 提示本文:
            base = replace(base, 原文=str(入力), 正規化文=normalized)
            base = 会話入力区分を保持(base, 制御文, 提示本文, 本文範囲)
        return HDS意味IR化(base, plan), plan

    def _意味束(
        self,
        入力: str,
        *,
        前回結果: object = None,
        HDS履歴: tuple[HDSIR, ...] = (),
        文脈: HDS文脈 | None = None,
    ) -> tuple[HDSコンパイル束, HDS構文化器成果]:
        """原文を一度だけ意味決定し、全下流成果を同じ意味IRから形成する。"""
        意味_base, plan = self._意味基礎IR(
            入力,
            前回結果=前回結果,
            HDS履歴=HDS履歴,
            文脈=文脈,
        )
        detailed = self._完成(意味_base, HDS履歴=HDS履歴, 文脈=文脈)
        意味_ir = replace(detailed.IR, 手順=None, 初期状態={})
        detailed = replace(detailed, IR=意味_ir)
        コア入力 = HDSコア入力へ(意味_ir)
        数量契約 = HDS数量計算契約を形成(意味_ir, None, plan)
        観測要求 = tuple((*HDS数量法則観測要求群(意味_ir, 数量契約), *HDS参照観測要求群(意味_ir)))
        return HDSコンパイル束(
            意味IR=意味_ir,
            計算計画=plan,
            コア入力=コア入力,
            参照観測要求=観測要求,
            候補検証契約=HDS候補検証契約群(意味_ir, 観測要求),
            数量計算契約=数量契約,
            失敗署名候補=detailed.失敗署名候補,
            チェックリスト=detailed.チェックリスト,
            認知世界差分=detailed.認知世界差分,
            監査参照候補=detailed.監査参照候補,
            作用差分構造=detailed.作用差分構造,
        ), detailed

    def _コア意味IR(self, 入力: str, **kwargs) -> HDSIR:
        return self.コンパイル束(入力, **kwargs).意味IR

    def _コア入力正本(self, 入力: str, **kwargs) -> HDSコア入力束:
        return self.コンパイル束(入力, **kwargs).コア入力

    def 意味コンパイル(self, 入力: str, *, 前回結果: object = None, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None) -> HDSIR:
        bundle, _ = self._意味束(入力, 前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈)
        return bundle.意味IR

    def コア入力コンパイル(self, 入力: str, *, 前回結果: object = None, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None) -> HDSコア入力束:
        return self.コンパイル束(入力, 前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈).コア入力

    def 作用差分コンパイル(self, 入力: str, *, 前回結果: object = None, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None) -> HDS作用差分構造:
        bundle, _ = self._意味束(入力, 前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈)
        return bundle.作用差分構造

    def コンパイル束(self, 入力: str, *, 前回結果: object = None, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None) -> HDSコンパイル束:
        bundle, _ = self._意味束(入力, 前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈)
        return bundle

    def 観測要求コンパイル(self, 入力: str, *, 前回結果: object = None, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None):
        return self.コンパイル束(入力, 前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈).参照観測要求

    def 計算降下(self, bundle: HDSカーネル束) -> HDS計算コンパイル成果:
        return self._計算降下.降下(bundle)

    def 計算コンパイル(self, 入力: str, *, 前回結果: object = None, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None) -> HDS計算コンパイル成果:
        return self.計算降下(self.コンパイル束(入力, 前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈))

    def コンパイル(self, 入力: str, *, 前回結果: object = None, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None) -> HDSIR:
        return self.コンパイル束(入力, 前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈).互換IR()

    def 詳細コンパイル(self, 入力: str, *, 前回結果: object = None, HDS履歴: tuple[HDSIR, ...] = (), 文脈: HDS文脈 | None = None) -> HDS構文化器成果:
        _, detailed = self._意味束(入力, 前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈)
        return detailed

    @staticmethod
    def _問い関係を持つ(ir: HDSIR) -> bool:
        for 関係 in ir.関係:
            for raw in 関係.条件:
                key, sep, payload = str(raw).partition("=")
                if sep and key.strip() == "不足位置" and payload.strip() in {"始点", "終点"}:
                    return True
        return False

    def _選択問題問い閉包(self, ir: HDSIR, question: str) -> HDSIR:
        if self._問い関係を持つ(ir):
            return ir
        choices = tuple(coord for coord in ir.座標 if coord.座標ID.startswith('選択肢:'))
        if len(choices) < 2:
            return ir
        text = self._正規化(str(question))
        if not text:
            return ir
        intent = HDS選択意図判定(question)
        focus = self._正規化(str(intent.焦点 or text))
        content = frozenset(意味語(focus)) - {"find", "calculate", "determine", "identify", "select", "choose", "all"}
        if not content and intent.種別 != "EXCEPTION":
            return ir
        existing_coord_ids = {coord.座標ID for coord in ir.座標}
        existing_関係_ids = {関係.関係ID for 関係 in ir.関係}
        def unique(base: str, existing: set[str]) -> str:
            候補 = base
            serial = 1
            while 候補 in existing:
                候補 = f"{base}:{serial}"
                serial += 1
            existing.add(候補)
            return 候補
        未知_id = unique('selection-query:未知', existing_coord_ids)
        known_id = unique("selection-query:surface", existing_coord_ids)
        関係_id = unique('selection-query:関係', existing_関係_ids)
        selection = "反転" if intent.種別 == "EXCEPTION" else "通常"
        coords = (
            *ir.座標,
            HDS座標(未知_id, "目的.未知始点", "選択肢", 値状態.未観測, 由来="選択問題構造", 暫定性="SELECTION_QUERY_GENERIC_CLOSURE"),
            HDS座標(known_id, "対象.問い本文", focus, 値状態.確定, 由来="選択問題構造", 暫定性="SELECTION_QUERY_GENERIC_CLOSURE"),
        )
        関係 = HDS関係(
            関係_id, (未知_id,), (known_id,), "問い適合",
            条件=("検索述語=match", "不足位置=始点", "選択問題閉包=v0.1", f"選択意図={selection}"),
            値状態=値状態.未観測, 由来="選択問題構造", 暫定性="SELECTION_QUERY_GENERIC_CLOSURE",
        )
        residuals = tuple(残差 for 残差 in ir.残差 if not (str(残差.残差ID) == "lang-sem:question-loss" and str(残差.種別) == '意味_loss'))
        return replace(ir, 座標=coords, 関係=(*ir.関係, 関係), 残差=residuals)

    def _問題基礎(self, question: str, choices: Sequence[str]) -> tuple[HDSIR, 言語計画]:
        if len(choices) < 2:
            raise ValueError("選択問題には2件以上の候補が必要")
        if len(choices) > 26:
            raise ValueError('公開構文化器の選択ラベル上限は26件')
        base, plan = self._意味基礎IR(question)
        選択肢_coords = tuple(
            HDS座標(f"選択肢:{chr(ord('A') + index)}", "目的.候補", str(text), 値状態.確定, 由来="選択問題入力")
            for index, text in enumerate(choices)
        )
        return replace(
            base,
            座標=base.座標 + 選択肢_coords,
            参照必須=True,
            種別="knowledge_query",
            実行核=HDS実行核('HDS_選択肢_selection', (), "結果", 境界=("NO_GUESS", "gold非参照", 'HDS-意味'), 検証=("全候補対称", "計算P非内包")),
            手順=None,
            初期状態={},
            閉包状態='CLOSED_FOR_意味_TRANSFER',
        ), plan

    def 問題コンパイル束(self, question: str, choices: Sequence[str]) -> HDS選択コンパイル束:
        base, plan = self._問題基礎(question, choices)
        detailed = self._完成(base)
        ir = self._選択問題問い閉包(replace(detailed.IR, 手順=None, 初期状態={}), question)
        候補意味IR = tuple(
            (chr(ord("A") + index), self._意味束(str(text))[0].意味IR)
            for index, text in enumerate(choices)
        )
        数量契約 = HDS数量計算契約を形成(ir, dict(候補意味IR), plan)
        観測要求 = tuple((*HDS数量法則観測要求群(ir, 数量契約), *HDS参照観測要求群(ir)))
        return HDS選択コンパイル束(
            意味IR=ir, 計算計画=plan, コア入力=HDSコア入力へ(ir),
            参照観測要求=観測要求,
            候補意味IR=候補意味IR,
            候補検証契約=HDS候補検証契約群(ir, 観測要求),
            数量計算契約=数量契約,
            失敗署名候補=detailed.失敗署名候補,
            チェックリスト=detailed.チェックリスト,
            認知世界差分=detailed.認知世界差分,
            監査参照候補=detailed.監査参照候補,
            作用差分構造=detailed.作用差分構造,
        )

    def 問題IR(self, question: str, choices: Sequence[str]) -> HDSIR:
        return self.問題コンパイル束(question, choices).意味IR

    def 問題コア入力(self, question: str, choices: Sequence[str]) -> HDSコア入力束:
        return self.問題コンパイル束(question, choices).コア入力

    def 問題観測要求(self, question: str, choices: Sequence[str]):
        return self.問題コンパイル束(question, choices).参照観測要求

    def 詳細問題IR(self, question: str, choices: Sequence[str]) -> HDS構文化器成果:
        base, _plan = self._問題基礎(question, choices)
        detailed = self._完成(base)
        return replace(detailed, IR=self._選択問題問い閉包(detailed.IR, question))

    def 失敗帰還(self, 成果: HDS構文化器成果, Bank: HDS失敗署名Bank, *, Run参照: str) -> HDS失敗署名BankSnapshot:
        return Bank.観測(成果.失敗署名候補, Run参照=Run参照)

    def 改善候補(self, Bank: HDS失敗署名Bank) -> tuple[HDS抽出規則改善候補, ...]:
        return Bank.snapshot().改善候補


__all__ = [
    "公開HDSコンパイラ方針", "公開HDSコンパイラ", 'HDS構文化器成果', "HDS失敗署名Bank",
    "HDSカーネル束", "HDSコンパイル束", "HDS選択コンパイル束", "HDS計算コンパイル成果",
]
