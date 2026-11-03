from __future__ import annotations

from dataclasses import dataclass
from typing import Sequence

from .意味字句 import 意味語
from .模型 import (
    _不成立入力の留保結果,
    MINIDORA模型核,
    内部言語状態,
    候補共同参照作用,
    成立候補,
    成立差,
    文脈付き言語状態,
    模型検査点,
    模型結果,
    模型統計,
    関係寄与,
    _コア寄与同一性,
)


@dataclass(frozen=True, slots=True)
class 能力作用記録:
    作用ID: str
    種別: str
    入力状態: str | None
    出力状態: str | None
    条件: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class 能力状態差記録:
    差分ID: str
    原因作用ID: str
    前状態: str
    後状態: str
    変化有無: bool = True


@dataclass(frozen=True, slots=True)
class 能力後続利用記録:
    原因差分ID: str
    成立状態: str
    後続作用ID: str
    追加条件: tuple[str, ...] = ()
    状態条件充足: bool = True


@dataclass(frozen=True, slots=True)
class 能力作用構造:
    作用: tuple[能力作用記録, ...] = ()
    状態差: tuple[能力状態差記録, ...] = ()
    後続利用: tuple[能力後続利用記録, ...] = ()


@dataclass(frozen=True, slots=True)
class 能力候補状態差:
    差分ID: str
    前段: str
    後段: str
    変化候補ID: tuple[str, ...]
    得点変化: tuple[tuple[str, int, int], ...]
    寄与変化: tuple[
        tuple[
            str,
            tuple[tuple[object, ...], ...],
            tuple[tuple[object, ...], ...],
        ],
        ...,
    ] = ()

    @property
    def 変化有無(self) -> bool:
        return bool(self.変化候補ID)

    @property
    def 状態差署名(self) -> tuple[object, ...]:
        """得点だけでなく、寄与構造の増減まで含む再作用用署名。"""
        return (
            self.変化候補ID,
            self.得点変化,
            self.寄与変化,
        )


def _寄与名正規化(name: str) -> str:
    value = str(name)
    if value.startswith("候補共同再照合:"):
        return "候補共同参照"
    return value


def _寄与同一性(item: 関係寄与) -> tuple[object, ...]:
    return _コア寄与同一性(item)


@dataclass
class _循環作業状態:
    寄与: dict[str, list[関係寄与]]
    検査点: list[模型検査点]
    既訪問: set[tuple[object, ...]]
    生成数: int = 0
    再利用数: int = 0
    再活性数: int = 0
    大域再照合数: int = 0
    候補横断更新数: int = 0
    再作用回数: int = 0

    def 追加(self, 候補ID: str, item: 関係寄与) -> bool:
        identity = _寄与同一性(item)
        if any(_寄与同一性(old) == identity for old in self.寄与[候補ID]):
            return False
        self.寄与[候補ID].append(item)
        self.生成数 += 1
        return True

    def 得点(self) -> dict[str, int]:
        return {cid: sum(item.差 for item in rows) for cid, rows in self.寄与.items()}

    def 状態署名(self) -> dict[str, tuple[int, tuple[tuple[object, ...], ...]]]:
        scores = self.得点()
        return {
            cid: (
                scores[cid],
                tuple(sorted((_寄与同一性(item) for item in rows), key=repr)),
            )
            for cid, rows in self.寄与.items()
        }

    def 差分(
        self,
        before: dict[str, tuple[int, tuple[tuple[object, ...], ...]]],
        after: dict[str, tuple[int, tuple[tuple[object, ...], ...]]],
        *,
        前段: str,
        後段: str,
        番号: int,
    ) -> 能力候補状態差:
        変化候補群: list[str] = []
        得点変化群: list[tuple[str, int, int]] = []
        寄与変化群: list[
            tuple[
                str,
                tuple[tuple[object, ...], ...],
                tuple[tuple[object, ...], ...],
            ]
        ] = []
        for 候補ID in sorted(after):
            旧状態 = before.get(候補ID, (0, ()))
            新状態 = after[候補ID]
            if 旧状態 != 新状態:
                変化候補群.append(候補ID)
                得点変化群.append((候補ID, int(旧状態[0]), int(新状態[0])))
                旧寄与集合 = set(旧状態[1])
                新寄与集合 = set(新状態[1])
                寄与変化群.append(
                    (
                        候補ID,
                        tuple(sorted(新寄与集合 - 旧寄与集合, key=repr)),
                        tuple(sorted(旧寄与集合 - 新寄与集合, key=repr)),
                    )
                )
        return 能力候補状態差(
            f"能力状態差:{番号:03d}",
            前段,
            後段,
            tuple(変化候補群),
            tuple(得点変化群),
            tuple(寄与変化群),
        )

    def 記録(self, 段階: str, active: Sequence[str] = (), reuse: Sequence[str] = ()) -> None:
        self.検査点.append(
            模型検査点(
                段階,
                tuple(sorted(self.得点().items())),
                tuple(active),
                tuple(reuse),
            )
        )


class 参照状態差連結作用:
    '参照資料の明示状態差から、追加条件なしで接続できる終端状態だけを候補差へ使う。\n\n    構文化器が「状態条件は満たすが追加条件は未確認」と残した接続は証拠化しない。\n    単一遷移だけでも証拠化せず、状態差が別作用を成立可能にした連結がある場合だけ使う。\n    '

    名称 = "候補共同参照:状態差連結"

    def 評価群(
        self,
        候補群: Sequence[tuple[str, 内部言語状態]],
        作用構造群: Sequence[能力作用構造],
    ) -> dict[str, 関係寄与]:
        scores = {cid: 0 for cid, _ in 候補群}
        証拠: dict[str, list[str]] = {cid: [] for cid, _ in 候補群}

        for structure_index, structure in enumerate(作用構造群):
            actions = {item.作用ID: item for item in structure.作用}
            deltas = {item.差分ID: item for item in structure.状態差 if item.変化有無}
            edges: list[tuple[str, str]] = []
            for link in structure.後続利用:
                if not link.状態条件充足 or link.追加条件:
                    continue
                delta = deltas.get(link.原因差分ID)
                next_作用 = actions.get(link.後続作用ID)
                if delta is None or next_作用 is None:
                    continue
                if delta.後状態 != link.成立状態:
                    continue
                edges.append((delta.原因作用ID, next_作用.作用ID))

            if not edges:
                continue
            sources = {src for src, _ in edges}
            targets = {dst for _, dst in edges}
            terminal_ids = sorted(targets - sources)
            if not terminal_ids:
                continue

            terminal_states = tuple(
                dict.fromkeys(
                    actions[作用_id].出力状態
                    for 作用_id in terminal_ids
                    if 作用_id in actions and actions[作用_id].出力状態
                )
            )
            if not terminal_states:
                continue

            rank: dict[str, int] = {}
            for cid, 候補 in 候補群:
                overlap = 0
                for 状態 in terminal_states:
                    overlap = max(overlap, len(意味語(状態).intersection(候補.意味語集合)))
                rank[cid] = overlap
            maximum = max(rank.values(), default=0)
            top = sorted(cid for cid, value in rank.items() if value == maximum and value > 0)
            if maximum <= 0 or len(top) != 1:
                continue
            cid = top[0]
            scores[cid] += 1
            証拠[cid].append(
                f"状態差連結:{structure_index}:{'|'.join(terminal_states)}"
            )

        return {
            cid: 関係寄与(self.名称, score, tuple(証拠[cid]))
            for cid, score in scores.items()
            if score > 0
        }


class MINIDORA能力状態差模型核(MINIDORA模型核):
    """状態差が存在した時だけ次作用集合を開く現行能力模型核。

    旧模型核の一般関係・形成済み関係・能力作用を保持しつつ、再作用の起動条件を
    「上位候補集合が未訪問」から「直前作用によって候補状態が実際に変化した」へ置き換える。
    """

    def _評価作用付き(
        self,
        文脈: 文脈付き言語状態,
        候補群: Sequence[成立候補],
        *,
        作用構造群: Sequence[能力作用構造] = (),
    ) -> 模型結果:
        if not 候補群:
            raise ValueError("成立差の評価には1候補以上が必要")
        候補ID群 = [item.候補ID for item in 候補群]
        if len(候補ID群) != len(set(候補ID群)):
            raise ValueError("候補IDは評価内で一意である必要がある")

        内部候補群: list[tuple[str, 内部言語状態]] = []
        for 候補 in 候補群:
            if 候補.状態.言語体系 != 文脈.現在.言語体系:
                raise ValueError("候補と言語文脈の言語体系が一致しない")
            内部候補群.append((候補.候補ID, self.言語対応.内部化(候補.状態)))

        incomplete = _不成立入力の留保結果(文脈, tuple(内部候補群))
        if incomplete is not None:
            return incomplete
        work = _循環作業状態({cid: [] for cid in 候補ID群}, [], set())
        状態差履歴: list[能力候補状態差] = []

        for cid, 状態 in 内部候補群:
            for 関係 in self._関係群:
                item = 関係.評価(文脈, 状態)
                if item:
                    work.追加(cid, item)
        work.記録("標準関係", 候補ID群)

        for cid, 状態 in 内部候補群:
            for 関係 in self._形成済み関係群:
                item = 関係.評価(文脈, 状態)
                if item:
                    work.追加(cid, item)
        work.記録("形成済み関係", 候補ID群)

        一次作用前状態 = work.状態署名()

        if 作用構造群:
            結果 = 参照状態差連結作用().評価群(tuple(内部候補群), tuple(作用構造群))
            for cid, item in 結果.items():
                work.追加(cid, item)

        for 作用 in self._能力作用群:
            if hasattr(作用, "評価群"):
                結果 = 作用.評価群(文脈, tuple(内部候補群))
                for cid, item in 結果.items():
                    work.追加(cid, item)
            else:
                for cid, 状態 in 内部候補群:
                    item = 作用.評価(文脈, 状態)
                    if item:
                        work.追加(cid, item)

        一次作用後状態 = work.状態署名()
        一次状態差 = work.差分(
            一次作用前状態,
            一次作用後状態,
            前段="形成済み関係",
            後段="一次能力作用",
            番号=len(状態差履歴),
        )
        if 一次状態差.変化有無:
            状態差履歴.append(一次状態差)
        work.記録(
            "一次能力作用",
            一次状態差.変化候補ID,
            (一次状態差.差分ID,) if 一次状態差.変化有無 else (),
        )

        未処理状態差 = 一次状態差 if 一次状態差.変化有無 else None
        for 循環番号 in range(1, self.最大再作用回数 + 1):
            if 未処理状態差 is None or not 未処理状態差.変化候補ID:
                break

            得点群 = work.得点()
            順位候補 = sorted(候補ID群, key=lambda cid: (-得点群[cid], cid))
            変化候補 = sorted(
                未処理状態差.変化候補ID,
                key=lambda cid: (-得点群[cid], cid),
            )
            # 現在首位を成立境界として保持しつつ、直前作用で変化した候補は全て再作用面へ残す。
            # 1候補だけが変化した場合は、比較境界を失わないよう最強の別候補を一つ加える。
            再作用候補: list[str] = []
            if 順位候補:
                再作用候補.append(順位候補[0])
            for cid in 変化候補:
                if cid not in 再作用候補:
                    再作用候補.append(cid)
            if len(再作用候補) < 2:
                for cid in 順位候補:
                    if cid not in 再作用候補:
                        再作用候補.append(cid)
                        break
            再作用候補群 = tuple(再作用候補)
            if len(再作用候補群) < 2:
                break

            再作用群 = tuple(
                作用
                for 作用 in self._能力作用群
                if hasattr(作用, "再評価群")
                and not (
                    isinstance(作用, 候補共同参照作用)
                    and not 文脈.参照状態
                )
            )
            if not 再作用群:
                break

            再作用署名 = (
                未処理状態差.状態差署名,
                再作用候補群,
                tuple(getattr(作用, "名称", type(作用).__name__) for 作用 in 再作用群),
            )
            if 再作用署名 in work.既訪問:
                break
            work.既訪問.add(再作用署名)
            work.再活性数 += 1
            work.大域再照合数 += 1
            work.再作用回数 += 1

            再作用前状態 = work.状態署名()
            再作用候補行 = tuple(row for row in 内部候補群 if row[0] in 再作用候補群)
            再利用記録: list[str] = [未処理状態差.差分ID]
            for 作用 in 再作用群:
                作用名 = str(getattr(作用, "名称", type(作用).__name__))
                再利用記録.append(作用名)
                # 意味anchorを持つ通常問題では元の全候補を維持し、候補除外による人工差を作らない。
                # 意味anchorを持たない制御的入力だけは、既存の状態差循環契約どおりactive境界を再照合する。
                作用対象 = (
                    tuple(内部候補群)
                    if isinstance(作用, 候補共同参照作用) and 文脈.現在.意味語集合
                    else 再作用候補行
                )
                結果 = 作用.再評価群(文脈, 作用対象, 循環番号)
                for cid, item in 結果.items():
                    if work.追加(cid, item):
                        work.再利用数 += 1

            再作用後状態 = work.状態署名()
            次状態差 = work.差分(
                再作用前状態,
                再作用後状態,
                前段=f"RECONCILE_{循環番号 - 1}" if 循環番号 > 1 else "一次能力作用",
                後段=f"RECONCILE_{循環番号}",
                番号=len(状態差履歴),
            )
            if 次状態差.変化有無:
                状態差履歴.append(次状態差)
                work.候補横断更新数 += len(次状態差.変化候補ID)
            work.記録(
                f"RECONCILE_{循環番号}",
                再作用候補群,
                tuple(再利用記録),
            )
            未処理状態差 = 次状態差 if 次状態差.変化有無 else None

        differences = tuple(
            成立差(cid, sum(item.差 for item in work.寄与[cid]), tuple(work.寄与[cid]))
            for cid in 候補ID群
        )
        maximum = max(item.差 for item in differences)
        top = tuple(item.候補ID for item in differences if item.差 == maximum)
        winner = top[0] if maximum > 0 and len(top) == 1 else None

        参照_prefixes = (
            "参照関係寄与",
            "候補共同参照",
            "候補共同再照合",
        )
        ref_scores = {
            row.候補ID: sum(
                contribution.差
                for contribution in row.寄与
                if contribution.関係名.startswith(参照_prefixes)
            )
            for row in differences
        }
        ref_max = max(ref_scores.values(), default=0)
        ref_top = tuple(cid for cid, value in ref_scores.items() if value == ref_max)
        ref_winner = ref_top[0] if ref_max > 0 and len(ref_top) == 1 else None

        stats = 模型統計(
            work.生成数,
            work.再利用数,
            work.再活性数,
            work.大域再照合数,
            work.候補横断更新数,
            work.再作用回数,
            len(work.検査点),
        )
        return 模型結果(
            文脈,
            differences,
            winner,
            top if len(top) > 1 else (),
            tuple(work.検査点),
            stats,
            ref_winner,
            ref_top if len(ref_top) > 1 else (),
        )

    def 評価(self, 文脈: 文脈付き言語状態, 候補群: Sequence[成立候補]) -> 模型結果:
        return self._評価作用付き(文脈, 候補群)

    def 評価言語状態(
        self,
        現在,
        候補群,
        *,
        履歴=(),
        条件=(),
        参照状態=(),
        作用構造群: Sequence[能力作用構造] = (),
    ) -> 模型結果:
        文脈 = self.文脈化(現在, 履歴, 条件, 参照状態)
        return self._評価作用付き(文脈, 候補群, 作用構造群=作用構造群)


def 標準能力模型核() -> MINIDORA能力状態差模型核:
    from .模型 import (
        参照関係寄与作用,
        意味連続関係,
        有向関係整合,
        条件結合関係,
        肯否整合関係,
        履歴近接関係,
        順序連続関係,
    )

    return MINIDORA能力状態差模型核(
        (
            意味連続関係(),
            順序連続関係(),
            有向関係整合(),
            肯否整合関係(),
            履歴近接関係(),
            条件結合関係(),
        ),
        能力作用群=(参照関係寄与作用(), 候補共同参照作用()),
    )


__all__ = [
    "能力作用記録",
    "能力状態差記録",
    "能力後続利用記録",
    "能力作用構造",
    "能力候補状態差",
    "参照状態差連結作用",
    "MINIDORA能力状態差模型核",
    "標準能力模型核",
]
