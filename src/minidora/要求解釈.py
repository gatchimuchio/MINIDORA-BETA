"""HDSの原文・残差を保持し、局所的な文書操作要求を合成計画へ降下する。

対応文法を全文消費できた場合だけ計画を返す。任意の日本語理解器ではない。
資料は別入力とし、その本文から能力名・新しい命令を生成しない。
"""
from __future__ import annotations

from collections.abc import Mapping
from copy import deepcopy
from dataclasses import dataclass, fields, is_dataclass, replace
from datetime import datetime
from enum import Enum
import hashlib
import json
import math
import re
import unicodedata

from .HDS中間表現 import HDSIR, HDS座標, HDS関係, HDS残差, HDS意味作用, HDS実行核, 値状態
from .能力合成 import 合成工程, 合成計画, 素材参照, _結果辞書, _参照結合
from .製品版.型 import 能力結果, 参照資料
from .文脈照応 import 会話参照スナップショット, 照応束縛, 過去対象表層

要求解釈版 = "MINIDORA-要求解釈-v0.2"


@dataclass(frozen=True, slots=True)
class 要求残差:
    原文範囲: tuple[int, int]
    原文: str
    理由: str
    解消条件: str


@dataclass(frozen=True, slots=True)
class 要求工程:
    識別子: str
    能力: str
    素材: 素材参照
    設定: tuple[tuple[str, str | int], ...]
    原文範囲: tuple[int, int]
    対象範囲: tuple[int, int] | None
    対象解決: str
    行数条件: str = "なし"
    行数: int = 0
    既定適用: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class 要求解釈結果:
    状態: str
    HDS保持: HDSIR | None
    要求: tuple[要求工程, ...]
    計画: 合成計画 | None
    初期資料: dict[str, 能力結果]
    残差: tuple[要求残差, ...]
    局所解消: tuple[str, ...] = ()
    ハッシュ: str = ""
    文脈識別子: str | None = None
    文脈束縛: tuple[照応束縛, ...] = ()

    @property
    def 成立(self) -> bool:
        return self.状態 == "合格" and self.計画 is not None and not self.残差

    def 整合確認(self) -> bool:
        '入力・解釈・計画・資料の意図しない変更を検出する。署名ではない。'
        try:
            return self.ハッシュ == _指紋(replace(self, ハッシュ=""))
        except (TypeError, ValueError, RecursionError):
            return False


def _正規値(値: object, 深さ: int = 0) -> object:
    if 深さ > 40:
        raise ValueError("入れ子上限")
    if isinstance(値, Enum):
        return _正規値(値.value, 深さ + 1)
    if 値 is None or type(値) in (str, int, bool):
        return 値
    if type(値) is float and math.isfinite(値):
        return 値
    if isinstance(値, datetime):
        return 値.isoformat()
    if is_dataclass(値) and not isinstance(値, type):
        return {f.name: _正規値(getattr(値, f.name), 深さ + 1) for f in fields(値)}
    if type(値) in (list, tuple):
        return [_正規値(v, 深さ + 1) for v in 値]
    if type(値) is dict and all(type(k) is str for k in 値):
        return {k: _正規値(v, 深さ + 1) for k, v in 値.items()}
    raise ValueError("記録不能な入力値")


def _符号(値: object) -> bytes:
    return json.dumps(_正規値(値), ensure_ascii=True, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _指紋(値: object) -> str:
    return hashlib.sha256(_符号(値)).hexdigest()


# 能力名の出現ではなく、対象・引数・述語・接続詞を持つ限定文法で読む。
_空白 = r"[^\S\r\n]*"
_対象 = rf'(?P<対象>資料「(?P<資料名>[^「」\r\n]{{1,128}})」|元の本文|元の資料|本文|提供文|その結果|それ|{過去対象表層})'
_接頭 = rf"(?:(?:まず|次に|続けて|最後に|その後|それから){_空白}[、,]?{_空白})??"
_終止 = r"(?:してください|して下さい|してくれ|して|する|せよ|し)"
_命令 = re.compile(
    _接頭 + rf"(?:{_対象}{_空白}(?:から|を){_空白})?(?:"
    rf"(?:(?P<数>[0-9０-９一二三四五六七八九十百零〇]+)行(?P<以内>以内)?で{_空白})?"
    rf"(?P<要約>要約){_終止}"
    rf"|(?P<抽出>数字|数値|URL|url|ＵＲＬ|リンク|キーワード){_空白}(?:を)?{_空白}抽出{_終止}"
    rf"|(?P<整形>箇条書き|リスト){_空白}に{_空白}{_終止}"
    rf")"
)
_区切り = re.compile(r"\s*[、,。;；\n]+\s*")
_段間 = re.compile(r"\s*から\s*")
_終端 = re.compile(r"[\s。.!！]*\Z")
_数文字 = {k: i for i, k in enumerate("零一二三四五六七八九")}


def _行数(表記: str) -> int:
    値 = unicodedata.normalize("NFKC", 表記)
    if 値.isascii() and 値.isdecimal():
        return int(値)
    if 値 in _数文字:
        return _数文字[値]
    raise ValueError("対応行数は1〜8")


class _未解(Exception):
    def __init__(self, 始点: int, 終点: int, 理由: str, 解消条件: str):
        super().__init__(理由)
        self.範囲 = (始点, 終点)
        self.解消条件 = 解消条件


class 要求計画器:
    """HDSIRから明示文書操作の計画を作る。コンパイル中に能力を実行しない。"""

    def __init__(self, *, 最大要求数: int = 32, 最大入力文字数: int = 8192,
                 最大資料バイト数: int = 1_000_000):
        for 上限 in (最大要求数, 最大入力文字数, 最大資料バイト数):
            if type(上限) is not int or 上限 <= 0:
                raise ValueError("上限は正の整数")
        self._上限 = (最大要求数, 最大入力文字数, 最大資料バイト数)

    def コンパイル(self, 意味IR: HDSIR, 資料: Mapping[str, 能力結果], *,
                   文脈: 会話参照スナップショット | None = None) -> 要求解釈結果:
        要求: list[要求工程] = []
        保持 = None
        原文 = ""
        起点 = None
        束縛: list[照応束縛] = []

        def 終了(状態: str, 計画=None, 資料=None, 残差=(), 解消=()):
            結果 = 要求解釈結果(状態, 保持, tuple(要求), 計画,
                                deepcopy(資料 or {}), tuple(残差), tuple(解消),
                                文脈識別子=起点.識別子 if 起点 is not None else None,
                                文脈束縛=tuple(束縛))
            return replace(結果, ハッシュ=_指紋(replace(結果, ハッシュ="")))

        try:
            if 文脈 is not None:
                if not isinstance(文脈, 会話参照スナップショット) or not 文脈.整合確認():
                    raise ValueError("文脈スナップショットの整合違反")
                起点 = deepcopy(文脈)
            if not isinstance(意味IR, HDSIR) or type(意味IR.原文) is not str:
                raise ValueError("HDSIRと原文が必要")
            原文 = 意味IR.原文
            原文.encode("utf-8")
            if not isinstance(意味IR.実行核, HDS実行核):
                raise ValueError("HDS実行核型不正")
            if len(原文) > self._上限[1]:
                raise ValueError("要求文字数上限")
            for 値, 型 in ((意味IR.座標, HDS座標), (意味IR.関係, HDS関係),
                          (意味IR.残差, HDS残差), (意味IR.意味作用履歴, HDS意味作用)):
                if type(値) is not tuple or not all(isinstance(x, 型) for x in 値):
                    raise ValueError("HDS構造型不正")
            if len(_符号(意味IR)) > self._上限[2]:
                raise ValueError("HDS記録サイズ上限")
            if len({r.関係ID for r in 意味IR.関係}) != len(意味IR.関係):
                raise ValueError("HDS関係ID重複")
            if len({r.残差ID for r in 意味IR.残差}) != len(意味IR.残差):
                raise ValueError("HDS残差ID重複")
            保持 = deepcopy(意味IR)
            座標 = 保持.座標辞書()
            if len(座標) != len(保持.座標):
                raise ValueError("HDS座標重複")
            原典 = [c for c in 保持.座標 if c.種別 == '情報源_text']
            if len(原典) != 1 or 原典[0].内容 != 原文 or 原典[0].値状態 != 値状態.確定:
                raise ValueError("HDS原文座標不一致")
            if not isinstance(資料, Mapping):
                raise ValueError("資料は名前付きの写像")
            for 名前, 値 in 資料.items():
                if type(名前) is not str or not 名前.strip() or len(名前) > 128:
                    raise ValueError("資料名不正")
                if not isinstance(値, 能力結果) or type(値.成立) is not bool or type(値.本文) is not str:
                    raise ValueError("資料値型不正")
                if type(値.参照) is not tuple or not all(isinstance(r, 参照資料) for r in 値.参照):
                    raise ValueError("資料参照型不正")
                値.本文.encode("utf-8")
                _結果辞書(値)
                _参照結合(値.参照)
            素材 = deepcopy(dict(資料))
            if len(_符号(素材)) > self._上限[2]:
                raise ValueError("資料サイズ上限")
            if not 原文.strip():
                raise _未解(0, len(原文), "要求が空", "処理要求を指定する")
            資料: dict[str, 能力結果] = {}
            資料キー = {n: f"資料:{i:04d}" for i, n in enumerate(sorted(素材))}
            最初の素材: 素材参照 | None = None
            位置 = len(原文) - len(原文.lstrip())
            while 位置 < len(原文):
                if len(要求) >= self._上限[0]:
                    raise _未解(位置, len(原文), "要求数上限", "要求を分ける")
                一致 = _命令.match(原文, 位置)
                if 一致 is None:
                    raise _未解(位置, len(原文), "未対応または曖昧な要求", "残った要求・条件を明示する")
                終点 = 一致.end()
                if _終端.fullmatch(原文[終点:]):
                    次 = len(原文)
                else:
                    区切り = _区切り.match(原文, 終点)
                    段間 = _段間.match(原文, 終点) if 一致.group(0).endswith("して") else None
                    接続 = 区切り or 段間
                    if 接続 is None:
                        raise _未解(終点, len(原文), "要求の未消費部分", "否定・条件・追加指定を省略せず対応範囲を確認する")
                    次 = 接続.end()
                    if 次 == len(原文) and not _終端.fullmatch(原文[終点:]):
                        raise _未解(終点, 次, "接続後の要求がない", "続く要求を記述する")
                if 次 == len(原文) and 一致.group(0).endswith("し"):
                    raise _未解(位置, 終点, "要求が連用形で未完", "終止形か後続操作を指定する")
                対象 = 一致.group("対象")
                対象範囲 = 一致.span("対象") if 対象 else None
                if ((対象 == "それ" and not 要求 and 起点 is not None)
                        or (対象 is not None and re.fullmatch(過去対象表層, 対象))):
                    if 起点 is None:
                        raise _未解(*一致.span("対象"), "会話文脈がない", "同じセッションの文脈を渡す")
                    try:
                        接続, 値 = 起点.解決(対象, 一致.span("対象"), 新規資料あり=bool(素材))
                    except ValueError as exc:
                        raise _未解(*一致.span("対象"), str(exc), "参照する応答・出力番号を明示する") from exc
                    束縛.append(接続)
                    資料[接続.資料キー] = 値
                    入力, 解決 = 素材参照("入力", 接続.資料キー), "会話成果への明示照応"
                    if 最初の素材 is None:
                        最初の素材 = 入力
                elif 対象 in ("その結果", "それ"):
                    if not 要求:
                        raise _未解(*一致.span("対象"), "前工程の結果がない", "対象資料を指定する")
                    入力 = 素材参照("工程", 要求[-1].識別子)
                    解決 = "前工程への明示照応"
                elif 対象 in ("元の本文", "元の資料"):
                    if 最初の素材 is None:
                        raise _未解(*一致.span("対象"), "元の素材が未確定", "先に対象資料を指定する")
                    入力, 解決 = 最初の素材, "最初の入力資料への復帰"
                elif 対象 is None and 要求:
                    入力, 解決 = 素材参照("工程", 要求[-1].識別子), "連続操作の省略対象"
                else:
                    名前 = 一致.group("資料名")
                    if 名前 is None:
                        if len(素材) != 1:
                            raise _未解(位置, 終点, "対象資料が未確定", "資料を一つ渡すか資料「名前」で指定する")
                        名前 = next(iter(素材))
                    if 名前 not in 素材:
                        raise _未解(位置, 終点, f"指定資料がない:{名前}", "指定した名前の資料を渡す")
                    if not 素材[名前].成立 or not 素材[名前].本文.strip():
                        raise _未解(位置, 終点, f"資料が未成立または空:{名前}", "成立した処理対象を渡す")
                    キー = 資料キー[名前]
                    資料[キー] = 素材[名前]
                    入力, 解決 = 素材参照("入力", キー), f"提供資料:{名前}"
                    if 最初の素材 is None:
                        最初の素材 = 入力
                行条件, 行数, 既定 = "なし", 0, ()
                if 一致.group("要約"):
                    能力 = "抽出要約"
                    if 一致.group("数") is None:
                        行数, 行条件, 既定 = 3, "上限", ("要約行数は既定上限3",)
                    else:
                        try:
                            行数 = _行数(一致.group("数"))
                        except ValueError:
                            行数 = 0
                        if not 1 <= 行数 <= 8:
                            raise _未解(*一致.span("数"), "対応行数は1〜8", "対応範囲の行数を指定する")
                        行条件 = "上限" if 一致.group("以内") else "一致"
                    設定 = (("行数", 行数),)
                elif 一致.group("抽出"):
                    種 = 一致.group("抽出")
                    種 = "数字" if 種 in ("数字", "数値") else "URL" if 種 in ("URL", "url", "ＵＲＬ", "リンク") else 種
                    能力, 設定 = "情報抽出", (("種別", 種),)
                else:
                    能力, 設定 = "文脈変換", (("形式", "箇条書き"),)
                ID = f"要求:{len(要求) + 1:04d}"
                要求.append(要求工程(ID, 能力, 入力, 設定, (位置, 終点), 対象範囲,
                                       解決, 行条件, 行数, 既定))
                資料[f"指示:{ID}"] = 能力結果(True, 原文[位置:終点])
                資料[f"設定:{ID}"] = 能力結果(True, "", データ=dict(設定))
                位置 = 次
            解消 = self._HDS照合(保持, tuple(要求), 起点, tuple(束縛))
            工程 = tuple(合成工程(t.識別子, (t.能力,), f"指示:{t.識別子}",
                                    (t.素材,), f"設定:{t.識別子}") for t in 要求)
            消費 = {t.素材.識別子 for t in 要求 if t.素材.領域 == "工程"}
            出力 = tuple(t.識別子 for t in 要求 if t.識別子 not in 消費)
            return 終了("合格", 合成計画(工程, 出力), 資料, 解消=解消)
        except _未解 as exc:
            a, b = exc.範囲
            return 終了("保留", 残差=(要求残差((a, b), 原文[a:b], str(exc), exc.解消条件),))
        except (TypeError, ValueError, RecursionError, AttributeError) as exc:
            # 不正なHDSは全量保存を保証しない。任意オブジェクトの文字列化もしない。
            try:
                _符号(保持)
            except (TypeError, ValueError, RecursionError):
                保持 = None
            return 終了("失敗", 残差=(要求残差((0, len(原文)), 原文,
                f"入力契約違反:{type(exc).__name__}:{exc}" if isinstance(exc, ValueError)
                else f"入力契約違反:{type(exc).__name__}", "入力契約を修正する"),))

    @staticmethod
    def _HDS照合(IR: HDSIR, 要求: tuple[要求工程, ...],
                 文脈: 会話参照スナップショット | None = None,
                 束縛: tuple[照応束縛, ...] = ()) -> tuple[str, ...]:
        def 保留(理由):
            raise _未解(0, len(IR.原文), 理由, "上流の該当構造を確認・解消する")

        if IR.入力言語 != "ja" or IR.出力言語 not in (None, "ja"):
            保留("この局所降下は日本語入出力のみ")
        許容座標 = {'情報源_text', '言語.normalized', "文脈.言語", "制御.選択意図",
                    "値.数量", "属性.単位", "対象.主題語", "目的.検索焦点",
                    "文脈.参照先", "文脈.指示語"}
        指示範囲 = ([t.対象範囲 for t in 要求 if t.対象解決 == "前工程への明示照応"]
                    + [b.原文範囲 for b in 束縛])
        def 局所照応(語):
            if 語 not in ("その", "それ"):
                return False
            出現 = tuple(re.finditer(re.escape(語), IR.原文))
            return bool(出現) and all(any(a <= m.start() and m.end() <= b for a, b in 指示範囲) for m in 出現)

        座標 = IR.座標辞書()
        解消座標, 解消関係 = set(), set()
        for r in IR.関係:
            if r.種別 != "共参照":
                continue
            if (文脈 is None or not 文脈.整合確認()
                    or IR.文脈引用 != (文脈.識別子,) or r.条件
                    or r.値状態 != 値状態.推定 or len(r.始点) != 1 or len(r.終点) != 1):
                保留(f"文脈根拠不一致:{r.関係ID}")
            指示 = 座標.get(r.始点[0])
            焦点 = 座標.get(r.終点[0])
            if (指示 is None or 焦点 is None or 指示.種別 != "文脈.指示語"
                    or 焦点.種別 != "文脈.参照先" or 焦点.内容 != 文脈.識別子
                    or 指示.値状態 != 値状態.推定 or 焦点.値状態 != 値状態.推定
                    or not 局所照応(指示.内容)):
                保留(f"未解文脈照応:{r.関係ID}")
            解消座標.update((*r.始点, *r.終点))
            解消関係.add(r.関係ID)
        for c in IR.座標:
            if c.座標ID in 解消座標:
                continue
            if c.種別 not in 許容座標:
                保留(f"未処理HDS座標:{c.座標ID}:{c.種別}")
            if c.値状態 != 値状態.確定:
                # 上流の前turnへの推定照応を、この要求内の前工程と混同しない。
                保留(f"未確定HDS座標:{c.座標ID}")
            if c.種別 == "制御.選択意図" and c.内容 != "通常":
                保留(f"未処理HDS選択意図:{c.内容}")
        for r in IR.関係:
            if r.関係ID in 解消関係:
                continue
            if r.種別 != "数量単位" or r.値状態 != 値状態.確定:
                保留(f"未処理HDS関係:{r.関係ID}")
            if r.条件 or not all(k in 座標 for k in (*r.始点, *r.終点)):
                保留(f"未処理HDS関係条件:{r.関係ID}")
        解消 = [f"関係:{ID}" for ID in sorted(解消関係)]
        for r in IR.残差:
            if r.種別 == "未解共参照" and not r.影響座標 and 局所照応(r.原文):
                解消.append(r.残差ID)
            else:
                保留(f"未解HDS残差:{r.残差ID}:{r.種別}")
        解消理由 = {r.理由 for r in IR.残差 if r.残差ID in 解消}
        for 作用 in IR.意味作用履歴:
            if any(損失 not in 解消理由 for 損失 in 作用.損失):
                保留(f"未解HDS意味損失:{作用.作用ID}")
        return tuple(解消)
