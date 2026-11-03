"""HDSの単一通常循環。独立監督・介入モード・第二の採否主体を設けない。"""
from __future__ import annotations
from dataclasses import replace, asdict
from copy import deepcopy
from time import perf_counter_ns
from .値 import 署名
from ..駆動系.取得 import 作用供給を取得, 作用機会を取得
from ..駆動系.変換 import 作用を変換
from ..駆動系.射影 import 作用結果を射影
from .政策 import HDS計装, HDS阻害, HDS作用失敗, 停止理由
from .認識 import 認識区分
from .観測 import 必要観測を構成
from .計画 import HDS作用仕様, 作用列を構成, 目的経路を構成
from .形成 import 形成手順を再利用
from .依存 import HDS依存辺
from .状態更新 import 有効認識, ノード有効
from .意味構成 import 不足意味構成作用, 関係仮説構成作用, 自動分岐作用
from .自動記憶 import 自動記憶圧縮作用
from .自動形成 import 自動経験形成作用, 自動形成文脈
from .未来 import 未来列を構成
from .診断 import 例外を診断
from .目的保持 import (目的契約署名, 目的を観測, 目的進展を判定, 作用が目的経路に属する,
                     目的ノード有効, 目的ノード署名, 進展ノードを取得)
from ..コア.状態操作 import 状態差を受理
from ..コア.検証管理 import 検証器群を実行
from .作用 import 観測作用, 仮説形成作用, 仮説再照合作用, 枝合流作用, 草案検証作用
from .指示接続 import (指示を観測, 指示到達ノード, 指示開始を検査, 指示維持を検査,
                     作用指示を検査, 作用対応を取る, 仕様を指示へ接続,
                     条件の不足元, 指示帰還を構成)


def _目的経路(状態, 作用群, 追加要求=frozenset(), 機会仕様=()):
    群 = {a.作用ID: a.計画仕様 for a in 作用群
         if isinstance(getattr(a, "計画仕様", None), HDS作用仕様)}
    群.update((x.作用ID, x) for x in 機会仕様)
    群 = {k: 仕様を指示へ接続(状態, v) for k, v in 群.items()}
    根 = ({"状態:" + x for x in 状態.要求状態 | frozenset(追加要求)}
          | {"残差:" + x for x in 状態.残差}
          | {"認識:" + x for x in 状態.要求認識} | set(指示到達ノード(状態)))
    return 目的経路を構成(根, tuple(群.values()),
        明示依存=tuple((a.作用ID, tuple(getattr(a, "目的依存", ()))) for a in 作用群))


def _関連仕様(状態, 作用群, 追加要求=frozenset()):
    # 旧呼出契約を保ちつつ、中間依存・生成物・消費者を同じ固定点へ揃える。
    return _目的経路(状態, 作用群, 追加要求).仕様群


def _機会仕様(作用, 機会):
    元 = getattr(作用, "計画仕様", None)
    if not isinstance(元, HDS作用仕様):
        元 = HDS作用仕様(機会.作用ID, 版=機会.契約版)
    return replace(元,
        入力状態=元.入力状態 | 機会.入力状態,
        追加状態=元.追加状態 | 機会.出力状態,
        解消残差=元.解消残差 | 機会.解消対象,
        読取認識=tuple(sorted(set(元.読取認識) | set(機会.読取認識))),
        読取成果=tuple(sorted(set(元.読取成果) | set(機会.読取成果))),
        読取ノード=tuple(sorted(set(元.読取ノード) | set(機会.読取ノード))),
        生成ノード=tuple(sorted(set(元.生成ノード) | set(機会.生成ノード)
                              | {"認識:" + x for x in 機会.識別対象})),
        目的依存=tuple(sorted(set(元.目的依存) | set(機会.目的依存))),
        必要権限=tuple(sorted(set(元.必要権限) | set(機会.必要権限))),
        資源負荷=max(元.資源負荷, 機会.資源負荷))


def _期待を計画仕様へ反映(仕様, 機会):
    """契約効果を保持したまま、実行内期待だけを一時的な計画仕様へ合成する。"""
    効果 = 機会.期待
    if 効果.空:
        return replace(仕様, 入力状態=機会.入力状態, 読取認識=機会.読取認識,
                       読取成果=機会.読取成果, 読取ノード=機会.読取ノード,
                       必要権限=機会.必要権限)
    追加状態 = (仕様.追加状態 | 効果.追加状態) - 仕様.削除状態
    削除状態 = (仕様.削除状態 | 効果.削除状態) - 仕様.追加状態
    解消残差 = (仕様.解消残差 | 効果.解消残差) - 仕様.追加残差
    追加残差 = (仕様.追加残差 | 効果.追加残差) - 仕様.解消残差
    return replace(仕様, 入力状態=機会.入力状態, 読取認識=機会.読取認識,
                   読取成果=機会.読取成果, 読取ノード=機会.読取ノード,
                   必要権限=機会.必要権限, 追加状態=追加状態, 削除状態=削除状態,
                   解消残差=解消残差, 追加残差=追加残差)


def 通常循環(主体, 初期状態, 前回=None):
    from ..HDS実行主体 import HDS実行状態, HDS実行結果, HDS作用記録, HDS作用機会, HDS作用結果, HDS作用状態, HDS終端
    if not isinstance(初期状態, HDS実行状態):
        raise TypeError("HDS実行状態が必要")
    現在 = deepcopy(初期状態)
    現在.状態署名
    履歴 = list(前回.履歴 if 前回 is not None else ())
    阻害履歴 = list(前回.阻害履歴 if 前回 is not None else ())
    統計 = asdict(前回.計装 if 前回 is not None else HDS計装())
    観測待ち = ()
    政策 = 主体.政策
    軟予算 = min(主体.最大作用回数, max(政策.初期作用予算, len(履歴)))
    # 外部入力で再開時の状態が本当に変わった場合、閉包の再構築に初期枠を与える。
    # これは目的進展の認定ではなく、ハード上限・探索契約・使用済み署名は維持する。
    if 前回 is not None and 初期状態.状態署名 != 前回.状態.状態署名:
        # 再評価では原資料の再読取から最終検証まで複数作用が必要になる。
        # 進展を捏造せず、既存政策の初期枠と増分の合計だけを再開枠とする。
        軟予算 = min(主体.最大作用回数, len(履歴) + 政策.初期作用予算 + 政策.予算増分)
    生成署名 = set()
    計画署名 = set()
    最終検証済 = set()
    if 主体.最終検証器:
        契約署名 = 署名((現在.状態署名, tuple((v.ID, v.版) for v in 主体.最終検証器)))
        if any(h.作用ID == "内的/目的検証" and h.作用入力署名 == 契約署名
               and h.作用状態 == HDS作用状態.成立 and h.阻害 is None for h in 履歴):
            最終検証済.add(現在.状態署名)
    失敗入力 = {}
    for h in 履歴:
        if h.阻害:
            失敗入力[h.作用ID] = h.阻害
    使用済み = {(h.作用ID, h.作用入力署名) for h in 履歴}
    形成試行済 = False
    圧縮作用 = 自動記憶圧縮作用(政策.圧縮開始文字数, 政策.圧縮最大文字数)
    適応記憶 = 主体.適応記憶
    目的契約 = 目的契約署名(現在)
    初期目的観測 = 目的を観測(現在, 有効認識, 契約署名=目的契約)
    最良直接尺度 = 初期目的観測.直接尺度
    計画最良残数 = {}
    進展台帳 = {n for h in 履歴 for n in h.進展根拠}
    必要ノード = frozenset()
    最終経路 = None
    無進展入力 = {}
    探索消費 = {}
    探索契約署名 = {}
    for 行 in 履歴:
        if 行.探索契約ID:
            回数, 資源 = 探索消費.get(行.探索契約ID, (0, 0))
            探索消費[行.探索契約ID] = (回数 + 1, 資源 + 行.消費資源)
            探索契約署名[行.探索契約ID] = 行.探索契約署名
    利用作用群 = tuple(主体.作用群)
    指示除外 = set()
    対応履歴 = {(h.作用ID, h.対応契約版): h.作用対応 for h in 履歴 if h.作用対応 is not None}
    指示開始署名 = 前回.指示開始署名 if 前回 else ""
    保守待ち = ()
    # 再開時も、外部入力による状態変更と目的変更を混同しない。
    if 前回 is not None and 目的契約署名(前回.状態) != 目的契約:
        raise ValueError("再開APIで目的契約を書換えられない")

    def 終了(終端, 種別, 旧理由):
        return HDS実行結果(終端, 現在, tuple(履歴), tuple(旧理由), 種別, HDS計装(**統計),
            観測待ち, tuple(阻害履歴), 前回.入力履歴 if 前回 is not None else (),
            目的経路=最終経路,
            指示帰還=指示帰還を構成(現在, 採用=終端 == HDS終端.採用, 保守待ち=保守待ち),
            指示除外=tuple(sorted(指示除外)), 指示開始署名=指示開始署名)

    def 契約失敗(対象, 例外):
        阻害履歴.append(HDS阻害(停止理由.契約違反, 対象, f"{type(例外).__name__}: {例外}"))
        return 終了(HDS終端.失敗, 停止理由.契約違反, ("HDS_CONTRACT_VIOLATION",))

    def 探索残枠(契約, 資源負荷):
        回数, 資源 = 探索消費.get(契約.ID, (0, 0))
        return 回数 < 契約.最大試行 and 資源 + 資源負荷 <= 契約.最大資源

    def 記録する(機会, 結果, 計画=(), 未来=(), 計画仕様=None, 探索=None):
        nonlocal 現在, 最良直接尺度
        前 = 現在
        指示前 = 指示を観測(前)
        読取 = {"認識:" + k for k in (*機会.読取認識, *機会.未確定読取)} | {"成果:" + k for k in 機会.読取成果} | {"状態:" + k for k in 機会.入力状態}
        # 座標登録・観測要求の準備を、確定事実の依存辺へ置き換えない。
        読取 |= {n for n in 機会.読取ノード if not n.startswith(("残差:", "認識座標:", "観測要求:", "指示条件:"))}
        読取 |= {n for n in 条件の不足元(前, 機会.読取ノード)
                 if not n.startswith(("残差:", "認識座標:", "観測要求:"))}
        産出 = ({"認識:" + x.ID for x in 結果.認識更新}
                | {"仮説:" + x.ID for x in 結果.仮説更新}
                | {"成果:" + k for k, _ in 結果.成果}
                | {"状態:" + k for k in 結果.追加状態}
                | {"枝:" + x.ID for x in 結果.枝更新}
                | {"草案:" + x.ID for x in 結果.草案更新}
                | {"形成:" + x.ID for x in 結果.形成更新})
        更新例外 = None
        目的進展 = False
        進展根拠 = ()
        射影開始 = perf_counter_ns()
        try:
            前目的 = 目的を観測(前, 有効認識, 契約署名=目的契約, 必要ノード=必要ノード)
            現在, 差, 結果 = 作用結果を射影(前, 結果, 読取, 産出)
            if 指示維持を検査(現在):
                raise ValueError("作用結果が指示の維持境界を越えた:" + ",".join(指示維持を検査(現在)))
            後目的 = 目的を観測(現在, 有効認識, 契約署名=目的契約, 必要ノード=必要ノード)
            進展根拠 = tuple(sorted(進展ノードを取得(前目的, 後目的) - 進展台帳))
            目的進展, 最良直接尺度 = 目的進展を判定(
                前観測=前目的, 後観測=後目的, 最良直接尺度=最良直接尺度,
                計画長=len(計画), 計画仕様=計画仕様, 状態差=差,
                計画最良残数=計画最良残数, 進展台帳=進展台帳,
            )
        except Exception as exc:
            更新例外 = exc
            結果 = HDS作用結果(HDS作用状態.失敗, 停止要求=True,
                               理由=(f"{type(exc).__name__}: {exc}",),
                               阻害=HDS阻害(停止理由.契約違反, 機会.作用ID, str(exc)))
            現在, 差 = 状態差を受理(前, 結果)
        統計["射影時間ns"] += perf_counter_ns() - 射影開始
        if not 目的進展:
            無進展入力[機会.作用ID] = 署名(tuple((n, 目的ノード署名(現在, n)) for n in sorted(必要ノード)))
        else:
            無進展入力.pop(機会.作用ID, None)
        消費 = tuple(sorted(set(機会.読取認識) | set(機会.未確定読取)))
        行 = HDS作用記録(len(履歴) + 1, 機会.作用ID, 機会.作用入力署名, 結果.状態,
                          前.状態署名, 現在.状態署名, 差,
                          tuple(sorted(前.残差 & 機会.解消対象)), tuple(sorted(前.未達状態 & 機会.出力状態)),
                          結果.理由, 消費, 機会.資源負荷, 結果.阻害, tuple(計画), tuple(未来), 結果.診断, 目的進展, 目的契約, 機会.目的依存, tuple(sorted(必要ノード)), 進展根拠)
        元仕様 = next((getattr(a, "計画仕様", None) for a in 利用作用群 if a.作用ID == 機会.作用ID), None)
        対応 = 作用対応を取る(前, 機会.作用ID, 元仕様)
        行 = replace(行, 指示前観測=指示前, 指示後観測=指示を観測(現在),
                   対応座標=(*対応.対象座標, 対応.作用座標) if 対応 else (),
                   作用対応=対応, 対応契約版=元仕様.版 if 元仕様 is not None else 機会.契約版,
                   作用定義ID=str(機会.作用定義ID or 機会.作用ID))
        if 探索 is not None:
            行 = replace(行, 探索契約ID=探索.ID, 探索契約署名=署名(探索))
            回数, 資源 = 探索消費.get(探索.ID, (0, 0))
            探索消費[探索.ID] = (回数 + 1, 資源 + 機会.資源負荷)
            統計["探索実行数"] += 1
        直前変化 = set(履歴[-1].状態差.影響対象) if 履歴 else set()
        履歴.append(行)
        適応記憶.結果を受け取る(機会, 結果, 差, 前)
        使用済み.add((機会.作用ID, 機会.作用入力署名))
        統計["作用実行数"] += 1
        統計["消費資源"] += 機会.資源負荷
        統計["状態差発生数"] += int(差.変化有無)
        統計["目的進展数"] += int(目的進展)
        統計["目的無進展数"] += int(not 目的進展)
        統計["後続消費数"] += len(直前変化 & 読取)
        統計["依存失効数"] += len(差.失効対象)
        if 機会.種別 == "観測":
            統計["観測実行数"] += 1
        if 機会.種別 == "大域再照合":
            統計["大域再照合数"] += 1
        if 結果.診断:
            統計["失敗診断数"] += 1
        for 種, 名 in (("意味構成", "意味構成数"), ("仮説形成", "仮説生成数"), ("枝生成", "枝生成数"), ("記憶圧縮", "圧縮数")):
            if 機会.種別 == 種:
                統計[名] += 1
        if 未来:
            統計["未来監査数"] += 1
        if 結果.阻害:
            阻害履歴.append(結果.阻害)
            失敗入力[機会.作用ID] = 結果.阻害
            if 結果.阻害.種別 == 停止理由.検証不成立:
                統計["検証失敗数"] += 1
        elif 結果.状態 == HDS作用状態.成立:
            失敗入力.pop(機会.作用ID, None)
        if 更新例外 is not None:
            raise 更新例外
        return 行

    while True:
        # 停止と供給も同じ通常循環の境界。上位の第二実行主体は置かない。
        try:
            if 主体.停止要求 is not None:
                停止 = 主体.停止要求()
                if type(停止) is not bool:
                    raise TypeError("停止要求はboolを返す必要がある")
                if 停止:
                    return 終了(HDS終端.保留, 停止理由.明示停止, ("HDS_USER_CANCELLED",))
        except Exception as exc:
            return 契約失敗("作用供給・停止境界", exc)
        if 現在.指示関係 is not None:
            try:
                if 指示開始署名 and 指示開始署名 != 現在.指示関係.署名:
                    raise ValueError("開始済みの指示契約が変更された")
                # 開始必要性は一回、原本・検証器の接続契約は再開後も毎回照合する。
                不足 = 指示開始を検査(現在, 主体, 開始条件=not bool(指示開始署名))
                if 不足:
                    return 終了(HDS終端.保留, 停止理由.依存未閉包, ("HDS_INSTRUCTION_START_UNRESOLVED", *不足))
                if not 指示開始署名:
                    指示開始署名 = 現在.指示関係.署名
                不足 = 指示維持を検査(現在)
                if 不足:
                    return 終了(HDS終端.保留, 停止理由.依存未閉包, ("HDS_INSTRUCTION_BOUNDARY_UNRESOLVED", *不足))
            except Exception as exc:
                return 契約失敗("指示接続", exc)
        if any(c.違反(現在.成立状態) for c in 主体.未来制約):
            阻害履歴.append(HDS阻害(停止理由.契約違反, "状態制約", "現在状態が明示不変条件に違反"))
            return 終了(HDS終端.失敗, 停止理由.契約違反, ("HDS_STATE_INVARIANT_VIOLATION",))
        if 現在.閉包済み:
            # 目的成立後はまず検証を閉じる。学習・圧縮を再び通常循環の目的へ昇格させない。
            if 主体.最終検証器 and 現在.状態署名 not in 最終検証済:
                if len(履歴) >= 主体.最大作用回数 or 統計["消費資源"] + 1 > 政策.最大資源:
                    return 終了(HDS終端.保留, 停止理由.予算枯渇, ("HDS_FINAL_VALIDATION_BUDGET_EXHAUSTED",))
                前署名 = 現在.状態署名
                機会 = HDS作用機会("内的/目的検証", 署名((前署名, tuple((v.ID, v.版) for v in 主体.最終検証器))), 種別="目的検証")
                if (機会.作用ID, 機会.作用入力署名) in 使用済み:
                    return 終了(HDS終端.保留, 停止理由.検証不成立, ("HDS_FINAL_VALIDATION_REPEATED",))
                try:
                    検証失敗 = list(検証器群を実行(現在, 主体.最終検証器, None))
                except Exception as exc:
                    記録する(機会, HDS作用結果(HDS作用状態.失敗, 停止要求=True,
                        理由=(f"{type(exc).__name__}: {exc}",),
                        阻害=HDS阻害(停止理由.契約違反, 機会.作用ID, str(exc) or type(exc).__name__)))
                    return 契約失敗("内的/目的検証", exc)
                結果 = HDS作用結果(HDS作用状態.成立,
                                   追加残差=frozenset("検証:" + k for k in 検証失敗),
                                   理由=("目的契約検証",),
                                   阻害=HDS阻害(停止理由.検証不成立, "内的/目的検証", ",".join(検証失敗), True) if 検証失敗 else None)
                記録する(機会, 結果)
                if 検証失敗:
                    continue
                最終検証済.add(現在.状態署名)

            if 現在.指示関係 is not None and 現在.指示関係.返却優先:
                # ここで保守を実行したり非同期開始を装ったりしない。履歴と原本を返す。
                保守待ち = ("任意の記憶圧縮",) + (("任意の経験形成",) if 政策.自動形成 else ())
                return 終了(HDS終端.採用, 停止理由.目的達成, ("HDS_GOAL_CLOSED",))
            # 閉包後処理は各1回だけ。実行しても通常作用選択へ戻らず、そのまま採用する。
            圧縮機会 = 圧縮作用.機会(現在)
            if 圧縮機会 is not None and (圧縮機会.作用ID, 圧縮機会.作用入力署名) not in 使用済み:
                if len(履歴) < 主体.最大作用回数 and 統計["消費資源"] + 圧縮機会.資源負荷 <= 政策.最大資源:
                    記録する(圧縮機会, 圧縮作用.実行(deepcopy(現在)))
            if 政策.自動形成 and not 形成試行済 and 前回 is None:
                形成試行済 = True
                形成 = 自動経験形成作用(初期状態, tuple(履歴), 利用作用群, 主体.最終検証器)
                機会 = 形成.機会(現在)
                if 機会 is not None and len(履歴) < 主体.最大作用回数 and 統計["消費資源"] + 機会.資源負荷 <= 政策.最大資源:
                    記録する(機会, 形成.実行(deepcopy(現在)))
                    統計["自動形成数"] += 1
                    統計["再現作用数"] += 形成.再現回数
                    統計["形成再検証数"] += int(形成.再現成功)
            return 終了(HDS終端.採用, 停止理由.目的達成, ("HDS_GOAL_CLOSED",))

        if len(履歴) >= 主体.最大作用回数 or 統計["消費資源"] >= 政策.最大資源:
            return 終了(HDS終端.保留, 停止理由.予算枯渇, ("HDS_ACTION_BUDGET_EXHAUSTED",))
        軟予算保留 = False
        if len(履歴) >= 軟予算:
            if any(x.目的進展 for x in 履歴[-政策.予算増分:]):
                軟予算 = min(主体.最大作用回数, 軟予算 + 政策.予算増分)
                統計["予算拡張数"] += 1
            else:
                # 進展と探索許可は別。有限の探索が選択された場合だけ1作用分を認める。
                軟予算保留 = True

        try:
            供給開始 = perf_counter_ns()
            try:
                統計["供給検討数"] += len(主体.作用供給器)
                利用作用群 = 作用供給を取得(主体, 現在, 政策)
            finally:
                統計["供給時間ns"] += perf_counter_ns() - 供給開始
            if 利用作用群 is None:
                return 終了(HDS終端.保留, 停止理由.予算枯渇, ("HDS_SUPPLY_CAPACITY_EXHAUSTED",))
        except Exception as exc:
            return 契約失敗("作用供給", exc)
        修復状態 = frozenset(k for b in 失敗入力.values() if b.修復可能 for k in b.必要状態)
        try:
            最終経路 = _目的経路(現在, 利用作用群, 修復状態)
        except Exception as exc:
            return 契約失敗("目的と作用の対応構成", exc)
        関連仕様 = 最終経路.仕様群
        関連作用ID = 最終経路.関連作用ID
        必要ノード = 最終経路.必要ノード | 条件の不足元(現在, 最終経路.必要ノード)
        必要認識 = 現在.要求認識 | frozenset(n.split(":", 1)[1] for n in 必要ノード if n.startswith("認識:"))
        必要認識 |= frozenset(k for b in 失敗入力.values() if b.修復可能 for k in b.必要認識)
        待ち = list(必要認識)
        while 待ち:
            x = 現在.認識辞書().get(待ち.pop())
            for k in x.依存 if x is not None else ():
                if k not in 必要認識:
                    必要認識 = 必要認識 | {k}
                    待ち.append(k)
        必要ノード |= frozenset("認識:" + k for k in 必要認識)
        必要ノード |= frozenset("認識座標:" + k for k in 必要認識)
        try:
            観測用認識 = tuple(replace(x, 区分=認識区分.失効) if x.区分 == 認識区分.確定 and not 有効認識(現在, x.ID) else x for x in 現在.認識)
            観測待ち = 必要観測を構成(観測用認識, 必要認識, 現在.仮説, 現在.観測要求, 最大件数=政策.最大観測要求)
        except ValueError as exc:
            阻害履歴.append(HDS阻害(停止理由.予算枯渇, "観測構成", str(exc)))
            return 終了(HDS終端.保留, 停止理由.予算枯渇, ("HDS_OBSERVATION_CAPACITY_EXHAUSTED",))
        内的 = [観測作用(r, p, 政策.参照再利用回数) for r in 観測待ち for p in 主体.観測器 if p.手段 in r.手段]
        内的.append(不足意味構成作用())
        if 必要認識:
            if 主体.関係規則:
                内的.append(関係仮説構成作用(主体.関係規則, 政策.最大内部生成))
            内的.append(自動分岐作用())
            内的.extend(仮説形成作用(t) for t in 主体.仮説雛型)
            内的.append(枝合流作用())
        内的.extend(草案検証作用(d.ID, 主体.検証器) for d in 現在.草案 if d.区分 in ("未検証", "失効"))
        差による再照合 = bool(履歴 and (履歴[-1].状態差.認識差 or 履歴[-1].状態差.変更依存))
        if 必要認識 and (len(履歴) % 政策.大域間隔 == 0 or 差による再照合 or any(x.区分 == 認識区分.競合 for x in 現在.認識 if x.ID in 必要認識)):
            内的.append(仮説再照合作用())
        全作用 = tuple(利用作用群) + tuple(内的)
        ID別 = {}
        全機会 = {}
        選択可能 = []
        制約除外 = []
        資源除外 = False
        依存除外 = False
        機会仕様別 = {}
        # 未記述の作用を先に調べ、その読取契約も加えてから完全契約を逆算する。
        順序 = sorted(全作用, key=lambda a: bool(getattr(getattr(a, "計画仕様", None), "契約完全", False)))
        静的再構成済み = False
        try:
            for a in 順序:
                if a.作用ID in ID別:
                    raise ValueError("動的生成作用IDの重複: " + a.作用ID)
                ID別[a.作用ID] = a
                spec = getattr(a, "計画仕様", None)
                if isinstance(spec, HDS作用仕様):
                    spec = 仕様を指示へ接続(現在, spec)
                if a not in 内的:
                    対応 = 作用対応を取る(現在, a.作用ID, spec)
                    if 対応 is not None:
                        鍵 = (a.作用ID, spec.版 if isinstance(spec, HDS作用仕様) else "v1")
                        if 鍵 in 対応履歴 and 対応履歴[鍵] != 対応:
                            raise ValueError("同じ作用・契約版の対象対応を無言変更できない")
                        対応履歴[鍵] = 対応
                    不足 = 作用指示を検査(現在, a.作用ID, spec)
                    if 不足:
                        指示除外.add((a.作用ID, 不足))
                        # 別の作用が条件を成立させた後に再検討できる。無関係とは断定しない。
                        continue
                if isinstance(spec, HDS作用仕様) and spec.契約完全:
                    if not 静的再構成済み:
                        最終経路 = _目的経路(現在, 利用作用群, 修復状態, tuple(機会仕様別.values()))
                        静的再構成済み = True
                    if a.作用ID in 最終経路.無関係作用:
                        統計["構成前除外数"] += 1
                        continue
                if 統計["機会検討数"] >= 政策.最大機会検討:
                    return 終了(HDS終端.保留, 停止理由.予算枯渇, ("HDS_OPPORTUNITY_BUDGET_EXHAUSTED",))
                検討開始 = perf_counter_ns()
                try:
                    統計["機会検討数"] += 1
                    o = 作用機会を取得(a, 現在)
                finally:
                    統計["機会時間ns"] += perf_counter_ns() - 検討開始
                if o is None:
                    continue
                spec = getattr(a, "計画仕様", None)
                if isinstance(spec, HDS作用仕様):
                    spec = 仕様を指示へ接続(現在, spec)
                権限 = tuple(sorted(set(o.必要権限) | (set(spec.必要権限) if isinstance(spec, HDS作用仕様) else set())))
                過去阻害 = 失敗入力.get(a.作用ID)
                修復入力 = tuple(過去阻害.必要状態) if 過去阻害 and 過去阻害.修復可能 else ()
                修復認識 = tuple(過去阻害.必要認識) if 過去阻害 and 過去阻害.修復可能 else ()
                読取認識 = tuple(sorted(set(o.読取認識) | set(修復認識)
                                     | (set(spec.読取認識) if isinstance(spec, HDS作用仕様) else set())))
                入力状態 = o.入力状態 | frozenset(修復入力) | (spec.入力状態 if isinstance(spec, HDS作用仕様) else frozenset())
                読取成果 = tuple(sorted(set(o.読取成果) | (set(spec.読取成果) if isinstance(spec, HDS作用仕様) else set())))
                読取ノード = tuple(sorted(set(o.読取ノード) | (set(spec.読取ノード) if isinstance(spec, HDS作用仕様) else set())))
                観測入力 = spec.観測入力 if isinstance(spec, HDS作用仕様) else ()
                探索 = spec.探索 if isinstance(spec, HDS作用仕様) else None
                if 探索 is not None:
                    契約値 = 署名(探索)
                    if 探索.ID in 探索契約署名 and 探索契約署名[探索.ID] != 契約値:
                        raise ValueError("同一目的内で探索契約と上限を無言変更できない")
                    探索契約署名[探索.ID] = 契約値
                    # 回数は目的・契約ごとの実行記録から取る。作用側のID/署名更新で上限を増やさない。
                    o = replace(o, 作用入力署名=署名((o.作用入力署名, 探索.ID, 探索消費.get(探索.ID, (0, 0))[0])))
                入力 = (o.作用入力署名, o.契約版,
                        tuple((k, 現在.ノード署名("認識:" + k)) for k in (*読取認識, *o.未確定読取)),
                        tuple((k, 現在.ノード署名("成果:" + k)) for k in 読取成果),
                        tuple((k, 目的ノード署名(現在, k)) for k in 読取ノード),
                        tuple((k, 目的ノード署名(現在, k)) for k in 観測入力),
                        tuple((k, 現在.ノード署名("状態:" + k)) for k in sorted(入力状態)))
                準備済 = (入力状態 <= 現在.成立状態
                        and all(有効認識(現在, k) for k in 読取認識)
                        and all(ノード有効(現在, "成果:" + k) for k in 読取成果)
                        and all(目的ノード有効(現在, k) for k in (*読取ノード, *観測入力)))
                意味入力 = o.意味入力署名
                if 準備済:
                    意味署名関数 = getattr(a, "意味入力を署名", None)
                    if callable(意味署名関数):
                        意味入力 = 意味署名関数(現在)
                    if 修復入力 or 修復認識:
                        意味入力 = 署名((意味入力,
                            tuple(現在.ノード署名("認識:" + k) for k in 修復認識),
                            tuple(現在.ノード署名("状態:" + k) for k in sorted(修復入力))))
                o = replace(o, 作用入力署名=署名(入力), 意味入力署名=意味入力,
                            必要権限=権限, 読取認識=読取認識, 読取成果=読取成果, 読取ノード=読取ノード, 入力状態=入力状態)
                # 実入力・権限・修復条件が成立した場合だけ期待効果を適用する。
                if 準備済:
                    o = 適応記憶.機会を補正(o)
                全機会[o.作用ID] = o
                機会仕様別[o.作用ID] = 仕様を指示へ接続(現在, _機会仕様(a, o))
                if o.作用ID.startswith("内的/"):
                    鍵 = (o.作用ID, o.作用入力署名)
                    if 鍵 not in 生成署名:
                        生成署名.add(鍵)
                        統計["生成件数"] += 1
                制約 = 政策.許可判定(o.作用ID, o.必要権限)
                if 制約:
                    制約除外.append((o, 制約))
                    continue
                if not o.状態変更可能 or (o.作用ID, o.作用入力署名) in 使用済み:
                    continue
                if not 準備済:
                    依存除外 = True
                    continue
                if o.資源負荷 > 政策.最大資源 - 統計["消費資源"]:
                    資源除外 = True
                    continue
                if isinstance(spec, HDS作用仕様) and 主体.未来制約:
                    計画仕様 = _期待を計画仕様へ反映(spec, o)
                    projected = (現在.成立状態 - 計画仕様.削除状態) | 計画仕様.追加状態
                    if any(c.違反(projected) for c in 主体.未来制約):
                        制約除外.append((o, 停止理由.検証不成立))
                        continue
                選択可能.append(o)
        except Exception as exc:
            return 契約失敗("作用機会観測", exc)
        最終経路 = _目的経路(現在, 利用作用群, 修復状態, tuple(機会仕様別.values()))
        関連仕様, 関連作用ID = 最終経路.仕様群, 最終経路.関連作用ID
        必要ノード |= 最終経路.必要ノード | 条件の不足元(現在, 最終経路.必要ノード)
        必要認識 |= frozenset(n.split(":", 1)[1] for n in 必要ノード if n.startswith("認識:"))
        必要ノード |= frozenset("認識座標:" + k for k in 必要認識)
        関連選択 = []
        進展入力署名 = 署名(tuple((n, 目的ノード署名(現在, n)) for n in sorted(必要ノード)))
        for o in 選択可能:
            仕様 = 機会仕様別.get(o.作用ID)
            探索 = 仕様.探索 if 仕様 is not None else None
            探索可能 = False
            if 探索 is not None:
                接続 = (set(探索.利用先) & 必要ノード
                        and any(not 目的ノード有効(現在, n) for n in 探索.利用先 if n in 必要ノード)
                        and (探索.再観測 or any(not 目的ノード有効(現在, n) for n in 探索.取得ノード)))
                探索可能 = bool(接続 and 探索残枠(探索, o.資源負荷))
                if not 探索可能:
                    統計["探索上限除外数"] += 1
                    continue
            if not 作用が目的経路に属する(現在, o, 認識有効判定=有効認識,
                    関連作用ID=関連作用ID, 必要認識=必要認識, 修復状態=修復状態,
                    計画仕様=機会仕様別.get(o.作用ID), 必要ノード=必要ノード):
                統計["目的関連除外数"] += 1
                continue
            if 無進展入力.get(o.作用ID) == 進展入力署名 and not 探索可能:
                # 旧契約互換: 同一作用が宣言した中間状態を実際に新規形成し、
                # その作用自身が現在の要求状態の産出も宣言している場合だけ、
                # 残存する通常予算内で次の段階を一度検討する。
                # 目的進展の計上や予算延長には使わない。
                直前 = 履歴[-1] if 履歴 else None
                段階作用継続 = bool(
                    直前 is not None and 直前.作用ID == o.作用ID
                    and not 直前.目的進展 and not 軟予算保留
                    and (set(直前.状態差.追加状態) & set(o.出力状態))
                    and (set(o.出力状態) & set(現在.要求状態))
                    and sum(h.作用ID == o.作用ID and not h.目的進展
                            for h in 履歴) == 1
                )
                if not 段階作用継続:
                    統計["無進展再試行除外数"] += 1
                    continue
            if 軟予算保留 and not 探索可能:
                continue
            関連選択.append(o)
        選択可能 = 関連選択
        統計["制約除外数"] += len(制約除外)
        if len(生成署名) > 政策.最大内部生成:
            return 終了(HDS終端.保留, 停止理由.予算枯渇, ("HDS_INTERNAL_CONSTRUCTION_BUDGET_EXHAUSTED",))

        計画 = None
        形成由来 = None
        計画仕様別 = {}
        可用ID = {o.作用ID for o in 選択可能}
        残探索 = 政策.最大探索状態 - 統計["探索状態数"]
        計画開始 = perf_counter_ns()
        if 関連仕様 and 残探索 > 0:
            計画用 = []
            for s in 関連仕様:
                if s.探索 is not None:
                    continue
                if 政策.許可判定(s.作用ID, s.必要権限):
                    continue
                o = 全機会.get(s.作用ID)
                if o is None:
                    continue
                if (o.作用ID, o.作用入力署名) in 使用済み:
                    continue
                if 政策.許可判定(o.作用ID, o.必要権限):
                    continue
                反映仕様 = _期待を計画仕様へ反映(s, o)
                計画用.append(反映仕様)
                計画仕様別[反映仕様.作用ID] = 反映仕様
            可用ノード = frozenset(n for s in 計画用 for n in s.入力ノード集合 if 目的ノード有効(現在, n))
            文脈 = 自動形成文脈(現在, 利用作用群)
            for 関係 in 現在.形成関係:
                計画 = 形成手順を再利用(関係, 現在.成立状態, 現在.残差, 現在.要求状態 | 修復状態,
                                     tuple(計画用), 文脈, 政策.最大資源 - 統計["消費資源"], 可用ノード=可用ノード)
                if 計画 is not None and any(x.制約違反 for x in 未来列を構成(現在.成立状態, 現在.残差, 計画.作用列, tuple(計画用), 主体.未来制約)):
                    計画 = None
                if 計画 is not None:
                    形成由来 = 関係
                    統計["形成再利用数"] += 1
                    break
            if 計画 is None:
                形成由来 = None
                計画 = 作用列を構成(現在.成立状態, 現在.残差, 現在.要求状態 | 修復状態,
                                  tuple(計画用), 最大深さ=政策.探索深さ, 最大状態数=残探索,
                                  最大資源=政策.最大資源 - 統計["消費資源"], 制約群=主体.未来制約,
                                  可用ノード=可用ノード,
                                  要求ノード=frozenset("認識:" + k for k in 現在.要求認識) | 指示到達ノード(現在))
            統計["探索状態数"] += 計画.探索状態数
            if 計画.成立:
                鍵 = 署名((現在.状態署名, 計画.作用列))
                if 鍵 not in 計画署名:
                    計画署名.add(鍵)
                    統計["動的計画数"] += 1
        統計["計画時間ns"] += perf_counter_ns() - 計画開始
        try:
            if 計画 is not None and 計画.成立 and 計画.作用列[0] in 可用ID:
                計画選択 = next(o for o in 選択可能 if o.作用ID == 計画.作用列[0])
                選択 = 計画選択
                # 動的計画は既定経路として保持する。独立経験から成立した学習証拠が
                # 計画先頭より明確に強い場合だけ、許可済み候補内で学習選択へ補正する。
                # 学習0点・同点では従来計画を一切変更しない。
                if hasattr(主体.適応記憶, "作用経路評価"):
                    計画学習得点, _ = 主体.適応記憶.作用経路評価(現在, 計画選択)
                    選択用状態 = deepcopy(現在)
                    選択用署名 = 選択用状態.状態署名
                    学習選択 = 主体.作用選択器.選択(
                        選択用状態, tuple(選択可能), tuple(履歴)
                    )
                    if 選択用状態.状態署名 != 選択用署名:
                        raise ValueError("作用選択器が主体状態を変更した")
                    if 学習選択 is not None:
                        学習得点, _ = 主体.適応記憶.作用経路評価(現在, 学習選択)
                        if 学習得点 > 計画学習得点:
                            選択 = 学習選択
                            統計["学習計画補正数"] += 1
            else:
                選択用状態 = deepcopy(現在)
                選択用署名 = 選択用状態.状態署名
                選択 = 主体.作用選択器.選択(選択用状態, tuple(選択可能), tuple(履歴))
                if 選択用状態.状態署名 != 選択用署名:
                    raise ValueError("作用選択器が主体状態を変更した")
            if 選択 is not None and 選択 not in 選択可能:
                raise ValueError("作用選択器が許可済み候補外を選択した")
        except Exception as exc:
            return 契約失敗("作用選択", exc)
        if 選択 is None:
            if 軟予算保留:
                return 終了(HDS終端.保留, 停止理由.無進展, ("HDS_NO_GOAL_PROGRESS_FOR_EFFORT_INCREASE",))
            if 資源除外:
                種別 = 停止理由.予算枯渇
            elif 制約除外:
                種別 = 制約除外[0][1]
            elif any(x.区分 == 認識区分.競合 for x in 現在.認識 if x.ID in 必要認識):
                種別 = 停止理由.証拠競合
            elif 観測待ち:
                種別 = 停止理由.観測不足
            elif any(d.区分 in ("未検証", "失効") and any(現在.ノード署名(k) != sig for k, sig in d.依存署名) for d in 現在.草案):
                種別 = 停止理由.依存未閉包
            elif 依存除外 or 現在.再評価待ち or 必要認識:
                種別 = 停止理由.依存未閉包
            elif 失敗入力:
                種別 = list(失敗入力.values())[-1].種別
            elif 計画 is not None and 計画.打切り:
                種別 = 停止理由.予算枯渇
            else:
                種別 = 停止理由.作用不足
            return 終了(HDS終端.保留, 種別, ("HDS_NO_PRODUCTIVE_ACTION",))

        選択仕様 = 機会仕様別.get(選択.作用ID)
        選択探索 = 選択仕様.探索 if 選択仕様 is not None else None
        if 軟予算保留:
            if 選択探索 is None or not 探索残枠(選択探索, 選択.資源負荷):
                return 終了(HDS終端.保留, 停止理由.無進展, ("HDS_EXPLORATION_ALLOWANCE_EXHAUSTED",))
            軟予算 = min(主体.最大作用回数, len(履歴) + 1)
            統計["探索予算拡張数"] += 1
        if 失敗入力 and (選択.作用ID not in 失敗入力 or 選択.入力状態 or 選択.読取認識):
            統計["修復選択数"] += 1
        作用開始 = perf_counter_ns()
        try:
            結果 = 作用を変換(ID別[選択.作用ID], 選択, 現在)
        finally:
            統計["作用時間ns"] += perf_counter_ns() - 作用開始
        try:
            planned = 計画.作用列 if 計画 is not None and 計画.成立 and 選択.作用ID == 計画.作用列[0] else ()
            if 形成由来 is not None and planned and 結果.状態 != HDS作用状態.成立:
                反例署名 = 署名((
                    "形成手順実行反例-v1", 形成由来.ID, 形成由来.版,
                    選択.作用ID, 選択.意味入力署名, 選択.契約版,
                    結果.状態, 結果.理由, 結果.阻害,
                ))
                隔離形成 = 形成由来.反例追加(反例署名)
                形成更新 = tuple(x for x in 結果.形成更新 if x.ID != 隔離形成.ID) + (隔離形成,)
                結果 = replace(結果, 形成更新=形成更新)
            specs = tuple(計画仕様別.values())
            未来 = 未来列を構成(現在.成立状態, 現在.残差, planned, specs, 主体.未来制約) if planned else ()
            記録する(選択, 結果, planned, 未来, 計画仕様別.get(選択.作用ID) if planned else None, 選択探索)
        except Exception as exc:
            return 契約失敗(選択.作用ID, exc)
        if 結果.停止要求:
            種別 = 結果.阻害.種別 if 結果.阻害 else 停止理由.明示停止
            return 終了(HDS終端.失敗 if 結果.状態 == HDS作用状態.失敗 else HDS終端.保留, 種別,
                      tuple(dict.fromkeys(("HDS_ACTION_REQUESTED_STOP", *結果.理由))))
