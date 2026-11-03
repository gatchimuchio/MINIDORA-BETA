from __future__ import annotations

from typing import Callable, Mapping, Sequence, TYPE_CHECKING

from .HDS実行主体 import HDS実行主体, HDS実行状態, HDS作用器, HDS実行結果, HDS作用供給器, HDS終端
from .統合駆動_v2.政策 import HDS運用政策
from .統合駆動_v2.認識 import HDS認識項目, 認識区分
from .統合駆動_v2.観測 import HDS観測器, HDS観測要求
from .統合駆動_v2.仮説 import HDS仮説, HDS仮説雛型, HDS作業枝
from .統合駆動_v2.依存 import HDS依存辺
from .統合駆動_v2.記憶 import HDS記憶
from .統合駆動_v2.検証 import HDS検証器, HDS草案
from .統合駆動_v2.形成 import HDS形成関係, 形成隔離を審査
from .統合駆動_v2.適応記憶 import HDS適応記憶
from .統合駆動_v2.状態更新 import 有効認識
from .統合駆動_v2.入力境界 import HDS異種表象, HDS異種入力作用

if TYPE_CHECKING:
    from .HDSコア入力 import HDSコア入力束
    from .HDS構文化処理系列_v1_4 import HDSカーネル束, HDS意味専用計画器
    from .HDS非退行包絡 import HDS非退行判定, HDS非退行包絡
    from .参照 import 参照取得診断, 参照記録, 参照記録群を統合, 参照経験記憶を統合, 固定参照供給器

HDS駆動コア版 = "MINIDORA-HDS-FIRST-v7"
HDS継承基準版 = "HDS-MINIDORA-63d5d7e7"

class HDS駆動コア:
    """Compiler Kernelを唯一の意味正本として消費するMINIDORA内部Core。"""

    def __init__(self, *, HDSコンパイラ=None, 最大作用回数: int = 32, 政策: HDS運用政策 | None = None,
                 観測器: Sequence[HDS観測器] = (), 仮説雛型: Sequence[HDS仮説雛型] = (),
                 検証器: Sequence[HDS検証器] = (), 最終検証器: Sequence[HDS検証器] = (),
                 関係規則=(), 未来制約=(), 作用供給器=(), 停止要求=None) -> None:
        self.HDSコンパイラ = HDSコンパイラ
        if type(最大作用回数) is not int or not 1 <= 最大作用回数 <= 4096:
            raise ValueError("最大作用回数は1..4096の整数が必要")
        self.最大作用回数 = 最大作用回数
        self.政策 = 政策
        self.観測器 = tuple(観測器); self.仮説雛型 = tuple(仮説雛型)
        self.検証器 = tuple(検証器); self.最終検証器 = tuple(最終検証器)
        self.関係規則 = tuple(関係規則); self.未来制約 = tuple(未来制約)
        self.作用供給器 = tuple(作用供給器); self.停止要求 = 停止要求
        # Core寿命を学習対象期間とする。結果ではなく通常循環が更新した状態だけを継承する。
        self._継続記憶 = HDS記憶()
        self._継続形成関係: tuple[HDS形成関係, ...] = ()
        self._継続認識: tuple[HDS認識項目, ...] = ()
        self._継続参照記憶: tuple[参照記録, ...] = ()
        self._適応記憶 = HDS適応記憶(4096)
        self._最終認識作業件数 = 0
        from .駆動系.学習 import 関係学習状態
        self._関係学習状態 = 関係学習状態()

    @property
    def 継続状態署名(self) -> str:
        from .コア.値 import 署名
        旧署名 = 署名((
            self._継続記憶,
            self._継続形成関係,
            tuple((x.ID, x.意味署名, x.改訂) for x in self._継続認識),
            tuple((x.識別子, x.供給器, x.由来, x.内容, x.条件, float(x.信頼)) for x in self._継続参照記憶),
            self._適応記憶.状態署名,
        ))
        return 署名((旧署名, self._関係学習状態)) if self._関係学習状態.形成 or self._関係学習状態.残差 else 旧署名

    @property
    def 継続記憶資料件数(self) -> int:
        return len(self._継続記憶.正本)

    @property
    def 継続形成関係件数(self) -> int:
        return len(self._継続形成関係)

    @property
    def 継続認識件数(self) -> int:
        return len(self._継続認識)

    @property
    def 適応経験数(self) -> int:
        return self._適応記憶.経験数

    @property
    def 最終認識作業件数(self) -> int:
        return int(self._最終認識作業件数)

    @property
    def 経路経験数(self) -> int:
        return self._適応記憶.経路経験数

    @property
    def 経路学習状態件数(self) -> tuple[int, int, int]:
        return self._適応記憶.経路状態集計

    @property
    def 継続参照件数(self) -> int:
        return len(self._継続参照記憶)

    @property
    def 継続参照記憶(self) -> tuple[参照記録, ...]:
        return tuple(self._継続参照記憶)

    def 継続状態を初期化(self) -> None:
        self._継続記憶 = HDS記憶()
        self._継続形成関係 = ()
        self._継続認識 = ()
        self._継続参照記憶 = ()
        self._適応記憶.初期化()
        self._最終認識作業件数 = 0
        from .駆動系.学習 import 関係学習状態
        self._関係学習状態 = 関係学習状態()

    def 形成関係を審査(
        self,
        形成ID: str,
        処置: str,
        理由: str,
        承認主体: str,
        反例参照: Sequence[str] = (),
    ) -> HDS形成関係:
        """同一Core内の隔離形成だけを、明示審査で復帰または棄却する。"""
        if not isinstance(形成ID, str) or not 形成ID.strip():
            raise ValueError("形成IDは空でない文字列が必要")
        形成辞書 = {x.ID: x for x in self._継続形成関係}
        対象 = 形成辞書.get(形成ID)
        if 対象 is None:
            raise KeyError("審査対象の形成関係が存在しない: " + 形成ID)
        更新 = 形成隔離を審査(対象, 処置, 理由, 承認主体, tuple(反例参照))
        形成辞書[形成ID] = 更新
        self._継続形成関係 = tuple(形成辞書[k] for k in sorted(形成辞書))
        return 更新

    @staticmethod
    def _認識作業集合(問合せ: str, 正本: Sequence[HDS認識項目], *,
                 初期認識: Sequence[HDS認識項目] = (), 要求認識: Sequence[str] = (),
                 カーネル正本=None) -> tuple[HDS認識項目, ...]:
        """全保持した認識から、現在入力へ接続する作業集合とその依存鎖だけを取り出す。"""
        from .意味字句 import 意味語
        辞書 = {x.ID: x for x in 正本}
        語 = set(意味語(問合せ))
        if カーネル正本 is not None:
            意味IR = getattr(カーネル正本, "意味IR", None)
            for 座標 in tuple(getattr(意味IR, "座標", ())):
                内容 = getattr(座標, "内容", None)
                if isinstance(内容, str):
                    語.update(意味語(内容))

        def 文字列群(値):
            if isinstance(値, str):
                yield 値
            elif isinstance(値, Mapping):
                for k, v in 値.items():
                    yield from 文字列群(k)
                    yield from 文字列群(v)
            elif isinstance(値, (tuple, list, set, frozenset)):
                for item in 値:
                    yield from 文字列群(item)

        def 項目語(x):
            out = set()
            for 値 in (x.対象, x.関係, x.値, *x.条件, x.範囲, x.時点, x.検証契約):
                for text in 文字列群(値):
                    out.update(意味語(text))
            return out

        選択 = {x.ID for x in 初期認識} | {str(x) for x in 要求認識}
        問い正規 = " ".join(str(問合せ).split()).casefold()
        if 語 or 問い正規:
            選択.update(
                x.ID for x in 正本
                if 語.intersection(項目語(x))
                or any(
                    len(語片) >= 2 and 語片.casefold() in 問い正規
                    for 語片 in (str(x.対象), str(x.関係))
                    if str(語片).strip()
                )
            )
        待ち = list(選択)
        while 待ち:
            ID = 待ち.pop()
            項目 = 辞書.get(ID)
            if 項目 is None:
                continue
            for 依存ID in 項目.依存:
                if 依存ID not in 選択:
                    選択.add(依存ID)
                    待ち.append(依存ID)
        return tuple(辞書[k] for k in sorted(選択) if k in 辞書)

    @staticmethod
    def _継続認識を統合(正本候補: Sequence[HDS認識項目], 作業ID: frozenset[str],
                  実行後: Sequence[HDS認識項目], 記憶: HDS記憶) -> tuple[HDS認識項目, ...]:
        """作業集合の更新だけを正本へ戻し、全保持側の有効性を線形依存検査する。"""
        辞書 = {x.ID: x for x in 正本候補 if x.ID not in 作業ID}
        辞書.update((x.ID, x) for x in 実行後)
        資料 = 記憶.正本辞書()
        memo = {}
        visiting = set()

        def 有効(ID):
            if ID in memo:
                return memo[ID]
            if ID in visiting:
                memo[ID] = False
                return False
            x = 辞書.get(ID)
            if x is None or x.区分 != 認識区分.確定 or x.条件 or x.反証:
                memo[ID] = False
                return False
            visiting.add(ID)
            ok = True
            for 根拠 in x.根拠:
                元 = 資料.get(根拠.資料ID)
                if 元 is None or 元.版 != 根拠.版 or 元.内容署名 != 根拠.内容署名:
                    ok = False
                    break
            if ok:
                ok = all(有効(k) for k in x.依存)
            visiting.discard(ID)
            memo[ID] = ok
            return ok

        return tuple(辞書[k] for k in sorted(辞書) if 有効(k))

    def 実行(self, 問合せ: str, *, 目的: Sequence[str] = (), 要求状態: Sequence[str] = (),
           追加作用: Sequence[HDS作用器] = (), 追加作用供給器: Sequence[HDS作用供給器] = (),
           カーネル正本: HDSカーネル束 | None = None, 入力正本: HDSコア入力束 | None = None,
           初期成立状態: Sequence[str] = (), 初期残差: Sequence[str] | None = None,
           初期成果: Mapping[str, object] | None = None, 主体状態: Mapping[str, object] | None = None,
           前回結果: object = None, HDS履歴=(), 文脈=None,
           初期認識: Sequence[HDS認識項目] = (), 要求認識: Sequence[str] = (),
           初期依存: Sequence[HDS依存辺] = (), 観測要求: Sequence[HDS観測要求] = (),
           初期記憶: HDS記憶 | None = None, 初期仮説: Sequence[HDS仮説] = (),
           初期枝: Sequence[HDS作業枝] = (), 初期草案: Sequence[HDS草案] = (),
           形成関係: Sequence[HDS形成関係] | None = None, 異種表象: Sequence[HDS異種表象] = (), 関係要求=None, 入力束=None) -> HDS実行結果:
        if 関係要求 is not None:
            # 構造化入力は既に入力系を通過している。自由文を第二の解釈経路で再解析しない。
            if (目的 or 要求状態 or 追加作用 or 追加作用供給器 or カーネル正本 is not None
                    or 入力正本 is not None or 初期成立状態 or 初期残差 or 初期成果 or 主体状態
                    or 前回結果 is not None or HDS履歴 or 文脈 is not None or 初期認識 or 要求認識
                    or 初期依存 or 観測要求 or 初期記憶 is not None or 初期仮説 or 初期枝
                    or 初期草案 or 形成関係 is not None or 異種表象 or 入力束 is not None):
                raise ValueError("構造化関係要求と旧入口の初期条件を混在させない")
            from .駆動系.接続 import 関係実行へ接続
            return 関係実行へ接続(self, 問合せ, 関係要求)
        if not isinstance(問合せ, str) or not 問合せ.strip():
            raise ValueError("HDS駆動コアの問合せは空でない文字列である必要がある")
        明示要求状態 = tuple(str(x) for x in 要求状態)
        明示残差 = tuple(str(x) for x in (初期残差 or ()))
        明示要求認識 = frozenset(要求認識)
        if not 明示要求状態 and not 明示残差 and not 明示要求認識:
            raise ValueError("HDS駆動コアには要求状態・初期残差・要求認識のいずれかによる明示的な完了条件が必要")

        継続認識辞書 = {x.ID: x for x in self._継続認識}
        for 項目 in tuple(初期認識):
            前 = 継続認識辞書.get(項目.ID)
            if 前 is not None and 前.意味署名 != 項目.意味署名 and 項目.改訂 <= 前.改訂:
                raise ValueError("継続認識を同一改訂以下で無言上書きできない")
            継続認識辞書[項目.ID] = 項目
        継続認識候補 = tuple(継続認識辞書[k] for k in sorted(継続認識辞書))
        初期認識群 = self._認識作業集合(
            問合せ, 継続認識候補, 初期認識=tuple(初期認識), 要求認識=tuple(要求認識),
            カーネル正本=カーネル正本,
        )
        認識作業ID = frozenset(x.ID for x in 初期認識群)
        self._最終認識作業件数 = len(初期認識群)

        作用群: list[HDS作用器] = []
        残差群 = set(明示残差)
        成果初期値 = dict(初期成果 or {})
        主体初期値 = dict(主体状態 or {})
        成立初期値 = set(str(x) for x in 初期成立状態)
        目的初期値 = list(str(x) for x in 目的)

        from .入力系.互換 import 既存入力を準備, 受理入力を準備
        if 入力束 is not None:
            from .入力系.契約 import 入力束 as 入力束型
            if not isinstance(入力束, 入力束型):
                raise TypeError("入力束型が必要")
            if 問合せ != 入力束.原本.本文:
                raise ValueError("入力束と実行原文が異なる")
            if 入力束.原本.選択肢:
                raise ValueError("選択入力は入力選択実行を使用する")
            if (カーネル正本 is not None or 入力正本 is not None or 前回結果 is not None
                    or HDS履歴 or 文脈 is not None):
                raise ValueError("入力束と別の意味正本・解釈文脈は混在できない")
            準備 = 受理入力を準備(
                入力束, 作用群=作用群, 残差群=残差群, 成果初期値=成果初期値,
                主体初期値=主体初期値, 成立初期値=成立初期値, 目的初期値=目的初期値,
            )
        else:
            準備 = 既存入力を準備(
                問合せ, self.HDSコンパイラ, カーネル正本=カーネル正本, 入力正本=入力正本,
                前回結果=前回結果, HDS履歴=HDS履歴, 文脈=文脈,
                作用群=作用群, 残差群=残差群, 成果初期値=成果初期値, 主体初期値=主体初期値,
                成立初期値=成立初期値, 目的初期値=目的初期値,
            )
        作用群 = list(準備.作用群)
        残差群 = set(準備.残差群)
        成果初期値 = dict(準備.成果初期値)
        主体初期値 = dict(準備.主体初期値)
        成立初期値 = set(準備.成立初期値)
        目的初期値 = list(準備.目的初期値)

        if 異種表象:
            残差群.add("異種入力未接続")
            作用群.append(HDS異種入力作用(tuple(異種表象)))
        作用群.extend(tuple(追加作用))

        初期 = HDS実行状態(
            tuple(dict.fromkeys(目的初期値)), frozenset(明示要求状態), frozenset(成立初期値),
            frozenset(残差群), tuple(sorted(成果初期値.items(), key=lambda 行: 行[0])),
            tuple(sorted(主体初期値.items(), key=lambda 行: 行[0])), 0,
            認識=初期認識群, 要求認識=明示要求認識, 依存=tuple(初期依存), 観測要求=tuple(観測要求),
            記憶=初期記憶 if 初期記憶 is not None else self._継続記憶, 仮説=tuple(初期仮説),
            枝=tuple(初期枝), 草案=tuple(初期草案),
            形成関係=self._継続形成関係 if 形成関係 is None else tuple(形成関係),
        )
        結果 = HDS実行主体(
            tuple(作用群), 最大作用回数=self.最大作用回数, 政策=self.政策, 観測器=self.観測器,
            仮説雛型=self.仮説雛型, 検証器=self.検証器, 最終検証器=self.最終検証器,
            関係規則=self.関係規則, 未来制約=self.未来制約,
            作用供給器=(*self.作用供給器, *tuple(追加作用供給器)), 停止要求=self.停止要求,
            適応記憶=self._適応記憶,
        ).実行(初期)
        # SUSPEND/FAILを含め、通常循環で実際に保持・形成された状態は次回処理の前提へ継承する。
        self._継続記憶 = 結果.状態.記憶
        self._継続形成関係 = 結果.状態.形成関係
        self._継続認識 = self._継続認識を統合(
            継続認識候補, 認識作業ID, 結果.状態.認識, 結果.状態.記憶,
        )
        return 結果

    def 出力を構成(self, 結果, **出力引数):
        """最終結果を不変の出力束にする。駆動・学習・表現・送達を再実行しない。"""
        from .駆動系.返却 import 結果から出力を構成
        return 結果から出力を構成(結果, **出力引数)

    def 入力実行(self, 束, **実行引数):
        """受理済み一般入力から実行する。入出力表現は学習状態を所有しない。"""
        from .入力系.射影 import 指示束を検査
        指示束を検査(束)
        if 束.原本.選択肢:
            raise ValueError("選択入力は入力選択実行を使用する")
        if any(k in 実行引数 for k in ("問合せ", "入力束", "カーネル正本", "入力正本", "関係要求", "前回結果", "HDS履歴", "文脈")):
            raise ValueError("入力実行へ別の意味入力を混在できない")
        return self.実行(束.原本.本文, 入力束=束, **実行引数)

    def 入力選択実行(self, 束, **実行引数):
        """既存の選択経路へ、形成済み問題カーネルだけを渡す。参照資料用Compilerは継承する。"""
        from .入力系.射影 import 指示束を検査
        核 = 指示束を検査(束)
        if not 束.原本.選択肢 or 束.方式 != "カーネル":
            raise ValueError("選択肢付きのカーネル入力が必要")
        if any(k in 実行引数 for k in ("問合せ", "選択肢", "カーネル正本")):
            raise ValueError("選択問題の入力を差し替えられない")
        if 束.診断:
            raise ValueError("入力境界の未解決診断がある。旧選択経路へ黙って渡さない")
        return self.選択実行(束.原本.本文, 束.原本.選択肢, カーネル正本=核, **実行引数)

    def 入力関係実行(self, 束, 射影契約):
        """HDS意味正本から明示射影した関係を、同一の駆動・学習経路へ渡す。"""
        from .入力系.関係射影 import 関係要求へ射影
        要求 = 関係要求へ射影(束, 射影契約)
        return self.関係実行(要求)

    @property
    def 関係学習状態(self):
        """不変の学習スナップショット。入力・出力系へ更新権限を渡さない。"""
        return self._関係学習状態

    def 関係実行(self, 要求):
        from .駆動系.契約 import 関係要求, 構造要求
        if not isinstance(要求, (関係要求, 構造要求)):
            raise TypeError("関係要求型が必要")
        return self.実行(要求.目的, 関係要求=要求)

    def 関係形成を隔離(self, ID, 反例):
        from .駆動系.学習 import 関係形成を隔離
        self._関係学習状態 = 関係形成を隔離(self._関係学習状態, ID, 反例)
        return self._関係学習状態

    def 関係形成を再検証(self, ID, 要求):
        from .駆動系.学習 import 関係形成を再検証
        self._関係学習状態 = 関係形成を再検証(self._関係学習状態, ID, 要求)
        return self._関係学習状態

    def 関係形成を審査(self, ID, 処置, 理由, 承認主体, 反例参照):
        from .駆動系.学習 import 関係形成を審査
        self._関係学習状態 = 関係形成を審査(self._関係学習状態, ID, 処置, 理由, 承認主体, tuple(反例参照))
        return self._関係学習状態

    def 非退行継承実行(self, 問合せ: str, *, 基準実行: Callable[[], object], 基準承認判定: Callable[[object], bool],
                   拡張採用証明: Callable[[object, object], bool], 拡張実行: Callable[[], object] | None = None,
                   拡張承認判定: Callable[[object], bool] | None = None, **実行引数) -> HDS非退行判定:
        from .HDS非退行包絡 import HDS非退行包絡
        基準結果 = 基準実行()
        実拡張実行 = 拡張実行 or (lambda: self.実行(問合せ, **実行引数))
        実拡張承認判定 = 拡張承認判定 or (lambda 結果: isinstance(結果, HDS実行結果) and 結果.終端 == HDS終端.採用)
        return HDS非退行包絡(基準結果, 基準承認判定=基準承認判定, 拡張実行=実拡張実行,
                         拡張承認判定=実拡張承認判定, 拡張採用証明=拡張採用証明)

    def 選択実行(self, 問合せ: str, 選択肢: Sequence[str], *, 初期参照=(), 初期参照診断=(), 参照供給器=None,
             計算実行器_=None, 模型核=None, 基礎能力核=None, 既存能力継承: bool = True,
             最大回復回数: int = 6, 拡張採用証明=None,
             カーネル正本: HDSカーネル束 | None = None) -> HDS実行結果:
        if self.HDSコンパイラ is None:
            raise ValueError("選択実行にはHDSコンパイラが必要")
        from .HDS構文化処理系列_v1_4 import HDSカーネル束, HDS意味専用計画器
        from .参照 import 参照取得診断, 参照記録, 参照記録群を統合, 参照経験記憶を統合, 固定参照供給器
        候補 = tuple(str(x) for x in 選択肢)
        if len(候補) < 2:
            raise ValueError("選択実行には2件以上の候補が必要")
        問題束 = カーネル正本
        if 問題束 is not None:
            if not isinstance(問題束, HDSカーネル束):
                raise TypeError("カーネル正本はHDSカーネル束である必要がある")
            束候補 = tuple(
                str(x.内容)
                for x in sorted(
                    (x for x in 問題束.意味IR.座標 if x.座標ID.startswith("選択肢:")),
                    key=lambda x: x.座標ID,
                )
            )
            if 束候補 and 束候補 != 候補:
                raise ValueError("カーネル正本の選択肢と実行引数が一致しない")
        else:
            問題束関数 = getattr(self.HDSコンパイラ, "問題コンパイル束", None)
            if callable(問題束関数):
                問題束 = 問題束関数(問合せ, 候補)
                if not isinstance(問題束, HDSカーネル束):
                    raise TypeError("問題コンパイル束がHDSカーネル束を返さなかった")
            else:
                問題IR関数 = getattr(self.HDSコンパイラ, "問題IR", None)
                問題入力関数 = getattr(self.HDSコンパイラ, "問題コア入力", None)
                if not callable(問題IR関数) or not callable(問題入力関数):
                    raise TypeError("選択実行には問題コンパイル束、または問題IRと問題コア入力が必要")
                from .HDS観測計画 import HDS参照観測要求群
                問題IR = 問題IR関数(問合せ, 候補)
                入力束 = 問題入力関数(問合せ, 候補)
                問題束 = HDSカーネル束(
                    意味IR=問題IR, 計算計画=HDS意味専用計画器().計画(問合せ), コア入力=入力束,
                    参照観測要求=HDS参照観測要求群(問題IR),
                )
        問題IR = 問題束.意味IR
        入力束 = 問題束.コア入力
        from .HDS選択継承循環 import (
            HDS選択継承供給, HDS選択継承設定, 参照成果名, 参照世代成果名,
            計算済み成果名, 参照記憶成果名, 参照取得診断成果名,
            関係観測消費成果名, 関係観測世代成果名, 選択閉包状態, 残差_未評価,
            残差_参照取得障害,
        )
        # 前回までの実観測を現在の問いで検索し直す。回答・採点結果は記憶へ入れない。
        記憶参照: tuple[参照記録, ...] = ()
        if self._継続参照記憶:
            from .HDS参照 import HDS参照検索
            記憶参照 = HDS参照検索(
                固定参照供給器(self._継続参照記憶, 名称="HDS継続参照記憶"),
                問題IR,
                上限=32,
                観測要求=tuple(問題束.参照観測要求),
            )
        初期参照群 = 参照記録群を統合(記憶参照, tuple(初期参照), 最大件数=64)

        入力残差非阻害対象 = tuple(
            f"HDS残差:{項目.種別}:{項目.理由}" for 項目 in 入力束.残差 if 項目.種別 == "未解共参照"
        )
        初期診断 = tuple(初期参照診断)
        if any(not isinstance(x, 参照取得診断) for x in 初期診断):
            raise TypeError("初期参照診断は参照取得診断tupleである必要がある")
        初期選択残差 = [残差_未評価]
        if any(x.状態 in {"失敗", "縮退"} for x in 初期診断):
            初期選択残差.append(残差_参照取得障害)
        供給 = HDS選択継承供給(
            問題束, self.HDSコンパイラ, 初期参照群, 模型核=模型核, 基礎能力核=基礎能力核,
            既存能力継承=既存能力継承, 参照供給器=参照供給器, 計算実行器_=計算実行器_,
            設定=HDS選択継承設定(最大回復回数), 拡張採用証明=拡張採用証明,
            入力残差非阻害対象=入力残差非阻害対象,
        )
        結果 = self.実行(
            問合せ, 目的=("選択問題を閉包する",), 要求状態=(選択閉包状態,), 初期残差=tuple(初期選択残差),
            初期成果={
                参照成果名: 初期参照群,
                参照世代成果名: 0,
                計算済み成果名: False,
                参照記憶成果名: 初期参照群,
                参照取得診断成果名: 初期診断,
                関係観測消費成果名: (),
                関係観測世代成果名: 0,
            },
            カーネル正本=問題束,
            追加作用供給器=(HDS作用供給器("HDS選択継承循環", 供給.構成, "v4"),),
        )
        最終参照 = 結果.状態.成果辞書().get(参照成果名, 初期参照群)
        if isinstance(最終参照, tuple) and all(isinstance(x, 参照記録) for x in 最終参照):
            self._継続参照記憶 = 参照経験記憶を統合(self._継続参照記憶, 最終参照)
        return 結果

__all__ = ["HDS駆動コア版", "HDS継承基準版", "HDS駆動コア"]


def __getattr__(名前):
    from importlib import import_module
    経路 = {
        "HDSコア入力束": "HDSコア入力",
        "HDSカーネル束": "HDS構文化処理系列_v1_4",
        "HDS意味専用計画器": "HDS構文化処理系列_v1_4",
        "HDS非退行判定": "HDS非退行包絡", "HDS非退行包絡": "HDS非退行包絡",
        **{k: "参照" for k in ("参照取得診断", "参照記録", "参照記録群を統合", "参照経験記憶を統合", "固定参照供給器")},
    }
    if 名前 not in 経路:
        raise AttributeError(名前)
    値 = getattr(import_module("." + 経路[名前], __package__), 名前)
    globals()[名前] = 値
    return 値
