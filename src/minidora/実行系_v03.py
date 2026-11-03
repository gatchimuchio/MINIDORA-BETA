from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, Iterable, Mapping, Sequence

from .HDS適合器 import HDSコンパイラProtocol, HDS文脈
from .HDS選択実行系 import HDS選択実行結果, HDS選択問題, HDS選択推論実行
from .HDS中間表現 import HDSIR
from .HDS参照 import HDS参照予算選択, HDS参照検索
from .HDS観測計画 import HDS参照観測要求群
from .第0層 import Layer0
from .多言語表層 import 表面化 as 多言語表面化
from .トリニティ文脈 import Trinity文脈系
from .主体 import 主体主幹, 主体状態, 主体更新提案, 主体整合結果, 主体更新記録
from .参照 import 参照供給器, 参照記録, 参照矛盾数
from .命令 import 手順
from .採否 import 実行状態, 採否, 採否結果
from .言語 import 自然言語器

if TYPE_CHECKING:
    from .K3機能 import K3相当能力核, SystemResult as K3能力結果


@dataclass(frozen=True, slots=True)
class 要求:
    問合せ: str
    手順: 手順 | None = None
    初期状態: dict[str, Any] = field(default_factory=dict)
    参照必須: bool = False
    主体更新提案: 主体更新提案 | None = None
    主体整合必須: bool = True
    矛盾数: int = 0
    境界違反: bool = False


@dataclass(frozen=True, slots=True)
class 結果:
    値: Any
    状態: dict[str, Any]
    参照: tuple[参照記録, ...]
    履歴: tuple[dict[str, Any], ...]
    採否: 採否結果
    主体状態: 主体状態 | None = None
    主体整合: 主体整合結果 | None = None
    主体監査履歴: tuple[主体更新記録, ...] = ()
    言語計画: str | None = None
    HDS_IR: HDSIR | None = None


class ミニドラ:
    'HDS-IR、Trinity J/C/M、Layer-0とK3機能相当能力核を接続する非ニューラルLLM 実行系。'

    def __init__(
        self,
        参照供給器_: 参照供給器 | None = None,
        第0層: Layer0 | None = None,
        主体主幹_: 主体主幹 | None = None,
        自然言語器_: 自然言語器 | None = None,
        HDSコンパイラ_: HDSコンパイラProtocol | None = None,
        Trinity文脈_: Trinity文脈系 | None = None,
        K3能力核_: K3相当能力核 | None = None,
    ) -> None:
        self.参照供給器 = 参照供給器_
        self.第0層 = 第0層 or Layer0()
        self.主体主幹 = 主体主幹_ or 主体主幹()
        self.自然言語器 = 自然言語器_ or 自然言語器()
        self.HDSコンパイラ = HDSコンパイラ_
        self.Trinity文脈 = Trinity文脈_ or Trinity文脈系()
        self._K3能力核 = K3能力核_

    @property
    def 主体状態(self) -> 主体状態:
        return self.主体主幹.現在

    @property
    def HDS履歴(self) -> tuple[HDSIR, ...]:
        return self.Trinity文脈.記憶主体.IR履歴

    @property
    def HDS文脈(self) -> HDS文脈:
        return self.Trinity文脈.判断主体.文脈()

    @property
    def K3能力核(self) -> K3相当能力核:
        if self._K3能力核 is None:
            from .K3機能 import K3相当能力核
            self._K3能力核 = K3相当能力核()
        return self._K3能力核

    def K3知識投入(self, statements: Iterable[str]) -> list[dict[str, Any]]:
        return self.K3能力核.知識投入(statements)

    def K3グリッド投入(self, grid: Sequence[Sequence[int]]) -> list[dict[str, Any]]:
        return self.K3能力核.グリッド投入(grid)

    def K3実行(self, request: str, 計算量: str | None = None) -> K3能力結果:
        return self.K3能力核.実行(request, 計算量)

    def _主体更新提案(self, 文脈状態: Mapping[str, Any], 要求_: 要求) -> 主体更新提案 | None:
        候補 = 文脈状態.get("主体更新提案", 要求_.主体更新提案)
        if 候補 is None:
            return None
        if isinstance(候補, 主体更新提案):
            return 候補
        if isinstance(候補, Mapping):
            return 主体更新提案(
                変更=候補.get("変更", {}),
                理由=tuple(候補.get("理由", ())),
                根拠=tuple(候補.get("根拠", ())),
            )
        raise TypeError("主体更新提案は 主体更新提案 または mapping である必要がある")

    def _採否合成(self, 基礎: 採否結果, 主体: 主体整合結果, 必須: bool) -> 採否結果:
        if not 必須 or 主体.状態 in {実行状態.合格, 実行状態.非適用}:
            return 基礎
        if 主体.状態 == 実行状態.失敗:
            return 採否結果(実行状態.失敗, 基礎.理由 + 主体.理由)
        return 採否結果(実行状態.保留, 基礎.理由 + 主体.理由)

    def _帰還(self, 結果: 結果) -> 結果:
        if 結果.HDS_IR is not None:
            self.Trinity文脈.帰還(結果.採否, 結果.値, 結果.HDS_IR)
        return 結果

    def _HDS未閉包(self, 要求_: 要求, ir: HDSIR, 理由: tuple[str, ...]) -> 結果:
        主体整合 = self.主体主幹.非適用結果("HDS-IRが実行閉包していないため主体更新未実行")
        return self._帰還(
            結果(
                None,
                dict(要求_.初期状態),
                (),
                (),
                採否結果(実行状態.保留, 理由),
                self.主体主幹.現在,
                主体整合,
                self.主体主幹.履歴,
                "HDS_IR",
                ir,
            )
        )

    def _HDS選択結果(
        self,
        要求_: 要求,
        ir: HDSIR,
        参照: tuple[参照記録, ...],
        選択: HDS選択実行結果,
    ) -> 結果:
        value = 選択.回答内容 if 選択.状態 == "APPROVE" else None
        reasons = list(選択.理由)
        conflict_count = 要求_.矛盾数 + 参照矛盾数(参照)
        if 要求_.境界違反:
            base = 採否結果(実行状態.失敗, tuple(reasons + ["境界違反"]))
            value = None
        elif conflict_count:
            base = 採否結果(実行状態.保留, tuple(reasons + ["未解消矛盾"]))
            value = None
        elif 選択.状態 == "APPROVE" and value is not None:
            base = 採否結果(実行状態.合格, tuple(reasons))
        else:
            base = 採否結果(実行状態.保留, tuple(reasons or ['HDS_選択肢_SUSPEND']))
            value = None

        状態: dict[str, Any] = dict(要求_.初期状態)
        状態.update(
            {
                "結果": value,
                "参照": 参照,
                "主体状態": self.主体主幹.状態辞書(),
                "HDS文脈": self.HDS文脈,
                "HDS候補ラベル": 選択.回答ラベル,
                "HDS候補コンパイル数": 選択.候補コンパイル数,
                'HDS_資料コンパイル数': 選択.資料コンパイル数,
                'HDS_資料コンパイル失敗数': 選択.資料コンパイル失敗数,
                "K追加事実数": 選択.K追加事実数,
                "K証拠事実数": 選択.K証拠事実数,
                "K証拠阻害事実数": 選択.K証拠阻害事実数,
            }
        )
        if 選択.K3結果 is not None:
            状態["K3努力水準"] = 選択.K3結果.努力水準
            状態["K3探索深さ上限"] = 選択.K3結果.探索深さ上限
            状態["K3証拠上限"] = 選択.K3結果.証拠上限
            状態["K3候補診断"] = tuple(
                {
                    "候補": item.候補,
                    "合計得点": item.合計得点,
                    "証拠得点": item.証拠得点,
                    '関係図得点': item.関係図得点,
                    "独立出典数": item.独立出典数,
                    '関係図深さ': item.関係図深さ,
                }
                for item in 選択.K3結果.候補診断
            )

        proposal = self._主体更新提案(状態, 要求_)
        主体 = self.主体主幹.評価更新(proposal)
        decision = self._採否合成(base, 主体, 要求_.主体整合必須)
        if 要求_.主体整合必須 and decision.状態 in {実行状態.保留, 実行状態.失敗}:
            value = None
            状態["結果"] = None

        history = (
            {
                "op": 'HDS_選択肢_NATIVE',
                "status": 選択.状態,
                "answer_label": 選択.回答ラベル,
                '候補_compiled': 選択.候補コンパイル数,
            },
            {
                "op": "R_TO_HDS_TO_K",
                '参照_count': len(参照),
                '資料_compiled': 選択.資料コンパイル数,
                '資料_compile_failed': 選択.資料コンパイル失敗数,
                "k_facts_added": 選択.K追加事実数,
                '証拠_facts': 選択.K証拠事実数,
                'blocked_証拠_facts': 選択.K証拠阻害事実数,
            },
        )
        return self._帰還(
            結果(
                value,
                状態,
                参照,
                history,
                decision,
                self.主体主幹.現在,
                主体,
                self.主体主幹.履歴,
                'HDS_選択肢_NATIVE',
                ir,
            )
        )

    def コンパイル(self, 問合せ: str) -> HDSIR:
        if self.HDSコンパイラ is None:
            raise RuntimeError('HDS 構文化器が接続されていない')
        return self.Trinity文脈.コンパイル(self.HDSコンパイラ, 問合せ)

    def 実行(self, 要求_: 要求) -> 結果:
        自動計画 = 要求_.手順 is None
        HDS中間表現: HDSIR | None = None
        hds_選択肢 = False
        plan_name: str | None = None
        initial_from_plan: dict[str, Any] = {}
        参照_from_plan = False
        手順_: 手順 | None = 要求_.手順

        if 自動計画 and self.HDSコンパイラ is not None:
            try:
                HDS中間表現 = self.コンパイル(要求_.問合せ)
            except (ValueError, TypeError) as exc:
                主体整合 = self.主体主幹.非適用結果('HDS 構文化器実行失敗')
                return 結果(
                    None,
                    dict(要求_.初期状態),
                    (),
                    (),
                    採否結果(実行状態.失敗, ('HDS 構文化器実行失敗', str(exc))),
                    self.主体主幹.現在,
                    主体整合,
                    self.主体主幹.履歴,
                    "HDS_IR",
                    None,
                )
            hds_選択肢 = HDS選択問題(HDS中間表現)
            if hds_選択肢:
                手順_ = None
                initial_from_plan = dict(HDS中間表現.初期状態)
                参照_from_plan = HDS中間表現.参照必須
                plan_name = 'HDS_選択肢_NATIVE'
            else:
                if not HDS中間表現.実行可能:
                    理由 = ["HDS_IR未閉包", *HDS中間表現.実行阻害理由]
                    if HDS中間表現.残差:
                        理由.extend(f"残差:{item.理由}" for item in HDS中間表現.残差)
                    return self._HDS未閉包(要求_, HDS中間表現, tuple(理由))
                手順_ = HDS中間表現.手順
                initial_from_plan = dict(HDS中間表現.初期状態)
                参照_from_plan = HDS中間表現.参照必須
                plan_name = HDS中間表現.種別 or "HDS_IR"
        elif 自動計画:
            計画 = self.自然言語器.計画(要求_.問合せ)
            手順_ = 計画.手順
            initial_from_plan = dict(計画.初期状態)
            参照_from_plan = 計画.参照必須
            plan_name = 計画.種別

        if 手順_ is None and not hds_選択肢:
            raise ValueError("実行手順が確定していない")
        参照必須 = 要求_.参照必須 or 参照_from_plan

        参照: tuple[参照記録, ...] = ()
        if self.参照供給器 is not None:
            if HDS中間表現 is not None:
                予算 = HDS参照予算選択(HDS中間表現)
                参照 = HDS参照検索(
                    self.参照供給器,
                    HDS中間表現,
                    上限=予算.取得上限,
                    一問合せ上限=予算.一問合せ上限,
                    最大問合せ並列=予算.最大問合せ並列,
                    観測要求=HDS参照観測要求群(HDS中間表現),
                )
            else:
                参照 = self.参照供給器.検索(要求_.問合せ)
        if 参照必須 and not 参照:
            判定 = 採否(根拠数=0)
            主体整合 = self.主体主幹.非適用結果("参照不足のため主体更新未実行")
            実行結果 = 結果(
                None,
                dict(要求_.初期状態),
                (),
                (),
                判定,
                self.主体主幹.現在,
                主体整合,
                self.主体主幹.履歴,
                plan_name,
                HDS中間表現,
            )
            return self._帰還(結果) if HDS中間表現 is not None else 結果

        if hds_選択肢 and HDS中間表現 is not None:
            selected = HDS選択推論実行(
                HDS中間表現,
                参照,
                コンパイル=self.コンパイル,
                基礎能力核=self.K3能力核,
            )
            return self._HDS選択結果(要求_, HDS中間表現, 参照, selected)

        初期 = dict(要求_.初期状態)
        初期.update(initial_from_plan)
        初期["参照"] = 参照
        初期["主体状態"] = self.主体主幹.状態辞書()
        if HDS中間表現 is not None:
            初期["HDS文脈"] = self.HDS文脈

        assert 手順_ is not None
        try:
            文脈 = self.第0層.実行(手順_, 初期)
        except (ValueError, TypeError, ZeroDivisionError) as exc:
            if not 自動計画:
                raise
            主体整合 = self.主体主幹.非適用結果("自動計画の実行失敗")
            実行結果 = 結果(
                None,
                初期,
                (),
                (),
                採否結果(実行状態.失敗, ("自動計画実行失敗", str(exc))),
                self.主体主幹.現在,
                主体整合,
                self.主体主幹.履歴,
                plan_name,
                HDS中間表現,
            )
            return self._帰還(結果) if HDS中間表現 is not None else 結果

        値 = 文脈.状態.get("結果")
        提案 = self._主体更新提案(文脈.状態, 要求_)
        主体整合 = self.主体主幹.評価更新(提案)
        結果根拠数 = (len(参照) if 値 is not None else 0) if 参照必須 else (1 if 値 is not None else 0)
        基礎判定 = 採否(
            根拠数=結果根拠数,
            矛盾数=要求_.矛盾数 + 参照矛盾数(参照),
            危険=要求_.境界違反,
        )
        判定 = self._採否合成(基礎判定, 主体整合, 要求_.主体整合必須)
        if 要求_.主体整合必須 and 判定.状態 in {実行状態.保留, 実行状態.失敗}:
            値 = None

        実行結果 = 結果(
            値,
            dict(文脈.状態),
            参照,
            tuple(文脈.履歴),
            判定,
            self.主体主幹.現在,
            主体整合,
            self.主体主幹.履歴,
            plan_name,
            HDS中間表現,
        )
        return self._帰還(結果) if HDS中間表現 is not None else 結果

    def 応答(self, 問合せ: str) -> str:
        結果 = self.実行(要求(問合せ))
        if 結果.HDS_IR is not None:
            言語 = 結果.HDS_IR.出力言語 or 結果.HDS_IR.入力言語
            return 多言語表面化(結果.値, 結果.採否.状態.value, 結果.採否.理由, 言語)
        return self.自然言語器.表面化(
            結果.値,
            結果.採否.状態.value,
            結果.採否.理由,
        )
