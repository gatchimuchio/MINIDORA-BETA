'採用済みの会話成果を、実行系局所状態と一緒に参照する。\n\nファイル永続化・意味類似検索・任意の照応理解を担わない。\nスナップショットの指紋は整合検査であり、認証や電子署名ではない。\n'
from __future__ import annotations

from copy import deepcopy
from dataclasses import dataclass, fields, is_dataclass, replace
from datetime import datetime
from enum import Enum
import hashlib
import json
import re
from threading import RLock
import unicodedata
from uuid import uuid4

from .HDS適合器 import HDS文脈
from .HDS中間表現 import HDSIR
from .局所解釈 import 局所解釈キャッシュ, 局所解釈スナップショット
from .能力合成 import _結果辞書, _参照結合
from .製品版.型 import 能力結果

文脈照応版 = "MINIDORA-文脈照応-v0.1"
過去対象表層 = r"(?:第[0-9０-９]+応答(?:の[0-9０-９]+番)?|(?:さっきの|直前の|前の)(?:[0-9０-９]+番|結果|回答|応答)?)"
_過去指定 = re.compile(r"第(?P<応答>[0-9]+)応答(?:の(?P<番号>[0-9]+)番)?")
_近傍指定 = re.compile(r"(?:さっきの|直前の|前の)(?:(?P<番号>[0-9]+)番|結果|回答|応答)?")


def _記録値(値: object, 深さ: int = 0) -> object:
    if 深さ > 48:
        raise ValueError("文脈記録の入れ子上限")
    if isinstance(値, Enum):
        return _記録値(値.value, 深さ + 1)
    if 値 is None or type(値) in (str, int, bool, float):
        return 値
    if isinstance(値, datetime):
        return 値.isoformat()
    if is_dataclass(値) and not isinstance(値, type):
        return {f.name: _記録値(getattr(値, f.name), 深さ + 1) for f in fields(値)}
    if type(値) in (tuple, list):
        return [_記録値(v, 深さ + 1) for v in 値]
    if type(値) is dict and all(type(k) is str for k in 値):
        return {k: _記録値(v, 深さ + 1) for k, v in 値.items()}
    raise ValueError("文脈記録不能な型")


def _文脈符号(値: object) -> bytes:
    return json.dumps(_記録値(値), ensure_ascii=True, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _文脈指紋(値: object) -> str:
    return hashlib.sha256(_文脈符号(値)).hexdigest()


@dataclass(frozen=True, slots=True)
class 会話成果:
    応答番号: int
    出力: tuple[tuple[str, 能力結果], ...]
    実行ハッシュ: str


@dataclass(frozen=True, slots=True)
class 照応束縛:
    原文範囲: tuple[int, int]
    表層: str
    文脈識別子: str
    応答番号: int
    出力番号: int
    出力ID: str
    資料キー: str
    結果ハッシュ: str


@dataclass(frozen=True, slots=True)
class 会話参照スナップショット:
    セッションID: str
    所有ID: str
    世代: int
    局所起点: 局所解釈スナップショット
    採用履歴: tuple[会話成果, ...]
    ハッシュ: str = ""

    @property
    def 識別子(self) -> str:
        return f"会話参照:{self.所有ID}:{self.世代}:{self.局所起点.版}:{self.ハッシュ}"

    def 整合確認(self) -> bool:
        try:
            return bool(self.ハッシュ) and self.ハッシュ == _文脈指紋(replace(self, ハッシュ=""))
        except (TypeError, ValueError, RecursionError):
            return False

    def HDS文脈へ(self) -> HDS文脈:
        if not self.整合確認():
            raise ValueError("文脈スナップショットの整合違反")
        s = self.局所起点
        # 本文を指示へ展開せず、焦点集合を識別する参照だけをCompilerへ渡す。
        参照 = self.識別子 if self.採用履歴 else None
        return HDS文脈(記憶版=s.版, 現在焦点=参照, 直前結果=参照,
                       直前IR=deepcopy(s.直前IR), 未解残差=s.未解残差,
                       記憶引用=(self.識別子,), 直前入力=s.直前入力,
                       直前採否=s.直前採否)

    def 解決(self, 表層: str, 原文範囲: tuple[int, int], *, 新規資料あり: bool = False
             ) -> tuple[照応束縛, 能力結果]:
        if not self.整合確認():
            raise ValueError("文脈スナップショットの整合違反")
        if not self.採用履歴:
            raise ValueError("参照可能な採用済み応答がない")
        語 = unicodedata.normalize("NFKC", 表層)
        番号 = None
        if 語 == "それ":
            if 新規資料あり:
                raise ValueError("それの対象が新規資料と会話成果の間で未確定")
            成果 = self.採用履歴[-1]
        else:
            過去 = _過去指定.fullmatch(語)
            近傍 = _近傍指定.fullmatch(語)
            if 過去:
                応答番号 = int(過去.group("応答"))
                成果 = next((x for x in self.採用履歴 if x.応答番号 == 応答番号), None)
                if 成果 is None:
                    raise ValueError("指定応答は存在しないか、採用済み成果がない")
                番号 = 過去.group("番号")
            elif 近傍:
                成果 = self.採用履歴[-1]
                番号 = 近傍.group("番号")
            else:
                raise ValueError("未対応の会話参照表層")
        if 番号 is None:
            if len(成果.出力) != 1:
                raise ValueError("参照先の成果が複数あるため出力番号の指定が必要")
            index = 1
        else:
            index = int(番号)
        if not 1 <= index <= len(成果.出力):
            raise ValueError("指定出力番号が範囲外")
        ID, 値 = 成果.出力[index - 1]
        if not 値.成立 or not 値.本文.strip():
            raise ValueError("参照先に成立した非空の本文がない")
        キー = f"文脈資料:{成果.応答番号:04d}:{index:04d}"
        return (照応束縛(原文範囲, 表層, self.識別子, 成果.応答番号,
                         index, ID, キー, _文脈指紋(_結果辞書(値))), deepcopy(値))


class 会話参照記憶:
    """既存の局所解釈キャッシュを再利用する、一つのセッションの所有境界。"""

    def __init__(self, セッションID: str, *, 最大応答数: int = 64,
                 最大記録バイト数: int = 2_000_000):
        if type(セッションID) is not str or not セッションID.strip() or len(セッションID) > 128:
            raise ValueError("セッションID不正")
        セッションID.encode("utf-8")
        for x in (最大応答数, 最大記録バイト数):
            if type(x) is not int or x <= 0:
                raise ValueError("文脈上限は正の整数")
        self._セッションID = セッションID
        self._所有ID = uuid4().hex
        self._世代 = 0
        self._局所 = 局所解釈キャッシュ()
        self._成果: tuple[会話成果, ...] = ()
        self._上限 = (最大応答数, 最大記録バイト数)
        self._ロック = RLock()

    def _起点(self, 局所=None, 成果=None) -> 会話参照スナップショット:
        s = 会話参照スナップショット(self._セッションID, self._所有ID, self._世代,
            deepcopy((局所 if 局所 is not None else self._局所).起点()),
            deepcopy(成果 if 成果 is not None else self._成果))
        return replace(s, ハッシュ=_文脈指紋(s))

    def 起点(self) -> 会話参照スナップショット:
        with self._ロック:
            return self._起点()

    def 現行確認(self, 起点: 会話参照スナップショット) -> bool:
        with self._ロック:
            return (isinstance(起点, 会話参照スナップショット) and 起点.整合確認()
                    and 起点.識別子 == self._起点().識別子)

    def 初期化(self) -> 会話参照スナップショット:
        with self._ロック:
            self._世代 += 1
            self._局所.初期化()
            self._成果 = ()
            return self._起点()

    def 更新(self, 起点: 会話参照スナップショット, 入力: str, 状態: str, *,
             出力: tuple[tuple[str, 能力結果], ...] = (), ir: HDSIR | None = None,
             実行ハッシュ: str = "") -> 会話参照スナップショット:
        with self._ロック:
            if not self.現行確認(起点):
                raise ValueError("文脈の所有境界または状態版が変わった")
            if 起点.局所起点.版 >= self._上限[0]:
                raise ValueError("会話応答数上限:明示初期化が必要")
            if type(入力) is not str or 状態 not in ("合格", "保留", "失敗", "中止"):
                raise ValueError("会話更新の入力または状態が不正")
            入力.encode("utf-8")
            if ir is not None and (not isinstance(ir, HDSIR) or ir.原文 != 入力):
                raise ValueError("会話更新のIRと入力が不一致")
            if type(実行ハッシュ) is not str or type(出力) is not tuple:
                raise ValueError("会話成果の型不正")
            if 状態 != "合格" and 出力:
                raise ValueError("未採用出力を会話成果として保存しない")
            if 状態 == "合格" and not 出力:
                raise ValueError("合格には最終出力が必要")
            IDs = set()
            for row in 出力:
                if type(row) is not tuple or len(row) != 2:
                    raise ValueError("出力行の型不正")
                ID, 値 = row
                if type(ID) is not str or not ID.strip() or ID in IDs:
                    raise ValueError("出力ID不正または重複")
                IDs.add(ID)
                _結果辞書(値)
                _参照結合(値.参照)
                if not 値.成立:
                    raise ValueError("不成立の出力を採用しない")
            # 候補状態を別に組み立て、すべての検査後に一度だけ所有状態へ反映する。
            候補 = deepcopy(self._局所)
            候補.更新(入力, 状態, deepcopy(出力) if 状態 == "合格" else None, deepcopy(ir))
            成果 = self._成果 + ((会話成果(候補.現在.版, deepcopy(出力), 実行ハッシュ),)
                                   if 状態 == "合格" else ())
            後 = self._起点(候補, 成果)
            if len(_文脈符号(後)) > self._上限[1]:
                raise ValueError("会話記録サイズ上限:明示初期化が必要")
            self._局所, self._成果 = 候補, 成果
            return 後
