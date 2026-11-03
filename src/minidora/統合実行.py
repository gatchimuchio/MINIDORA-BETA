"""部品を共通の実行・検証・採用・会話履歴へ接続する同期の統合入口。

GUIや既存標準チャットの置換ではない。自由文の対応範囲は既存解釈器に従う。
"""
from __future__ import annotations

from copy import deepcopy
from dataclasses import asdict, dataclass, replace
from hashlib import sha256
from threading import Lock
from time import perf_counter_ns

from .純粋結果庫 import 純粋結果庫
from .能力合成 import 能力合成器, 合成計画, 合成工程, 素材参照, 合成結果, _結果辞書, _符号化
from .長文脈管理 import 長文脈庫, 長文脈起点, 文脈登録
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈

統合実行版 = "MINIDORA-統合実行-v0.2"


@dataclass(frozen=True, slots=True)
class 受入条件:
    対象出力: str
    検査能力: str
    設定参照: str | None = None
    基準資料: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class 統合計画:
    起点: 長文脈起点
    計画: 合成計画
    資料: dict[str, 能力結果]
    出力ID: tuple[str, ...]
    依頼文: str
    能力版: tuple[tuple[str, str, bool], ...]
    ハッシュ: str = ""
    依存記録: tuple[str, ...] = ()


def _計画印(plan):
    return sha256(_符号化({"起点": asdict(plan.起点), "計画": asdict(plan.計画),
        '資料': {k: _結果辞書(v) for k, v in plan.資料.items()}, "出力": plan.出力ID,
        "依頼": plan.依頼文, "能力": plan.能力版, "依存記録": plan.依存記録})).hexdigest()


@dataclass(frozen=True, slots=True)
class 統合応答:
    状態: str
    本文: str
    出力: tuple[tuple[str, 能力結果], ...]
    理由: str
    起点: 長文脈起点
    更新後: 長文脈起点
    実行: 合成結果 | None
    計測: dict
    解釈: object = None
    採用記録ID: tuple[str, ...] = ()

    @property
    def 成立(self):
        return self.状態 == "合格"

    def 辞書化(self):
        info = None
        if type(self.解釈) is dict:
            interpreted = self.解釈.get("解釈")
            translation = self.解釈.get("翻訳")
            info = {"原文": self.解釈.get("原文"), "入力言語": self.解釈.get("入力言語"),
                    "日本語原文": interpreted.HDS保持.原文 if interpreted and interpreted.HDS保持 else None,
                    "残差": [asdict(r) for r in interpreted.残差] if interpreted else [],
                    "翻訳保留": translation.保留理由 if translation else ""}
        return {"状態": self.状態, "本文": self.本文, "理由": self.理由,
                "出力": {k: _結果辞書(v) for k, v in self.出力},
                "起点": asdict(self.起点), "更新後": asdict(self.更新後),
                "実行ハッシュ": self.実行.ルートハッシュ if self.実行 else "",
                "実行能力": [r.能力 for r in self.実行.履歴] if self.実行 else [],
                "計測": deepcopy(self.計測), "解釈情報": info, "採用記録ID": list(self.採用記録ID)}


    def 意味辞書化(self):
        """回答の比較用射影。計測・所有ID・実行記録は辞書化()に残す。

        意味結果内部は全項目を保持する。データ中の「計算量」「時刻」などを
        名前だけで計測と見なして除去しない。実行監査をこの射影で代用しない。
        """
        full = self.辞書化()
        return {key: full[key] for key in ('状態', '本文', '理由', '出力', '解釈情報')}


class 統合セッション:
    """文書・数学・コード等の成功成果を同じ会話焦点と長文脈へ採用する。"""
    def __init__(self, セッションID: str, *, 外部読取許可=False, 再利用=True,
                 取得器=None, 閲覧器=None, 最大応答数=64, 最大回答文字数=100000, 追加能力=(), 基底能力=None):
        if type(外部読取許可) is not bool:
            raise ValueError("外部許可はbool")
        if type(最大応答数) is not int or not 1 <= 最大応答数 <= 256:
            raise ValueError("応答上限不正")
        if type(最大回答文字数) is not int or not 1 <= 最大回答文字数 <= 100000:
            raise ValueError("回答文字数上限不正")
        self._庫 = 長文脈庫(セッションID)
        self._再利用 = 純粋結果庫(有効=再利用)
        if 基底能力 is None:
            # 既定の全能力構成は変更しない。読取専用の検証では登録済み部分群を明示できる。
            from .統合能力 import 統合能力群
            self._能力 = 統合能力群(self._庫, self._再利用, 外部読取許可=外部読取許可,
                                   取得器=取得器, 閲覧器=閲覧器)
        else:
            if type(基底能力) is not tuple:
                raise ValueError("基底能力は明示した登録契約のtuple又はNone")
            self._能力 = 基底能力
        # 追加契約も既存の登録検査・版照合・外部許可・採用境界を通す。
        if type(追加能力) is not tuple:
            raise ValueError("追加能力は登録済み契約のtuple")
        self._能力 = (*self._能力, *追加能力)
        self._許可 = 外部読取許可
        self._実行器 = 能力合成器(self._能力)
        self._上限 = (最大応答数, 最大回答文字数)
        self._履歴 = ()
        self._ロック = Lock()

    def 起点(self):
        return self._庫.起点()

    def 能力一覧(self):
        return tuple({"名前": r.モジュール.名前, "版": r.モジュール.版, "外部読取": r.外部読取}
                     for r in self._能力)

    def _能力版(self):
        return tuple((r.モジュール.名前, r.モジュール.版, r.外部読取) for r in self._能力)

    def 採用履歴スナップショット(self):
        """追加の解釈器が、同じ起点の採用済み成果だけを参照する。"""
        if not self._ロック.acquire(blocking=False):
            raise ValueError("処理中の統合セッション")
        try:
            return self.起点(), self._現行採用履歴()
        finally:
            self._ロック.release()

    def _現行採用履歴(self):
        # 歴史記録は削除しない。失効した出力を次の依頼の現行根拠へ再昇格させない。
        active = []
        for turn, row in enumerate(self._履歴, 1):
            values = tuple(value for i, value in enumerate(row["出力"])
                           if self._庫.原記録(f"応答:{turn}:出力:{i}")["現行"])
            if values:
                active.append({**deepcopy(row), "出力": deepcopy(values)})
        return tuple(active)

    def 記録を失効(self, 起点, 識別子: tuple[str, ...], *, 理由: str):
        """明示訂正による記録失効。庫の原子的更新後にのみ現行焦点から除外する。"""
        if not self._ロック.acquire(blocking=False):
            raise ValueError("処理中の統合セッション")
        try:
            return self._庫.更新(起点, 失効ID=識別子, 理由=理由)
        finally:
            self._ロック.release()

    def 再利用統計(self):
        return self._再利用.統計()

    def 文脈要求(self, 要求):
        from .長文脈接続 import 長文脈要求資料
        return 長文脈要求資料(self._庫, 要求)

    def 保存文脈(self):
        return self._庫.保存文字列()

    def 原記録(self, 識別子):
        return self._庫.原記録(識別子)

    def 初期化(self):
        if not self._ロック.acquire(blocking=False):
            raise ValueError("処理中の統合セッション")
        try:
            after = self._庫.初期化(self.起点())
            self._履歴 = ()
            self._再利用.消去()
            return after
        finally:
            self._ロック.release()

    def 準備(self, 計画: 合成計画, 資料: dict[str, 能力結果], *,
             依頼文="明示された能力計画を実行", 条件: tuple[受入条件, ...] = (),
             依存記録: tuple[str, ...] = ()) -> 統合計画:
        if not self._ロック.acquire(blocking=False):
            raise ValueError("処理中の統合セッション")
        try:
            if type(計画) is not 合成計画 or type(資料) is not dict or type(条件) is not tuple or len(条件) > 32:
                raise ValueError("統合要求の型・規模不正")
            if type(依頼文) is not str or not 依頼文.strip() or len(依頼文) > 8192:
                raise ValueError("依頼文不正")
            if type(計画.工程) is not tuple or type(計画.出力工程) is not tuple:
                raise ValueError("計画の列型不正")
            if (type(依存記録) is not tuple or len(依存記録) > 64
                    or any(type(k) is not str for k in 依存記録)
                    or len(set(依存記録)) != len(依存記録)
                    or any(not self._庫.原記録(k)["現行"] for k in 依存記録)):
                raise ValueError("依存記録は現行の一意な原記録IDに限定する")
            資料 = deepcopy(資料)
            for key, value in 資料.items():
                if type(key) is not str or key.startswith("統合:"):
                    raise ValueError("入力名不正または予約名衝突")
                _結果辞書(value)
            steps = list(deepcopy(計画.工程))
            if any(s.識別子.startswith("統合:") for s in steps):
                raise ValueError("工程の予約名衝突")
            gates = []
            allowed = {"成果検査", "コード検証", "数学結果採用", "関係判定採用", "記載値採用", "文書操作"}
            for i, condition in enumerate(条件):
                if type(condition) is not 受入条件 or type(condition.基準資料) is not tuple:
                    raise ValueError("受入条件の型不正")
                if condition.対象出力 not in 計画.出力工程 or condition.検査能力 not in allowed:
                    raise ValueError("検査対象・検査能力不正。外部読取を検証に使わない")
                if condition.検査能力 == "文書操作":
                    if condition.設定参照 not in 資料 or 資料[condition.設定参照].データ.get("操作") != "型検査":
                        raise ValueError("文書の最終検査には型検査を指定する")
                key = f"統合:検証:{i}"
                資料[key] = 能力結果(True, "最終成果を明示条件で検証")
                inputs = (素材参照("工程", condition.対象出力),
                          *(素材参照("入力", k) for k in condition.基準資料))
                steps.append(合成工程(key, (condition.検査能力,), key, inputs, condition.設定参照))
                gates.append(key)
            plan = 合成計画(tuple(steps), (*計画.出力工程, *gates))
            # 実行しない構造検査。要求時の外部許可は別途実行時にも必要。
            self._実行器._準備(plan, 資料, True)
            packed = 統合計画(self.起点(), plan, 資料, 計画.出力工程, 依頼文, self._能力版(), 依存記録=依存記録)
            if len(_符号化({k: _結果辞書(v) for k, v in 資料.items()})) > 2000000:
                raise ValueError("統合入力サイズ上限")
            return replace(packed, ハッシュ=_計画印(packed))
        finally:
            self._ロック.release()

    @staticmethod
    def _停止(stop):
        if stop is None:
            return
        value = stop()
        if type(value) is not bool:
            raise ValueError("停止判定型不正")
        if value:
            raise InterruptedError("停止要求")

    def _応答値(self, 状態, start, 結果=None, outputs=(), reason="", metrics=None, interpretation=None):
        body = (outputs[0][1].本文 if len(outputs) == 1 else
                "\n\n".join(f"［{key}］\n{value.本文}" for key, value in outputs)) if 状態 == "合格" else ""
        ids = tuple(f"応答:{len(self._履歴)}:出力:{i}" for i in range(len(outputs))) if 状態 == "合格" else ()
        return 統合応答(状態, body, deepcopy(outputs), reason, start, self.起点(),
                        結果, deepcopy(metrics or {}), interpretation, ids)

    def _確定(self, start, request, 情報源, outputs, 結果, stop, 依存記録=()):
        self._停止(stop)
        if start != self.起点() or len(self._履歴) >= self._上限[0]:
            raise ValueError("状態変更または応答数上限")
        if not 結果.成立 or not 結果.監査整合() or not outputs:
            raise ValueError("採用する実行結果が不成立")
        if any(not v.成立 for _, v in outputs):
            raise ValueError("不成立の最終出力")
        body = outputs[0][1].本文 if len(outputs) == 1 else "\n\n".join(f"［{k}］\n{v.本文}" for k, v in outputs)
        if len(body) > self._上限[1]:
            raise ValueError("最終回答の文字数上限。切断しない")
        turn = len(self._履歴)+1
        資料_rows = tuple(文脈登録(f"応答:{turn}:資料:{i}", v) for i, v in enumerate(情報源.values()))
        # 全提供資料への保守的な依存。意味的に必要な資料を推定したことにはしない。
        dependencies = (*依存記録, *(r.識別子 for r in 資料_rows))
        out_rows = tuple(文脈登録(f"応答:{turn}:出力:{i}", value, "成果", dependencies)
                         for i, (_, value) in enumerate(outputs))
        new = {"依頼": request, "出力": deepcopy(outputs), "実行ハッシュ": 結果.ルートハッシュ}
        候補 = self._履歴 + (new,)
        if len(_符号化([{**r, "出力": [(k, _結果辞書(v)) for k, v in r["出力"]]} for r in 候補])) > 2000000:
            raise ValueError("会話採用履歴サイズ上限")
        self._停止(stop)
        # 庫の原子的更新が成功するまで会話焦点を更新しない。
        self._庫.更新(start, (*資料_rows, *out_rows))
        self._履歴 = 候補

    def _計測(self, before, begun):
        after = self._再利用.統計()
        delta = {}
        for name, row in after["能力別"].items():
            old = before["能力別"].get(name, {})
            delta[name] = {k: v-old.get(k, 0) for k, v in row.items()}
        return {"経過ミリ秒": (perf_counter_ns()-begun)/1000000, "再利用差分": delta,
                "保存バイト数": after["保存バイト数"]}

    def 実行(self, 計画: 統合計画, *, 外部読取許可=False, 停止要求=None):
        start = self.起点()
        if not self._ロック.acquire(blocking=False):
            return self._応答値("保留", start, reason="同じ統合セッションの処理中")
        start = self.起点()  # ロック取得までの競合で古くなった起点を使用しない。
        begun, before = perf_counter_ns(), self._再利用.統計()
        結果 = None
        try:
            self._停止(停止要求)
            if type(計画) is not 統合計画 or 計画.ハッシュ != _計画印(計画):
                raise ValueError("統合計画の改変・型不正")
            if (type(計画.起点.世代) is not int or type(計画.起点.改訂) is not int
                    or 計画.起点 != start or 計画.能力版 != self._能力版()):
                raise ValueError("別所有者・旧計画・能力版変更")
            if type(外部読取許可) is not bool or (外部読取許可 and not self._許可):
                raise ValueError("外部読取許可の範囲外")
            if len(self._履歴) >= self._上限[0]:
                raise ValueError("応答数上限")
            fixed = deepcopy(計画)
            結果 = self._実行器.実行(fixed.計画, fixed.資料,
                文脈=能力文脈(fixed.依頼文, start.セッションID),
                外部読取許可=外部読取許可, 停止要求=停止要求)
            if not 結果.成立:
                return self._応答値(結果.状態, start, 結果, reason=結果.理由,
                                      metrics=self._計測(before, begun))
            values = dict(結果.出力)
            outputs = tuple((key, values[key]) for key in fixed.出力ID)
            self._確定(start, fixed.依頼文, fixed.資料, outputs, 結果, 停止要求, fixed.依存記録)
            return self._応答値("合格", start, 結果, outputs, metrics=self._計測(before, begun))
        except Exception as exc:
            状態 = "中止" if isinstance(exc, InterruptedError) else "失敗"
            return self._応答値(状態, start, 結果, reason="統合実行不成立:"+type(exc).__name__,
                                  metrics=self._計測(before, begun))
        finally:
            self._ロック.release()

    def 計画実行(self, 計画, 資料, *, 条件=(), 依頼文="明示された能力計画を実行",
                 外部読取許可=False, 停止要求=None):
        try:
            prepared = self.準備(計画, 資料, 条件=条件, 依頼文=依頼文)
        except Exception as exc:
            return self._応答値("保留", self.起点(), reason="統合準備不成立:"+type(exc).__name__)
        return self.実行(prepared, 外部読取許可=外部読取許可, 停止要求=停止要求)

    def 応答(self, 依頼: str, 資料=None, *, 入力言語="ja", 停止要求=None):
        """既存の実HDSによる日英文書依頼。対応外を汎用回答で埋めない。"""
        start = self.起点()
        if not self._ロック.acquire(blocking=False):
            return self._応答値("保留", start, reason="同じ統合セッションの処理中")
        start = self.起点()  # ロック取得までの競合で古くなった起点を使用しない。
        begun, before = perf_counter_ns(), self._再利用.統計()
        結果, detail = None, None
        try:
            self._停止(停止要求)
            if len(self._履歴) >= self._上限[0]:
                raise ValueError("応答数上限")
            from .統合依頼 import 依頼を準備
            from .要求解釈実行 import 要求計画を実行
            interpretation, snapshot, detail = 依頼を準備(
                依頼, 資料, 入力言語, start.セッションID, self._現行採用履歴(), self._上限[0])
            self._停止(停止要求)
            if interpretation is None or not interpretation.成立:
                return self._応答値("保留", start, reason="依頼の未対応・未解釈部分を保持", interpretation=detail,
                                      metrics=self._計測(before, begun))
            executed = 要求計画を実行(interpretation, self._実行器, 文脈起点=snapshot, 停止要求=停止要求)
            結果 = executed.合成
            if not executed.成立:
                return self._応答値(executed.状態, start, 結果, reason=";".join(executed.理由),
                                      interpretation=detail, metrics=self._計測(before, begun))
            self._確定(start, 依頼, interpretation.初期資料, executed.出力, 結果, 停止要求)
            return self._応答値("合格", start, 結果, executed.出力, interpretation=detail,
                                  metrics=self._計測(before, begun))
        except Exception as exc:
            return self._応答値("中止" if isinstance(exc, InterruptedError) else "失敗", start, 結果,
                reason="統合依頼不成立:"+type(exc).__name__, interpretation=detail, metrics=self._計測(before, begun))
        finally:
            self._ロック.release()
