from __future__ import annotations

from dataclasses import replace
import re
from typing import Iterable

from .HDS構文化記録 import (
    HDS構文化器成果,
    HDS保持契約,
    HDS原理探索要求,
    HDS原理段階,
    HDS監査状態,
    HDS監査要求,
    HDS監査項目,
    HDS認知世界断片,
)
from .HDS中間表現 import HDSIR, HDS座標, HDS意味作用, 値状態


_文分割 = re.compile(r"(?<=[。！？!?;；])\s*|\n+")
_発話主体規則 = (
    re.compile(r"(?:私は|我々は|筆者は|著者は)"),
    re.compile(r"\b(?:I|we|the author|the authors)\b", re.I),
)
_時間規則 = (
    re.compile(r"(?:現在|現時点|今後|将来|過去|以前|以後|以降|当時|今日|明日|昨日|長期|短期|中期)"),
    re.compile(r"\b(?:currently|presently|now|future|past|today|tomorrow|yesterday|before|after|since|until|long[- ]term|short[- ]term)\b", re.I),
    re.compile(r"(?:19|20)\d{2}年(?:\d{1,2}月(?:\d{1,2}日)?)?"),
    re.compile(r"\b(?:19|20)\d{2}(?:-\d{1,2}(?:-\d{1,2})?)?\b"),
)
_空間規則 = (
    re.compile(r"(?:において|の範囲内|国内|国外|市場|組織内|制度上|情報空間|物理空間|地域|領域)"),
    re.compile(r"\b(?:within|in the|at the|market|organization|jurisdiction|region|domain|environment)\b", re.I),
)
_目的規則 = (
    re.compile(r"(?:ために|目的(?:は|として|で)|狙い(?:は|として)|必要性)"),
    re.compile(r"\b(?:in order to|for the purpose of|goal|objective|purpose)\b", re.I),
)
_機構規則 = (
    re.compile(r"(?:によって|を通じて|を介して|機構|メカニズム|経路|作用機序)"),
    re.compile(r"\b(?:through|via|by means of|mechanism|pathway|process)\b", re.I),
)
_一般作用規則 = (
    re.compile(r"(?P<s>[^。！？、]{1,80}?)(?:が|は)(?P<o>[^。！？、]{1,80}?)(?:を)?(?P<v>変える|変化させる|変更する|更新する|改善する|悪化させる)"),
    re.compile(r"(?P<s>[^?!.;,]{1,100}?)\s+(?P<v>changes?|transforms?|alters?|updates?|improves?|worsens?)\s+(?P<o>[^?!.;,]{1,100})", re.I),
)

_動態規則 = {
    "初期状態": (
        re.compile(r"(?:初期状態|開始時|当初|起点)"),
        re.compile(r"\b(?:initial state|initially|at the start|baseline state)\b", re.I),
    ),
    "遷移": (
        re.compile(r"(?:遷移|変化|変わる|変える|移行|推移|→)"),
        re.compile(r"\b(?:transition|changes?|transforms?|moves? to|becomes?)\b", re.I),
    ),
    "分岐": (
        re.compile(r"(?:分岐|場合|ならば|なら|条件に応じ)"),
        re.compile(r"\b(?:branch|if|unless|depending on|otherwise)\b", re.I),
    ),
    "更新": (
        re.compile(r"(?:更新|改訂|修正|再構成|置換|再定義)"),
        re.compile(r"\b(?:update|revise|revision|reconstruct|replace|redefine)\b", re.I),
    ),
    "停止": (
        re.compile(r"(?:停止|終了|中止|打ち切|SUSPEND|保留)"),
        re.compile(r"\b(?:stop|terminate|halt|suspend|hold)\b", re.I),
    ),
    "帰還": (
        re.compile(r"(?:帰還|フィードバック|戻す|再入力|次(?:回|状態)へ)"),
        re.compile(r"\b(?:feedback|return|feed back|next state|next iteration)\b", re.I),
    ),
}

_暗黙知規則 = {
    "定義": (
        re.compile(r"(?:とは|と定義(?:する|される)|を指す|という意味)"),
        re.compile(r"\b(?:defined as|means|refers to|by .{0,40} we mean)\b", re.I),
    ),
    "前提": (
        re.compile(r"(?:前提|仮定|当然視|と仮定する)"),
        re.compile(r"\b(?:assume|assuming|given that|suppose|premise)\b", re.I),
    ),
    "射程": (
        re.compile(r"(?:射程|適用範囲|範囲|に限る|のみ|条件下|対象外|非適用)"),
        re.compile(r"\b(?:scope|within|only|limited to|applies to|out of scope|under)\b", re.I),
    ),
    "不確実性": (
        re.compile(r"(?:可能性|かもしれ|推定|推測|仮説|予測|不確実|未確定|暫定)"),
        re.compile(r"\b(?:may|might|could|likely|unlikely|estimate|hypothesis|prediction|uncertain|provisional)\b", re.I),
    ),
}

_論証接続規則 = (
    re.compile(r"(?:したがって|ゆえに|よって|なぜなら|ので|従って)"),
    re.compile(r"\b(?:therefore|thus|hence|because|since|so that)\b", re.I),
)
_反論規則 = (
    re.compile(r"(?:しかし|一方で|反論|反例|ただし|とはいえ|にもかかわらず)"),
    re.compile(r"\b(?:however|but|counterargument|counterexample|on the other hand|nevertheless)\b", re.I),
)
_可能性規則 = (
    re.compile(r"(?:可能|不可能|あり得る|ありえない|可能性)"),
    re.compile(r"\b(?:possible|impossible|possibility|cannot|can't|may|might|could)\b", re.I),
)
_投影注意規則 = (
    re.compile(r"(?:世界|本質|真理|絶対|必ず|常に|すべて|全て)"),
    re.compile(r"\b(?:world|essence|truth|absolute|always|never|all|universal)\b", re.I),
)
_可逆性規則 = (
    re.compile(r"(?:不可逆|可逆|rollback|ロールバック|切り戻|撤回|実装|実行|作用)"),
    re.compile(r"\b(?:irreversible|reversible|rollback|undo|withdraw|implement|execute|act)\b", re.I),
)
_資源規則 = (
    re.compile(r"(?:資源|計算量|メモリ|時間制約|予算|コスト|トークン)"),
    re.compile(r"\b(?:resource|compute|memory|budget|cost|token|time limit)\b", re.I),
)
_自己適用規則 = (
    re.compile(r"(?:HDS|本フレーム|本原理|本稿自身|この原則自体)"),
    re.compile(r"\b(?:HDS|this framework|this principle|itself|self-application)\b", re.I),
)
_原理規則 = (
    (HDS原理段階.原理候補, re.compile(r'(?:原理候補|principle 候補)', re.I)),
    (HDS原理段階.原理候補, re.compile(r"(?:原理|法則|\bprinciple\b|\blaw\b)", re.I)),
    (HDS原理段階.機構候補, re.compile(r"(?:機構|メカニズム|\bmechanism\b)", re.I)),
    (HDS原理段階.パターン, re.compile(r"(?:パターン|反復構造|\bpattern\b)", re.I)),
)
_阻害状態 = {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}


def _unique(values: Iterable[str]) -> tuple[str, ...]:
    out: list[str] = []
    seen: set[str] = set()
    for raw in values:
        value = " ".join(str(raw).split()).strip()
        if not value:
            continue
        key = value.casefold()
        if key in seen:
            continue
        seen.add(key)
        out.append(value)
    return tuple(out)


def _文群(text: str) -> tuple[str, ...]:
    return tuple(part.strip() for part in _文分割.split(text) if part.strip())


def _該当文(text: str, patterns: Iterable[re.Pattern[str]]) -> tuple[str, ...]:
    return _unique(sentence for sentence in _文群(text) if any(pattern.search(sentence) for pattern in patterns))


def _該当表層(text: str, patterns: Iterable[re.Pattern[str]]) -> tuple[str, ...]:
    return _unique(match.group(0) for pattern in patterns for match in pattern.finditer(text))


def _一般作用端点(text: str) -> tuple[tuple[str, ...], tuple[str, ...]]:
    for pattern in _一般作用規則:
        match = pattern.search(text)
        if match:
            return _unique((match.group("s"),)), _unique((match.group("o"),))
    return (), ()


def _関係端点(ir: HDSIR) -> tuple[tuple[str, ...], tuple[str, ...]]:
    coords = ir.座標辞書()
    starts: list[str] = []
    ends: list[str] = []
    for 関係 in ir.関係:
        for coordinate_id in 関係.始点:
            coordinate = coords.get(coordinate_id)
            if coordinate is not None and coordinate.値状態 not in _阻害状態:
                starts.append(str(coordinate.内容))
        for coordinate_id in 関係.終点:
            coordinate = coords.get(coordinate_id)
            if coordinate is not None and coordinate.値状態 not in _阻害状態:
                ends.append(str(coordinate.内容))
    return _unique(starts), _unique(ends)


def _監査項目追加(items: list[HDS監査項目], *, layer: str, kind: str, contents: Iterable[str], 状態: HDS監査状態 = HDS監査状態.観測, required: tuple[str, ...] = (), reopen: tuple[str, ...] = ()) -> None:
    for content in _unique(contents):
        items.append(HDS監査項目(f"{layer}:{kind}:{len(items)}", layer, kind, content, 状態, 必要情報=required, 再開放条件=reopen))


def _監査要求追加(requirements: list[HDS監査要求], *, kind: str, reason: str, required: tuple[str, ...] = (), refs: tuple[str, ...] = (), probes: tuple[str, ...] = ()) -> None:
    key = (kind, reason, required, refs, probes)
    if any((r.種別, r.理由, r.必要情報, r.影響参照, r.次の観測候補) == key for r in requirements):
        return
    requirements.append(HDS監査要求(f"audit:{len(requirements):03d}", kind, reason, required, refs, probes))


def _抽出(ir: HDSIR) -> tuple[HDS認知世界断片, tuple[HDS監査項目, ...], tuple[HDS監査要求, ...], HDS原理探索要求, dict[str, tuple[str, ...]]]:
    text = " ".join(str(ir.正規化文 or ir.原文).split()).strip()
    starts, ends = _関係端点(ir)
    if not starts and not ends:
        starts, ends = _一般作用端点(text)
    speakers = _該当表層(text, _発話主体規則)
    times = _該当表層(text, _時間規則)
    spaces = _該当表層(text, _空間規則)
    purposes = _該当文(text, _目的規則)
    mechanisms = _該当文(text, _機構規則)

    missing = tuple(name for name, values in (("発話主体", speakers), ("作用主体", starts), ("対象", ends), ("時間", times), ("空間", spaces), ("目的", purposes), ("機構", mechanisms)) if not values)
    world = HDS認知世界断片(speakers, starts, ends, times, spaces, purposes, mechanisms, missing)

    items: list[HDS監査項目] = []
    for kind, values in (("発話主体", speakers), ("作用主体", starts), ("対象", ends), ("時間", times), ("空間", spaces), ("目的", purposes), ("機構", mechanisms)):
        _監査項目追加(items, layer="第0層", kind=kind, contents=values)
    for name in missing:
        _監査項目追加(items, layer="第0層", kind="座標未固定", contents=(name,), 状態=HDS監査状態.未固定, required=(name,), reopen=("追加入力またはRで座標を観測する",))

    意味: dict[str, tuple[str, ...]] = {}
    for kind, patterns in _動態規則.items():
        values = _該当文(text, patterns)
        意味[f"動態.{kind}"] = values
        _監査項目追加(items, layer="第1層", kind=kind, contents=values)
    for kind, patterns in _暗黙知規則.items():
        values = _該当文(text, patterns)
        意味[f"{kind}.明示"] = values
        _監査項目追加(items, layer="第2層", kind=kind, contents=values)

    argument_links = _該当文(text, _論証接続規則)
    counterarguments = _該当文(text, _反論規則)
    意味["論証.接続"] = argument_links
    意味["論証.反論"] = counterarguments
    _監査項目追加(items, layer="第3層", kind="推論接続", contents=argument_links)
    _監査項目追加(items, layer="第3層", kind="反論", contents=counterarguments)
    items.append(HDS監査項目(f"第4層:一貫性:{len(items)}", "第4層", "全層横断照合", "座標・動態・定義・前提・射程・不確実性・論証の変更履歴を横断照合する", HDS監査状態.要求, 必要情報=("各層の観測済み項目と未固定項目",), 再開放条件=("新観測・定義変更・前提変更・射程変更・時間経過",)))
    items.append(HDS監査項目(f"第5層:留保:{len(items)}", "第5層", "留保", '現行射影で扱えていない項目の存在を予約する', HDS監査状態.留保, 必要情報=("未知の未知を含む未採用項目",), 再開放条件=("反復失敗・新しい破綻類型・追加観測",)))

    principle_stage = HDS原理段階.未形成
    principle_markers: list[str] = []
    for stage, pattern in _原理規則:
        matches = tuple(match.group(0) for match in pattern.finditer(text))
        if matches:
            principle_markers.extend(matches)
            principle_stage = stage
            break
    if principle_stage == HDS原理段階.未形成 and ir.関係:
        principle_stage = HDS原理段階.影
    principle_questions = _unique(sentence for sentence in _文群(text) if re.search(r"(?:なぜ|どう|原理|機構|\bwhy\b|\bhow\b|\bprinciple\b|\bmechanism\b)", sentence, re.I))
    principle_needs = ("反対モデル", "成立条件", "不成立条件", "反証条件", "摂動または反実仮想", "適用範囲", "境界条件", "再開放条件") if principle_stage != HDS原理段階.未形成 else ()
    principle = HDS原理探索要求(principle_stage, _unique(principle_markers), principle_questions, principle_needs, 意味.get("射程.明示", ()), (), ("新観測・反例・境界変更で再監査する",) if principle_needs else ())

    requirements: list[HDS監査要求] = []
    if missing:
        _監査要求追加(requirements, kind="座標固定要求", reason="基底座標に未固定項目がある", required=missing, probes=tuple(f"{item}を観測または明示する" for item in missing))
    unresolved = tuple(残差.原文 for 残差 in ir.残差 if 残差.種別 in {"未解共参照", "未解関係両端"} or "未解" in 残差.種別)
    if unresolved:
        _監査要求追加(requirements, kind="未定義・未解参照要求", reason="未解参照または未閉包関係が残っている", required=("参照先", "意味境界"), refs=_unique(unresolved))
    if unresolved or any(coord.値状態 in _阻害状態 for coord in ir.座標):
        _監査要求追加(requirements, kind="閉包要求", reason="局所閉包に未観測または留保座標が残る", required=("対象X", "関係R", "同一性・境界・判定・停止条件"))
    if any(pattern.search(text) for pattern in _可能性規則):
        _監査要求追加(requirements, kind="不可能性要求", reason="可能・不可能の表現を主張強度へ直結させない", required=("成立条件", "条件の証拠", "矛盾", "不可能性証拠", "対称な否定候補"), probes=("成立条件を変えた観測", "反例または不成立条件"))
    if counterarguments:
        _監査要求追加(requirements, kind="反論対称性要求", reason="反論が存在するため元主張と反論へ同型監査を要求する", required=("元主張への適用結果", "反論自身への適用結果", "非対称根拠"))
        _監査要求追加(requirements, kind="反論強度要求", reason="反論の存在だけで元主張を棄却しない", required=("具体性", "証拠", "因果・構造密度", "内部整合", "射程一致", "反証可能性"))

    claim_like = bool(ir.関係 or argument_links or principle_stage != HDS原理段階.未形成)
    if claim_like:
        _監査要求追加(requirements, kind="証拠要求", reason="関係・論証・原理候補を外部事実へ接地する必要がある", required=("証拠型", "出典", "観測条件", "反証", "欠損"))
    if argument_links:
        _監査要求追加(requirements, kind="論証要求", reason="前提から結論への接続が明示されている", required=("推論形式", "前提妥当性", "定義・時系列・文脈整合"))
    if any(pattern.search(text) for pattern in _投影注意規則):
        _監査要求追加(requirements, kind="投影境界要求", reason='世界・本質・真理・全称表現を有限射影から直接確定しない', required=("表現限界", "適用範囲", "未表現残差", "別解釈"))
    if any(pattern.search(text) for pattern in _可逆性規則):
        _監査要求追加(requirements, kind="可逆性要求", reason="作用・実装・不可逆性に関わる表現がある", required=("rollback", "撤回条件", "不可逆範囲", "回復コスト"))
    dynamic_any = any(意味.get(f"動態.{kind}") for kind in _動態規則)
    if times or dynamic_any:
        _監査要求追加(requirements, kind="時間帰属要求", reason="時間または状態遷移が明示されている", required=("作用効果", "遅延効果", "時間経過", "外生変化", "観測誤差", "多因子", "未帰属"))
    if any(pattern.search(text) for pattern in _資源規則):
        _監査要求追加(requirements, kind="資源要求", reason="資源制約が意味保持・観測完全性へ影響し得る", required=("必要保持量", "観測完全性", "意味忠実度", "停止条件"))
    if principle_stage != HDS原理段階.未形成:
        _監査要求追加(requirements, kind="原理探索要求", reason="パターン・機構・原理に関わる構造が観測された", required=principle.必要監査, probes=("反対モデル", "反証", "摂動", "反実仮想", "追加観測"))

    _監査要求追加(requirements, kind="保持要求", reason="原理または将来の再解釈への寄与を事前に剪定しない", required=("座標", "関係", "不確実性", "残差", "由来", "旧解釈", "再開放条件"))
    _監査要求追加(requirements, kind="意味損失要求", reason="有限言語・射影・実装による不可避な損失を無損失と装わない", required=("表現限界", "翻訳近似", "未分別", "未知境界"))
    _監査要求追加(requirements, kind="暫定性要求", reason='構文化器出力を最終世界・最終原理・完全記述へ昇格させない', required=("時点", "射程", "版", "再開放条件"))
    if any(pattern.search(text) for pattern in _自己適用規則):
        _監査要求追加(requirements, kind="自己適用要求", reason="フレーム・原理・HDS自身が主張対象に含まれる", required=("自己例外の不存在", "改訂可能性", "旧版保持"))
    _監査要求追加(requirements, kind="最終採否委譲", reason='構文化器は構造・不足・監査要求を生成するが、真偽・原理・最終採否を決定しない', required=("HDS判断側または同等の採否境界へ委譲する",))
    return world, tuple(items), tuple(requirements), principle, 意味


def _IRへ射影(ir: HDSIR, world: HDS認知世界断片, requirements: tuple[HDS監査要求, ...], principle: HDS原理探索要求, 意味: dict[str, tuple[str, ...]]) -> HDSIR:
    if any(coord.種別 == '監査.構造' and str(coord.内容) == "v1" for coord in ir.座標):
        return ir
    coords = list(ir.座標)
    operations = list(ir.意味作用履歴)
    seen = {(str(coord.種別), " ".join(str(coord.内容).split()).strip()) for coord in coords}
    counter = 0

    def add(kind: str, content: object, 状態: 値状態 = 値状態.確定) -> None:
        nonlocal counter
        value = " ".join(str(content).split()).strip()
        if not value or (kind, value) in seen:
            return
        seen.add((kind, value))
        coords.append(HDS座標(f"archv1:{counter}", kind, value, 状態, 由来='公開HDS 構文化器 構造 v1', 再開放条件=("新観測・文脈変更・版更新で再監査する",)))
        counter += 1

    add('監査.構造', "v1")
    existing_kinds = {str(coord.種別) for coord in ir.座標}
    if "対象.始点" not in existing_kinds:
        for value in world.作用主体:
            add("対象.作用主体", value)
    if "対象.終点" not in existing_kinds:
        for value in world.対象:
            add("対象.作用対象", value)
    for kind, values in 意味.items():
        状態 = 値状態.推定 if kind == "不確実性.明示" else 値状態.確定
        for value in values:
            add(kind, value, 状態)
    for missing in world.未固定座標:
        add("監査.座標未固定", missing, 値状態.未観測)
    for requirement in requirements:
        add("監査.要求", requirement.種別, 値状態.留保)
    add("監査.原理段階", principle.段階.value, 値状態.推定 if principle.段階 != HDS原理段階.未形成 else 値状態.未観測)
    add("保持.契約", "全座標・全関係・不確実性・残差・由来・旧解釈を保持し、不可逆剪定しない")
    add("暫定性.既定", "原則暫定")
    operations.append(HDS意味作用('構文化器-構造-v1', "開放多層監査射影", ("normalized",), tuple(coord.座標ID for coord in coords if coord.座標ID.startswith("archv1:")), "座標固定・動態・暗黙知・論証・原理探索入力・監査要求を追加射影", 保持構造=("原文", "関係", "不確実性", "留保", "由来", "再開放条件"), 損失=('HDS Native/Kernelの導出規則と最終採否は公開構文化器へ含めない',), 検証=("日本語基底", "事実補完なし", "最終採否なし", "固定次元化なし")))
    return replace(ir, 座標=tuple(coords), 意味作用履歴=tuple(operations))


def 公開HDS詳細成果(ir: HDSIR) -> HDS構文化器成果:
    world, items, requirements, principle, 意味 = _抽出(ir)
    return HDS構文化器成果(_IRへ射影(ir, world, requirements, principle, 意味), world, items, requirements, principle, HDS保持契約())


def 公開HDSフロントエンド射影(ir: HDSIR) -> HDS構文化器成果:
    '既存HDS-IR互換を保ったまま、構造 v1の公開Front-End射影を重ねる。'
    return 公開HDS詳細成果(ir)


__all__ = ["公開HDS詳細成果", "公開HDSフロントエンド射影"]
