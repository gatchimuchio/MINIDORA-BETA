"""合成計画の各工程を、同じHDS通常循環の作用候補として供給する。"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import asdict
import math
from ..HDS実行主体 import HDS関数作用, HDS作用結果, HDS作用状態
from ..統合駆動_v2.政策 import HDS阻害, 停止理由
from ..能力合成 import _結果辞書, _参照結合
from ..製品版.能力契約 import 能力文脈
from ..製品版.型 import 能力結果
from ..採否 import 実行状態
from ..会話回答 import 回答を構成, 回答記録整合
from ..監査改善接続 import 改善回答を検査
from ..実行回復 import 回復方針を決定, 失敗を分類
from ..コア.内容計画 import 内容計画を構成, 内容計画を検査, 内容計画を表現
from .値 import 指紋, 正準, 結果を保存, 結果を復元, 計画を復元, 運用版
from .解釈 import 計画を構成, 構造を保存
from .数量生成 import 数量回答を検査
from .内容構成 import 資料文章を検査
from .関係内容 import 関係回答を検査

計画接頭辞 = "運用計画:"


def _会話束HDS対応を検査(解釈, 包, 所有セッション, *, 前採用包=None, 文脈証明=(),
                 前命令包=None, 命令証明=()):
    """保存した実Compiler全文と局所包を照合。実採用所有は呼出元責任。"""
    from ..HDS再生 import HDSIR復元
    from ..HDS会話契約 import 会話契約を検査
    if (type(解釈) is not dict or 解釈.get("方式") != "一般会話目的束"
            or type(解釈.get("原文")) is not str or 指紋(解釈.get("要求")) != 指紋(包)
            or type(解釈.get("HDS")) is not dict
            or type(解釈["HDS"].get("全文")) is not dict):
        raise ValueError("会話束の原解釈・契約・HDS全文対応が不一致です")
    if (指紋(解釈.get("前採用包")) != 指紋(前採用包)
            or 指紋(解釈.get("文脈証明", [])) != 指紋(list(文脈証明))
            or 指紋(解釈.get("前命令包")) != 指紋(前命令包)
            or 指紋(解釈.get("命令証明", [])) != 指紋(list(命令証明))):
        raise ValueError("会話束の保存文脈と独立に供給した採用文脈が不一致です")
    保存全文 = 解釈["HDS"]["全文"]
    if 保存全文.get("手順") is not None:
        raise ValueError("会話束の手順付きHDS全文の再構成は未接続です")
    再 = HDSIR復元(保存全文)
    # 再生器の省略時既定・str/bool coercionを、この保存契約へ持ち込まない。
    if 指紋(構造を保存(再)) != 指紋(保存全文) or 再.原文 != 解釈["原文"]:
        raise ValueError("会話束のHDS全文が保存時の型・欄・原文と完全同値ではありません")
    会話契約を検査(包, 所有セッション=所有セッション,
               期待原文=解釈["原文"], 意味IR=再,
               文脈証明=文脈証明, 命令証明=命令証明)
    return 再


def 現行計画(状態):
    records = dict(状態.成果)
    names = [k for k in records if k.startswith(計画接頭辞) and "/" not in k]
    if not names:
        return None
    key = max(names, key=lambda k: int(k.split(":")[-1]))
    return key, records[key]


def _工程鍵(plan_key, sid):
    return plan_key + "/成果/" + sid


def _済(plan_key, sid):
    return plan_key + "/成立/" + sid


def _試行鍵(plan_key, sid, index):
    return plan_key + "/試行/" + sid + "/" + str(index)


def 最終素材を構成(results, packet):
    if len(results) == 1:
        item = results[0]
        if packet["回答種別"] != "能力成果" or 回答記録整合(item) or 改善回答を検査(item.データ):
            return item
    return 回答を構成(results)


def 入力依存を収集(packet, inputs, 結果参照=()):
    """出力の祖先工程が読む資料を追跡する。能力が参照を返し忘れても消さない。"""
    plan = 計画を復元(packet["計画"])
    steps = {step.識別子: step for step in plan.工程}
    seen, needed, queue = set(), set(), list(plan.出力工程)
    while queue:
        sid = queue.pop()
        if sid in seen:
            continue
        seen.add(sid)
        step = steps[sid]
        needed.add(step.指示参照)
        if step.設定参照:
            needed.add(step.設定参照)
        for ref in step.入力:
            if ref.領域 == "工程":
                queue.append(ref.識別子)
            else:
                needed.add(ref.識別子)
    refs = {ref.識別子 for key in needed
            for ref in 結果を復元(packet["資料"][key]).参照}
    refs.update(参照.識別子 for 参照 in 結果参照)
    dependencies = {name: 元資料["版"] for name, 元資料 in inputs["資料"].items()
                    if any(ref.識別子 in refs for ref in 結果を復元(元資料["結果"]).参照)}
    interpretation = packet.get("解釈", {})
    if interpretation.get("方式") == "継続":
        interpretation = interpretation["目的"]
    if (interpretation.get("方式") in ("数量再表現", "一般再表現", "関係再表現") or (interpretation.get("方式") == "会話"
            and interpretation.get("依頼", {}).get("行為") == "再表現")):
        dependencies.update(inputs.get("前回依存", {}))
    return dependencies


def _部分木(coverage, root):
    rows = {row["目的鍵"]: row for row in coverage}
    seen, queue = set(), [root]
    while queue:
        key = queue.pop()
        if key in seen:
            continue
        if key not in rows:
            raise ValueError("再計画の被覆が閉じていない")
        seen.add(key)
        row = rows[key]
        if row["解決"] == "作用":
            queue.extend(pair[1] for pair in row["入力役割"])
    return rows, seen


def _局所変更を検査(old, new, policy):
    old_rows, old_tree = _部分木(old["被覆"], policy.対象目的)
    new_rows, new_tree = _部分木(new["被覆"], policy.対象目的)
    if {k: v for k, v in old_rows.items() if k not in old_tree} != {k: v for k, v in new_rows.items() if k not in new_tree}:
        raise ValueError("回復対象外の目的・役割が変化した")
    if old["役割"]["目的"] != new["役割"]["目的"] or old["目録"] != new["目録"]:
        raise ValueError("回復中の目的又は作用契約の変更")


class 工程供給:
    def __init__(self, 目録, *, 最大回復=4, 最大結果バイト=2_000_000, 停止要求=None, 取得制御=None):
        self.目録 = 目録
        self.最大回復 = 最大回復
        self.最大結果バイト = 最大結果バイト
        if 停止要求 is not None and not callable(停止要求):
            raise TypeError("工程の停止要求はcallable")
        self.停止要求 = 停止要求
        from ..取得実行制御 import 共有取得制御
        if 取得制御 is not None and not isinstance(取得制御, 共有取得制御):
            raise TypeError("工程の取得制御の型不正")
        self.取得制御 = 取得制御

    def _文脈(self, 状態, plan_key, packet, step):
        values = dict(状態.成果)
        materials = {k: 結果を復元(v) for k, v in packet["資料"].items()}
        sources = []
        for ref in step.入力:
            value = materials[ref.識別子] if ref.領域 == "入力" else 結果を復元(values[_工程鍵(plan_key, ref.識別子)])
            if not value.成立:
                raise ValueError("不成立の上流結果")
            sources.append((ref, value))
        references = _参照結合(r for _, value in sources for r in value.参照)
        auxiliary = {"合成入力": tuple({"参照": asdict(ref), "結果": _結果辞書(value)} for ref, value in sources),
                     "合成設定": deepcopy(materials[step.設定参照].データ) if step.設定参照 else {}}
        return 能力文脈(materials[step.指示参照].本文, values["運用入力"]["セッションID"],
                        "\n\n".join(value.本文 for _, value in sources), references, (), auxiliary,
                        停止要求=self.停止要求, 取得制御=self.取得制御)

    def __call__(self, 状態):
        self.目録.照合()
        current = 現行計画(状態)
        if current is None:
            return ()
        plan_key, packet = current
        if packet["方式"] == "管理":
            return ()
        if packet["目録"] != self.目録.ハッシュ:
            raise ValueError("計画と能力目録の版不一致")
        plan = 計画を復元(packet["計画"])
        values = dict(状態.成果)
        actions = []
        for step in plan.工程:
            inputs = ("運用:計画構成済", *(_済(plan_key, ref.識別子) for ref in step.入力 if ref.領域 == "工程"))
            reads = (plan_key, *(_工程鍵(plan_key, ref.識別子) for ref in step.入力 if ref.領域 == "工程"))
            for index, name in enumerate(step.能力候補):
                record_key = _試行鍵(plan_key, step.識別子, index)
                作用識別子 = "運用能力/" + str(plan_key) + "/" + step.識別子 + "/" + name
                registration = self.目録.取得(name)
                境界 = self.目録.境界契約(name)
                def eligible(s, key=record_key, st=step, number=index):
                    current_values = dict(s.成果)
                    return (key not in current_values and _済(plan_key, st.識別子) not in s.成立状態
                            and (number == 0 or _試行鍵(plan_key, st.識別子, number - 1) in current_values))
                def execute(s, st=step, ability=name, number=index, key=record_key, 境界=境界):
                    self.目録.照合()
                    ctx = self._文脈(s, plan_key, packet, st)
                    モジュール = self.目録.取得(ability).モジュール
                    before = 指紋(_結果辞書(能力結果(True, "", データ=正準({"文脈": {
                        "入力": ctx.入力文, "補助": ctx.補助, "参照": [r.辞書化() for r in ctx.直前参照]}}))))
                    try:
                        score = モジュール.判定(deepcopy(ctx))
                        if type(score) not in (int, float) or not math.isfinite(score) or not 0 <= score <= 1:
                            raise ValueError("能力の適用判定は有限な0..1の数値が必要")
                        cache_key = self.目録.再利用鍵(ability, ctx)
                        cache = self.目録.再利用庫
                        cache.計数(ability, "要求")
                        結果 = cache.取得(cache_key) if cache_key is not None and score > 0 else None
                        reused = 結果 is not None
                        if reused:
                            cache.計数(ability, "再利用")
                        elif score > 0:
                            cache.計数(ability, "原実行")
                            結果 = モジュール.実行(deepcopy(ctx))
                        else:
                            結果 = 能力結果(False, "", 保留理由="能力の適用条件を満たさない", 採否状態=実行状態.非適用)
                        self.目録.照合()
                        packed = 結果を保存(結果)
                        from ..能力合成 import _符号化
                        if len(_符号化(packed)) > self.最大結果バイト:
                            raise ValueError("能力結果の保存上限。切断せず停止")
                        if (結果.成立 and 結果.本文 and 境界 is not None
                                and "C8" in 境界.関連コア責任ID):
                            # 局所部品が既に生成した明示本文だけを共通内容契約へ載せる。
                            # 構造化中間結果に表面文章を強制せず、語彙・文法・専門説明方式もコアへ移さない。
                            plan = 内容計画を構成(
                                結果.本文, 種別=ability, 根拠=tuple(結果.根拠),
                                由来=tuple("参照:" + x.識別子 for x in 結果.参照),
                            )
                            if not 内容計画を検査(plan) or 内容計画を表現(plan) != 結果.本文:
                                raise ValueError("能力結果の内容計画境界が不整合")
                        record = {"能力": ability, "版": モジュール.版, "工程": st.識別子,
                                  "入力印": before, "出力印": 指紋(packed), "採否": 結果.状態.value,
                                  "再利用": reused, "理由": 結果.保留理由}
                        if 結果.成立:
                            if cache_key is not None and not reused:
                                cache.保存(cache_key, 結果)
                            return HDS作用結果(HDS作用状態.成立,
                                追加状態=frozenset({_済(plan_key, st.識別子)}),
                                成果=((_工程鍵(plan_key, st.識別子), packed), (key, record)),
                                理由=("能力成果をHDSへ帰還", ability, "純粋結果再利用" if reused else "能力実行"))
                        # 同じ有限型・容量検査を通過した不成立成果も原値のまま保持する。
                        # 工程成立・残差解消・採用結果には供給しない。
                        record["不成立成果"] = packed
                        return HDS作用結果(HDS作用状態.保留, 成果=((key, record),), 理由=(結果.保留理由,))
                    except Exception as exc:
                        # 契約違反を不適用や成功に縮退させない。
                        return HDS作用結果(HDS作用状態.失敗, 停止要求=True,
                            成果=((key, {"能力": ability, "工程": st.識別子, "理由": str(exc), "例外": type(exc).__name__}),),
                            理由=("能力実行契約違反", type(exc).__name__, str(exc)),
                            阻害=HDS阻害(停止理由.契約違反, ability, str(exc) or type(exc).__name__))
                def 意味入力署名関数(s, st=step, ability=name):
                    return self.目録.意味入力署名(ability, self._文脈(s, plan_key, packet, st))
                actions.append(HDS関数作用(作用識別子, execute, 入力状態=inputs,
                    出力状態=(_済(plan_key, step.識別子),), 読取成果=tuple(dict.fromkeys(reads)),
                    機会判定=eligible, 必要権限=("外部読取",) if registration.外部読取 else (),
                    優先度=1.0 - index / 1000, 契約版=registration.モジュール.版,
                    作用定義ID="運用能力/" + name, 意味入力署名=意味入力署名関数))
        recovery = self._回復作用(状態, plan_key, packet, plan)
        if recovery is not None:
            actions.append(recovery)
        outputs = tuple(_工程鍵(plan_key, sid) for sid in plan.出力工程)
        def answer(s):
            データ = dict(s.成果)
            results = tuple(結果を復元(データ[key]) for key in outputs)
            final = 最終素材を構成(results, packet)
            対応証拠 = {"原文": データ["運用入力"]["原文"], "計画鍵": plan_key, "計画印": 指紋(packet),
                        "目録": self.目録.ハッシュ, "出力": list(outputs),
                        "出力印": [指紋(データ[key]) for key in outputs], "回答印": 指紋(結果を保存(final)),
                        "資料依存": 入力依存を収集(packet, データ["運用入力"], final.参照)}
            return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"運用:応答成立"}),
                               解消残差=frozenset({"運用:成果未構成"}),
                               成果=(("運用応答", 結果を保存(final)), ("運用採用対応", 対応証拠)),
                               理由=("要求に対応する全出力から応答構成",))
        actions.append(HDS関数作用("運用/成果構成/" + plan_key, answer,
            入力状態=tuple(_済(plan_key, sid) for sid in plan.出力工程), 出力状態=("運用:応答成立",),
            解消対象=("運用:成果未構成",), 読取成果=("運用入力", plan_key, *outputs), 契約版=運用版,
            作用定義ID="運用/成果構成"))
        return tuple(actions)

    def _回復作用(self, 状態, plan_key, packet, plan):
        if int(plan_key.split(":")[-1]) >= self.最大回復:
            return None
        データ = dict(状態.成果)
        from .資料補充 import 資料補充を構成, 資料補充を検査, 資料補充版
        補充 = 資料補充を構成(plan_key, packet, データ, self.目録)
        if 補充 is not None:
            next_key = 計画接頭辞 + str(int(plan_key.split(":")[-1]) + 1)
            def supplement(s):
                values = dict(s.成果)
                資料補充を検査(plan_key, packet, values, self.目録, self.最大回復)
                再 = 資料補充を構成(plan_key, packet, values, self.目録)
                if 再 is None or 指紋(再) != 指紋(補充):
                    raise ValueError("資料補充の原要求・不足・取得元が変化しました")
                成果, 成立 = [(next_key, 再)], set()
                for st in 計画を復元(再["計画"]).工程:
                    if st.識別子 in 再["資料補充"]["再開工程"]:
                        continue
                    復帰元 = 再["資料補充"].get("成果復帰", {}).get(st.識別子)
                    if 復帰元 is not None:
                        if not 結果を復元(values[復帰元]).成立:
                            raise ValueError("取得成果の復帰元は実成功に限定します")
                        成果.append((_工程鍵(next_key, st.識別子), deepcopy(values[復帰元])))
                        成立.add(_済(next_key, st.識別子))
                        元接頭辞 = 復帰元.split("/成果/", 1)[0]
                        for i in range(len(st.能力候補)):
                            元試行 = _試行鍵(元接頭辞, st.識別子, i)
                            if 元試行 in values:
                                成果.append((_試行鍵(next_key, st.識別子, i), deepcopy(values[元試行])))
                        continue
                    if _済(plan_key, st.識別子) in s.成立状態:
                        成果.append((_工程鍵(next_key, st.識別子), deepcopy(values[_工程鍵(plan_key, st.識別子)])))
                        成立.add(_済(next_key, st.識別子))
                    for i in range(len(st.能力候補)):
                        旧 = _試行鍵(plan_key, st.識別子, i)
                        if 旧 in values:
                            成果.append((_試行鍵(next_key, st.識別子, i), deepcopy(values[旧])))
                return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset(成立), 成果=tuple(成果),
                                    理由=(("旧候補の実不成立と原不足を保持して公開資料を追加検索"
                                          if "取得失敗後再検索" in 再["資料補充"] else
                                          "同じ原要求の内容不足から公開資料を追加検索"
                                          if "再検索工程" in 再["資料補充"] else
                                          "同じ原要求の内容不足から未消費本文の取得を再開"),))
            根拠 = 補充["資料補充"]
            # 補充の再導出・旧チェーン検査・対象外コピーが読む原値を署名へ載せる。
            復帰 = 根拠.get("取得失敗後再検索")
            読取 = {"運用入力", plan_key, 根拠["不足試行鍵"],
                    復帰["意味成果鍵"] if 復帰 else _工程鍵(plan_key, 根拠["意味工程"]),
                    根拠["成果復帰"][根拠["取得工程"]] if 復帰 else _工程鍵(plan_key, 根拠["取得工程"])}
            for 名 in データ:
                if 名.startswith(計画接頭辞) and int(名.split("/", 1)[0].split(":")[-1]) <= int(plan_key.split(":")[-1]):
                    読取.add(名)
            return HDS関数作用("運用/資料補充/" + plan_key, supplement,
                入力状態=("運用:計画構成済",), 読取成果=tuple(sorted(読取)),
                目的依存=("残差:運用:成果未構成",), 優先度=5, 契約版=資料補充版,
                作用定義ID="運用/資料補充")
        if not packet["役割"]:
            return None
        info = {sid: (goal, 行為) for sid, goal, 行為 in packet["役割"]["工程作用"]}
        rules = {r.識別子: r for r in self.目録.役割作用}
        for step in plan.工程:
            failure_key = _試行鍵(plan_key, step.識別子, len(step.能力候補) - 1)
            if _済(plan_key, step.識別子) in 状態.成立状態 or failure_key not in データ or step.識別子 not in info:
                continue
            failure = データ[failure_key]
            pieces = failure.get("理由", "").split(":", 2)
            category = pieces[1] if len(pieces) == 3 and pieces[0] == "会話失敗" else "能力不成立"
            goal, 行為 = info[step.識別子]
            rule = rules[行為]
            matched = next((r for r in rule.回復 if r.失敗種別 == category), None)
            if matched is None or matched.動作() == "同一作用再試行":
                continue
            reopen = (goal, 行為) if matched.対象 == "自己" else None
            if matched.対象 == "入力役割":
                bindings = dict(dict(packet["役割"]["入力役割"]).get(step.識別子, ()))
                parent = bindings.get(matched.入力役割)
                if parent and parent["領域"] == "工程" and parent["識別子"] in info:
                    pair = info[parent["識別子"]]
                    if pair[1] in matched.対象作用:
                        reopen = pair
            policy = 回復方針を決定(種別=category, 分類=失敗を分類(category), 発生目的=goal, 発生作用=行為,
                    規則=matched, 再開放=reopen, 契約=指紋(asdict(matched)))
            if not policy.自動実行:
                continue
            next_key = 計画接頭辞 + str(int(plan_key.split(":")[-1]) + 1)
            def replan(s, policy=policy, next_key=next_key):
                values = dict(s.成果)
                banned = tuple(tuple(x) for x in packet["禁止"]) + policy.禁止
                if len(set(banned)) != len(banned):
                    raise ValueError("同じ回復を反復しない")
                候補 = 計画を構成(packet["解釈"], values["運用入力"], self.目録, 禁止=banned)
                _局所変更を検査(packet, 候補, policy)
                return HDS作用結果(HDS作用状態.成立, 成果=((next_key, 候補),
                            (next_key + "/回復根拠", 正準(asdict(policy)))),
                            理由=("目的と資料を保持した局所再計画", policy.動作))
            return HDS関数作用("運用/回復/" + plan_key, replan, 入力状態=("運用:計画構成済",),
                    読取成果=(plan_key, failure_key, "運用入力"), 目的依存=("残差:運用:成果未構成",),
                    優先度=5, 契約版=運用版, 作用定義ID="運用/回復")
        return None

    def 最終検証(self, 状態, _):
        """原依頼・計画・実出力を照合する。任意の世界事実の正しさの証明ではない。"""
        try:
            self.目録.照合()
            values = dict(状態.成果)
            answer = 結果を復元(values["運用応答"])
            proof = values["運用採用対応"]
            if proof["原文"] != values["運用入力"]["原文"] or proof["回答印"] != 指紋(values["運用応答"]):
                return False
            current_key, packet = 現行計画(状態)
            if ("資料補充" in packet or
                    packet.get("回答種別") == "資料文章回答"):
                from .資料補充 import 資料補充を検査
                資料補充を検査(current_key, packet, values, self.目録, self.最大回復)
            if proof["計画鍵"] != current_key or proof["計画印"] != 指紋(packet) or proof["目録"] != self.目録.ハッシュ:
                return False
            if packet["方式"] == "管理":
                return answer.成立 and proof["出力"] == []
            if proof["資料依存"] != 入力依存を収集(packet, values["運用入力"], answer.参照):
                return False
            plan = 計画を復元(packet["計画"])
            expected = [_工程鍵(current_key, sid) for sid in plan.出力工程]
            if proof["出力"] != expected or proof["出力印"] != [指紋(values[k]) for k in expected]:
                return False
            if any(not 結果を復元(values[k]).成立 for k in expected):
                return False
            expected_results = tuple(結果を復元(values[k]) for k in expected)
            expected_answer = 最終素材を構成(expected_results, packet)
            if 指紋(結果を保存(expected_answer)) != 指紋(values["運用応答"]):
                return False
            if packet["回答種別"] == "会話目的束回答":
                return self._会話目的束を検証(answer, packet, values, current_key)
            if packet["回答種別"] == "数量回答":
                if not 数量回答を検査(answer):
                    return False
                原要求 = packet["解釈"]
                if 原要求["方式"] == "継続":
                    原要求 = 原要求["目的"]
                構造 = answer.データ["構造"]
                if 原要求["方式"] == "数量言語":
                    if 構造["要求"] != 原要求["要求"] or answer.データ["表示"] != 原要求["要求"]["表示"]:
                        return False
                elif 原要求["方式"] == "数量再表現":
                    前 = 結果を復元(values["運用入力"]["前回結果"])
                    表示 = {**前.データ["表示"], **原要求["表示差分"]}
                    if 構造 != 前.データ["構造"] or answer.データ["表示"] != 表示:
                        return False
                else:
                    return False
                return all(name in values["運用入力"]["資料"] and
                           text == values["運用入力"]["資料"][name]["本文"]
                           for name, text in 構造["資料"].items())
            if packet["回答種別"] == "関係資料回答":
                if not 関係回答を検査(answer):
                    return False
                原要求 = packet["解釈"]
                if 原要求["方式"] == "継続":
                    原要求 = 原要求["目的"]
                構造 = answer.データ["構造"]
                if 原要求["方式"] == "関係資料":
                    if 構造["要求"] != 原要求["要求"] or answer.データ["表示形式"] != 原要求["要求"]["形式"]:
                        return False
                elif 原要求["方式"] == "関係再表現":
                    前回 = 結果を復元(values["運用入力"]["前回結果"])
                    if 構造 != 前回.データ["構造"] or answer.データ["表示形式"] != 原要求["形式"]:
                        return False
                elif 原要求["方式"] == "関係検討":
                    return self._主張検討を検証(answer, packet, values, current_key)
                else:
                    return False
                if 構造["要求"]["範囲"] == "全資料" and set(構造["資料群"]) != set(values["運用入力"]["資料"]):
                    return False
                return all(名 in values["運用入力"]["資料"] and 項["資料"] == values["運用入力"]["資料"][名]["結果"]
                           for 名, 項 in 構造["資料群"].items())
            if packet["回答種別"] == "資料文章回答":
                if not 資料文章を検査(answer):
                    return False
                原要求 = packet["解釈"]
                if 原要求["方式"] == "継続":
                    原要求 = 原要求["目的"]
                構造 = answer.データ["構造"]
                if 原要求["方式"] == "一般資料":
                    if 構造["要求"] != 原要求["要求"] or answer.データ["表示形式"] != 原要求["要求"]["形式"]:
                        return False
                elif 原要求["方式"] == "一般再表現":
                    前回 = 結果を復元(values["運用入力"]["前回結果"])
                    if 構造 != 前回.データ["構造"] or answer.データ["表示形式"] != 原要求["形式"]:
                        return False
                else:
                    return False
                if 構造["要求"]["版"] in ("HDS一般会話要求-v2", "HDS一般会話要求-v3"):
                    return self._会話根拠を検証(構造, packet, values, current_key)
                if 構造["要求"]["範囲"] == "公開取得":
                    return 構造["取得記録"] is not None
                if 構造["要求"]["範囲"] == "全資料" and set(構造["資料群"]) != set(values["運用入力"]["資料"]):
                    return False
                return all(名 in values["運用入力"]["資料"] and 項["資料"] == values["運用入力"]["資料"][名]["結果"]
                           for 名, 項 in 構造["資料群"].items())
            if packet["回答種別"] == "検討回答":
                return answer.成立 and 改善回答を検査(answer.データ)
            return answer.成立 and (回答記録整合(answer) or 改善回答を検査(answer.データ))
        except (ValueError, TypeError, KeyError):
            return False

    @staticmethod
    def _主張検討を検証(回答, 包, 値, 計画鍵):
        """原IR・当回材料と四実成果を結ぶ。旧方式の終端は変更しない。"""
        from .関係依頼 import 関係計画を作る, 主張回答を検査
        from .関係内容 import 関係内容を構成
        from .値 import 計画を保存
        try:
            解釈, 入力 = 包["解釈"], 値["運用入力"]
            if (包["原文"] != 入力["原文"] or 解釈["方式"] != "関係検討"
                    or 指紋(解釈) != 指紋(値["運用解釈"])
                    or not 主張回答を検査(回答, 解釈, 入力)):
                return False
            再計画, 再素材, 種別 = 関係計画を作る(解釈, 入力)
            if (種別 != 包["回答種別"] or 指紋(計画を保存(再計画)) != 指紋(包["計画"])
                    or 指紋({名: 結果を保存(項) for 名, 項 in 再素材.items()}) != 指紋(包["資料"])):
                return False
            実列 = [結果を復元(値[_工程鍵(計画鍵, 工程.識別子)]) for 工程 in 再計画.工程]
            if len(実列) != 4 or any(not 項.成立 for 項 in 実列):
                return False
            読解, 内容, 文章, 照合 = 実列
            構造 = 回答.データ["構造"]
            return (指紋(読解.データ) == 指紋(構造)
                    and 指紋(内容.データ) == 指紋(関係内容を構成(構造))
                    and 指紋(結果を保存(文章)) == 指紋(回答.データ["文章"])
                    and 指紋(結果を保存(照合)) == 指紋(結果を保存(回答))
                    and 読解.参照 == 内容.参照 == 回答.参照)
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError, OverflowError):
            return False

    def _会話目的束を検証(self, 回答, 包, 値, 計画鍵):
        """目的束を実Compiler・原計画・各子実出力・根拠へ束縛する。"""
        from .一般依頼 import 一般計画を作る
        from .会話回答束 import 会話束回答を検査
        from .会話文脈 import 会話入力文脈を読む
        try:
            入力 = 値["運用入力"]
            契約 = 回答.データ["契約包"]
            文脈引数 = {"所有セッション": 入力["セッションID"],
                      **会話入力文脈を読む(契約, 入力)}
            if not 会話束回答を検査(回答, **文脈引数):
                return False
            原解釈 = 包["解釈"]
            if 原解釈["方式"] == "継続":
                原解釈 = 原解釈["目的"]
            実解釈 = 値["運用解釈"]
            if 実解釈["方式"] == "継続":
                実解釈 = 実解釈["目的"]
            if (原解釈["原文"] != 入力["原文"] or 包["原文"] != 入力["原文"]
                    or 指紋(原解釈) != 指紋(実解釈)):
                return False
            _会話束HDS対応を検査(原解釈, 契約, **文脈引数)
            再計画, 再素材, 種別 = 一般計画を作る(原解釈, 入力)
            from .値 import 計画を保存
            再保存素材 = {名: 結果を保存(項) for 名, 項 in 再素材.items()}
            if (種別 != 包["回答種別"] or 指紋(計画を保存(再計画)) != 指紋(包["計画"])
                    or 指紋(再保存素材) != 指紋(包["資料"])):
                return False
            子計画群 = 再素材["一般:子計画"].データ["子計画"]
            保存子群 = 回答.データ["目的成果"]
            if not 1 <= len(子計画群) <= 4 or len(保存子群) != len(子計画群):
                return False
            if sum(項.能力候補 == ("知識取得",) for 項 in 再計画.工程) > 4:
                return False
            for 子計画, 保存子 in zip(子計画群, 保存子群):
                実保存 = 値[_工程鍵(計画鍵, 子計画["出力"])]
                if (子計画["目的ID"] != 保存子["目的ID"]
                        or 指紋(実保存) != 指紋(保存子["子成果"])):
                    return False
                実子 = 結果を復元(実保存)
                子包 = {"計画": 子計画["計画"], "解釈": 子計画["解釈"],
                       "外部許可": 包["外部許可"],
                       "資料": {名: 再保存素材[対応] for 名, 対応 in 子計画["素材対応"].items()}}
                子値 = 値
                元目的 = 子計画["解釈"].get("前束目的ID")
                命令目的 = 子計画["解釈"].get("前命令目的ID")
                if 元目的 is not None and 命令目的 is not None:
                    return False
                if 元目的 is not None:
                    前回 = 結果を復元(入力["前回結果"])
                    if (文脈引数["前採用包"] is None or not 入力["前回有効"]
                            or 前回.データ.get("種別") != "会話目的束回答"
                            or 指紋(前回.データ["契約包"]) != 指紋(文脈引数["前採用包"])):
                        return False
                    前子 = [行 for 行 in 前回.データ["目的成果"] if 行["目的ID"] == 元目的]
                    if len(前子) != 1:
                        return False
                    子値 = {**値, "運用入力": {**入力, "前回結果": 前子[0]["子成果"]}}
                if 命令目的 is not None:
                    if (文脈引数["前命令包"] is None
                            or 命令目的 not in 文脈引数["前命令包"]["目的参照"]):
                        return False
                    子値 = {**値, "運用入力": {**入力, "前回結果": None, "前回有効": False}}
                if (not 資料文章を検査(実子)
                        or 指紋(実子.データ["構造"]["要求"]) != 指紋(子計画["解釈"]["要求"])):
                    return False
                追加引数 = {"会話契約": 契約} if 命令目的 is not None else {}
                if not self._会話根拠を検証(実子.データ["構造"], 子包, 子値, 計画鍵, **追加引数):
                    return False
            return 1 <= len(回答.本文) <= 24000
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
            return False

    @staticmethod
    def _会話根拠を検証(構造, 包, 値, 計画鍵, *, 会話契約=None):
        """今回の実入力又は有効な採用原本に根拠を束縛する。登録資料へ偽装しない。"""
        from .一般接続 import 取得資料を復元
        入力, 要求 = 値["運用入力"], 構造["要求"]
        if 包["解釈"].get("方式") == "一般再表現":
            if not 入力["前回有効"] or 入力["前回結果"] is None:
                return False
            前回 = 結果を復元(入力["前回結果"])
            return 資料文章を検査(前回) and 構造 == 前回.データ["構造"]
        素材 = {名: 結果を復元(項) for 名, 項 in 包["資料"].items()}
        束縛 = 素材["一般:束縛"].データ
        if set(束縛) != {"元要求印", "実範囲", "資料名"} or 束縛["元要求印"] != 指紋(要求):
            return False
        名前 = 束縛["資料名"]
        if type(名前) is not list or len(名前) != len(set(名前)):
            return False
        if 束縛["実範囲"] == "公開取得":
            if not 入力["外部許可"] or not 包["外部許可"] or 名前:
                return False
            計画 = 計画を復元(包["計画"])
            取得工程 = [工程 for 工程 in 計画.工程 if 工程.能力候補 == ("知識取得",)]
            if not 1 <= len(取得工程) <= 4:
                return False
            原資料, 記録 = {}, []
            for 工程 in 取得工程:
                成果 = 結果を復元(値[_工程鍵(計画鍵, 工程.識別子)])
                for 名, 資料 in 取得資料を復元(成果).items():
                    if 名 in 原資料 and 原資料[名] != 資料:
                        return False
                    原資料[名] = 資料
                # 知識取得の実型tupleは保存時に復元される。選択能力が保持する
                # 交換形へ同じ正準化を施し、内容差と容器型差を区別する。
                記録.append(正準(成果.データ))
            return (構造["取得記録"] == 記録 and 構造["資料順"] == list(原資料)
                    and set(構造["資料群"]) == set(原資料)
                    and all(行["資料"] == 結果を保存(原資料[名]) for 名, 行 in 構造["資料群"].items()))
        if not 1 <= len(名前) <= 16 or 構造["資料順"] != 名前 or set(構造["資料群"]) != set(名前):
            return False
        原資料 = {名: 結果を保存(素材["一般:資料:" + str(番号)]) for 番号, 名 in enumerate(名前)}
        if any(構造["資料群"][名]["資料"] != 原資料[名] for 名 in 名前):
            return False
        if 束縛["実範囲"] == "提示本文":
            return 名前 == ["提示本文"] and 要求["提示本文"] == 素材["一般:資料:0"].本文 and 構造["取得記録"] is None
        if 束縛["実範囲"] == "命令提示原文":
            from .会話文脈 import 命令提示材料を読む
            元目的 = 包["解釈"].get("前命令目的ID")
            if (会話契約 is None or 元目的 is None or 名前 != ["提示本文"]
                    or 構造["取得記録"] is not None or 要求["提示本文"]
                    or 入力["前回結果"] is not None or 入力["前回有効"]):
                return False
            実材料 = 命令提示材料を読む(会話契約, 元目的, 入力)
            return 実材料 is not None and 原資料["提示本文"] == 結果を保存(実材料)
        if 束縛["実範囲"] == "前回実資料":
            if not 入力["前回有効"] or 入力["前回結果"] is None or not (包["解釈"].get("継承") or 包["解釈"].get("根拠継承")):
                return False
            前回 = 結果を復元(入力["前回結果"])
            if not 資料文章を検査(前回):
                return False
            旧資料 = 前回.データ["構造"]["資料群"]
            return (all(名 in 旧資料 and 原資料[名] == 旧資料[名]["資料"] for 名 in 名前)
                    and 構造["取得記録"] == 前回.データ["構造"]["取得記録"])
        if 束縛["実範囲"] not in ("指定資料", "全資料", "会話資料") or 構造["取得記録"] is not None:
            return False
        if 束縛["実範囲"] == "全資料" and set(名前) != set(入力["資料"]):
            return False
        return all(名 in 入力["資料"] and 原資料[名] == 入力["資料"][名]["結果"] for 名 in 名前)
