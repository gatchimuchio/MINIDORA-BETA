"""画像・音声等の外部適合器後表象を受ける。画素/波形の認識器ではない。"""
from __future__ import annotations
from dataclasses import dataclass
from .値 import 文字, 文字列組, 不変値, 署名
from .記憶 import HDS資料
from .認識 import HDS認識項目, 認識区分


@dataclass(frozen=True, slots=True)
class HDS異種表象:
    ID: str
    種別: str
    適合器版: str
    原資料: HDS資料
    関係群: tuple[tuple[str, str, object], ...]

    def __post_init__(self):
        for n in ("ID", "種別", "適合器版"):
            文字(getattr(self, n), n)
        if not isinstance(self.原資料, HDS資料):
            raise TypeError("適合器由来の原資料記録が必要")
        if not isinstance(self.関係群, tuple):
            raise TypeError("関係群はtuple")
        for 対象, 関係, 値 in self.関係群:
            文字(対象)
            文字(関係)
            不変値(値)


class HDS異種入力作用:
    def __init__(self, 表象群: tuple[HDS異種表象, ...]):
        self.表象群 = 表象群
        if not isinstance(表象群, tuple) or any(not isinstance(x, HDS異種表象) for x in 表象群):
            raise TypeError("異種表象tupleが必要")
        文字列組(tuple(x.ID for x in 表象群))
        self.作用ID = "異種入力接続"

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        return HDS作用機会(self.作用ID, 署名(self.表象群), 出力状態=frozenset({"異種入力接続済み"}),
                            解消対象=frozenset({"異種入力未接続"}), 種別="入力境界")

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        資料 = {}
        項目 = []
        for r in self.表象群:
            if r.原資料.ID in 資料 and 資料[r.原資料.ID] != r.原資料:
                raise ValueError("適合器原資料の版が競合")
            資料[r.原資料.ID] = r.原資料
            for i, (対象, 関係, 値) in enumerate(r.関係群):
                項目.append(HDS認識項目(f"入力/{r.ID}/{i}", 対象, 関係, 値, 認識区分.暫定,
                                       根拠=(r.原資料.出典(),), 条件=(f"適合器検証:{r.適合器版}",)))
        return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"異種入力接続済み"}),
                            解消残差=frozenset({"異種入力未接続"}), 認識更新=tuple(項目),
                            記憶更新=状態.記憶.更新(tuple(資料[k] for k in sorted(資料))),
                            理由=("適合器後の明示表象を接続。実内容の正しさは未確定",))
