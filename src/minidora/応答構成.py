"""証拠報告から、結論・適用範囲・反対記載を保った日本語の回答を構成する。

数値証拠報告に限定した計画・表現器。自由作文や新しい事実認定は行わない。
内部の成立は回答の構成成功であり、記載値の採用・事実性とは区別する。
"""
from __future__ import annotations

from copy import deepcopy
from dataclasses import asdict, dataclass
from datetime import datetime
from hashlib import sha256
import json
import unicodedata

from .能力合成 import _結果辞書, _参照結合
from .製品版.型 import 能力結果, 参照資料
from .証拠統合 import 証拠統合器, 証拠照合要求, 証拠記録整合, 証拠統合版

応答構成版 = "MINIDORA-応答構成-v0.1"
_理由文 = {
    "対象記載なし": "指定した対象・属性・適用範囲に対応する数値記載がありません。",
    "記載競合": "同じ比較群の記載を同時には満たせないため、一つの値を選べません。",
    "未解釈記載あり": "解釈できていない記載が残っており、条件や留保がないとは判断できません。",
    "適用条件または時点が未記載": "条件または時点が未記載の資料を、指定範囲に適用できるか未確定です。",
    "適用範囲未選択": "条件または時点が異なる複数の比較群があるため、適用する群の指定が必要です。",
    "一意値なし": "記載の共通範囲はありますが、値を一点には決められません。",
    "資料系統数不足": "重複をまとめた資料系統数が、指定された最低数に達していません。",
}
_比較文 = {"一致": "に等しい", "不一致": "ではない", "以上": "以上",
           "以下": "以下", "未満": "未満", "超": "を超える"}
_限界文 = ("これは渡された資料の記載比較に基づく説明です。資料の真偽と出典の独立性は未確認です。"
          "条件や時点が未記載であることは、現実に同じ条件であることや最新であることを意味しません。"
          "引用は記載位置の追跡であり、現実の事実を保証するものではありません。")


@dataclass(frozen=True, slots=True)
class 応答仕様:
    形式: str = "段落"
    詳細度: str = "要点"
    最大文字数: int = 20000

    def 検証(self) -> None:
        if self.形式 not in ("段落", "箇条書き") or self.詳細度 not in ("要点", "詳細"):
            raise ValueError("未対応の応答形式・詳細度")
        if type(self.最大文字数) is not int or not 1 <= self.最大文字数 <= 100000:
            raise ValueError("応答文字数上限不正")


def _符号(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _指紋(value: object) -> str:
    return sha256(_符号(value)).hexdigest()


def _表示値(value: str) -> str:
    '外部文字列を一行の引用として表示する。原値は資料と原資料に保持する。\n\n    制御・双方向文字と表示構文の記号を可視エスケープする。HTML用の出力ではない。\n    '
    if type(value) is not str:
        raise ValueError("表示値は文字列")
    value.encode("utf-8")
    parts = []
    for c in value:
        if unicodedata.category(c) in ("Cc", "Cf", "Zl", "Zp") or c in '\\<>&[](){}!*_`#「」〔〕':
            parts.append(f"\\u{ord(c):04x}")
        else:
            parts.append(c)
    return "「" + "".join(parts) + "」"


def 能力結果を復元(raw: dict) -> 能力結果:
    """既存合成器のJSON互換結果を復元する。任意オブジェクトへ型変換しない。"""
    if type(raw) is not dict or set(raw) != {"成立", "本文", "根拠", "参照", "データ", "保留理由"}:
        raise ValueError("能力結果の項目不一致")
    if type(raw["参照"]) not in (list, tuple) or type(raw["根拠"]) not in (list, tuple):
        raise ValueError("能力結果の配列型不正")
    refs = []
    for item in raw["参照"]:
        if type(item) is not dict or set(item) != set(参照資料.__dataclass_fields__):
            raise ValueError("参照資料の項目不一致")
        item = dict(item)
        if item["公開時刻"] is not None:
            if type(item["公開時刻"]) is not str:
                raise ValueError("公開時刻型不正")
            item["公開時刻"] = datetime.fromisoformat(item["公開時刻"])
        refs.append(参照資料(**item))
    結果 = 能力結果(raw["成立"], raw["本文"], tuple(raw["根拠"]), tuple(refs),
                        deepcopy(raw["データ"]), raw["保留理由"])
    _結果辞書(結果)
    return 結果


def _報告を確認(report: 能力結果) -> 能力結果:
    # hash一致を意味的整合の代用にしない。元資料から既存比較器を再実行する。
    if not 証拠記録整合(report) or report.データ["版"] != 証拠統合版:
        raise ValueError("証拠報告の整合・版不一致")
    request = 証拠照合要求(**report.データ["要求"])
    再生 = 証拠統合器().実行(request, deepcopy(report.参照))
    if not 再生.成立 or _符号(_結果辞書(再生)) != _符号(
            {**_結果辞書(report), "参照": [r.辞書化() for r in sorted(
                _参照結合(report.参照), key=lambda r: r.識別子)]}):
        raise ValueError("証拠報告と元資料からの再計算が不一致")
    return 再生


def _適用範囲(condition: str | None, moment: str | None) -> str:
    return ("条件=" + (_表示値(condition) if condition is not None else "未記載")
            + "、時点=" + (_表示値(moment) if moment is not None else "未記載"))


def _共通域文(group: dict, unit: str) -> str:
    domain = group["共通域"]
    if domain["空"]:
        return "これらを同時に満たす値はありません。"
    if domain["一点"] is not None:
        return f'共通する値は{domain["一点"]} {unit}です。'
    bounds = []
    if domain["下限"] is not None:
        bounds.append(f'{domain["下限"]} {unit}' + ("以上" if domain["下限を含む"] else "を超える"))
    if domain["上限"] is not None:
        bounds.append(f'{domain["上限"]} {unit}' + ("以下" if domain["上限を含む"] else "未満"))
    text = "共通範囲は" + "かつ".join(bounds) + "です。" if bounds else "上下限は定まっていません。"
    if domain["除外値"]:
        text += "除外値は" + "、".join(f"{x} {unit}" for x in domain["除外値"]) + "です。"
    return text + "一意の値には決められません。"


def _構成(reports: tuple[能力結果, ...], spec: 応答仕様) -> 能力結果:
    spec.検証()
    if type(reports) is not tuple or not 1 <= len(reports) <= 8:
        raise ValueError("報告数は1〜8のtuple")
    # 入力のJSON契約・サイズを再比較より前に確認する。
    raw = [_結果辞書(r) for r in reports]
    if len(_符号(raw)) > 2000000:
        raise ValueError("応答入力サイズ上限")
    reports = tuple(_報告を確認(r) for r in reports)
    if len({r.データ["記録SHA256"] for r in reports}) != len(reports):
        raise ValueError("同一報告の重複")
    refs = tuple(sorted(_参照結合(s for r in reports for s in r.参照), key=lambda s: s.識別子))
    labels = {s.識別子: i for i, s in enumerate(refs, 1)}
    nodes, citations, states = [], [], []
    sources = {s.識別子: s for s in refs}

    def node(kind: str, text: str, origin: list[dict], 情報源_ids=()) -> None:
        numbers = sorted({labels[s] for s in 情報源_ids})
        nodes.append({"単位ID": f"応答:{len(nodes)+1:04d}", "種別": kind, "文": text,
                      "由来": origin, "出典番号": numbers})

    for ri, report in enumerate(reports, 1):
        資料 = report.データ
        request = 資料["要求"]
        all_ids = [s.識別子 for s in report.参照]
        状態 = {"報告番号": ri, "対象": request["対象"], "属性": request["属性"],
                 "判定": 資料["判定"], "記載値採用可": 資料["採用可"],
                 "条件": 資料["採用条件"], "時点": 資料["採用時点"],
                 "理由": deepcopy(資料["理由"]), "証拠記録SHA256": 資料["記録SHA256"]}
        states.append(状態)
        def origin(section, key):
            return {"報告番号": ri, "区分": section, "キー": key}
        node("見出し", f'{ri}. {_表示値(request["対象"])}の{_表示値(request["属性"])}', [origin("要求", "対象・属性")])
        if 資料["採用可"]:
            text = f'資料上の記載から採用できる値は{資料["採用値"]} {資料["採用単位"]}です。'
            text += _適用範囲(資料["採用条件"], 資料["採用時点"]) + "の記載比較に限ります。"
        else:
            text = "現時点の資料比較では値の採用を保留します。"
        node("結論", text, [origin("判定", 資料["判定"])], all_ids)
        requested_範囲 = ("指定条件=" + (_表示値(request["条件"]) if request["条件"] is not None else "指定なし")
                           + "、指定時点=" + (_表示値(request["時点"]) if request["時点"] is not None else "指定なし"))
        node("要求条件", f'{requested_範囲}、最低資料系統数={request["最低資料系統数"]}です。', [origin("要求", "適用範囲・資料数")])
        for reason in 資料["理由"]:
            node("保留理由", _理由文[reason], [origin("理由", reason)], all_ids)

        claim_origins = {}
        for section in ("主張", "残差", "対象外"):
            for index, item in enumerate(資料[section]):
                ref = sources[item["参照ID"]]
                start, end = item["開始"], item["終了"]
                if type(start) is not int or type(end) is not int or not 0 <= start < end <= len(ref.本文):
                    raise ValueError("根拠位置不正")
                if ref.本文[start:end] != item["原文"]:
                    raise ValueError("根拠原文不一致")
                link = {**origin(section, str(index)), "参照ID": ref.識別子,
                        "出典番号": labels[ref.識別子], "開始": start, "終了": end, "原文": item["原文"]}
                citations.append(link)
                if section == "主張":
                    claim_origins[item["主張ID"]] = link
        claims = {c["主張ID"]: c for c in 資料["主張"]}
        for gi, group in enumerate(資料["群"]):
            members = [claims[k] for k in group["主張ID"]]
            selected = ((request["条件"] is None or group["条件"] == request["条件"])
                        and (request["時点"] is None or group["時点"] == request["時点"]))
            text = ("比較対象の群: " if selected else "指定範囲と一致しない群（記録保持）: ")
            text += _適用範囲(group["条件"], group["時点"]) + "。"
            text += _共通域文(group, members[0]["単位"])
            text += f'重複をまとめた資料系統数は{group["資料系統数"]}です（独立性は未確認）。'
            node("比較群", text, [origin("群", str(gi))], [c["参照ID"] for c in members])
            # 同じ意味の記載だけを束ね、反対記載・単位・適用範囲を落とさない。
            bundles: dict[tuple[str, str, str], list[dict]] = {}
            for c in members:
                bundles.setdefault((c["値"], c["単位"], c["比較"]), []).append(c)
            for (value, unit, op), bundle in bundles.items():
                text = f'資料の記載は、{value} {unit}{_比較文[op]}というものです。'
                links = [claim_origins[c["主張ID"]] for c in bundle]
                node("記載", text, links, [c["参照ID"] for c in bundle])
                if spec.詳細度 == "詳細":
                    for c in bundle:
                        node("原文引用", "原文: " + _表示値(c["原文"]), [claim_origins[c["主張ID"]]], [c["参照ID"]])
        for section, kind in (("残差", "未解釈"), ("対象外", "対象外")):
            items = [c for c in citations if c["報告番号"] == ri and c["区分"] == section]
            if items:
                text = (f"解釈できていない記載が{len(items)}件あります。これらを解消せず、値は採用しません。"
                        if section == "残差" else f"別対象・別属性の記載が{len(items)}件あり、この値の根拠には用いていません。")
                node(kind, text, items, [c["参照ID"] for c in items])
                if spec.詳細度 == "詳細":
                    for link in items:
                        node("原文引用", kind + "の原文: " + _表示値(link["原文"]), [link], [link["参照ID"]])
    node("限界", _限界文, [{"区分": "契約", "版": 応答構成版}])
    for ref in refs:
        n = labels[ref.識別子]
        # URLを回答本文へ埋めず、元の参照構造に保持する。表示器で自動実行しない。
        node("出典", f'出典{n}: 題名={_表示値(ref.題名)}、提供元={_表示値(ref.出典)}。',
             [{"区分": "資料情報", "参照ID": ref.識別子}])
    parts, segments, offset = [], [], 0
    for item in nodes:
        prefix = "- " if spec.形式 == "箇条書き" and item["種別"] not in ("見出し", "出典") else ""
        marks = "".join(f"〔出典{n}〕" for n in item["出典番号"])
        text = prefix + item["文"] + marks
        separator = "\n" if spec.形式 == "箇条書き" else "\n\n"
        if parts:
            parts.append(separator)
            offset += len(separator)
        segments.append({"単位ID": item["単位ID"], "開始": offset, "終了": offset+len(text), "本文": text})
        parts.append(text)
        offset += len(text)
    body = "".join(parts)
    if len(body) > spec.最大文字数:
        return 能力結果(False, "", 保留理由="必須内容を保持すると応答文字数上限を超える",
                        データ={"必要文字数": len(body), "指定上限": spec.最大文字数, "版": 応答構成版})
    payload = {"版": 応答構成版, "用途": "利用者向け説明。新たな根拠資料ではない",
               "仕様": asdict(spec), "項目状態": states, "応答計画": nodes,
               "文章対応": segments, "引用対応": citations,
               "元報告": [_結果辞書(r) for r in reports], "事実認定": "未実施"}
    bases = tuple(f'証拠報告:{i}:{r.データ["記録SHA256"]}' for i, r in enumerate(reports, 1))
    結果 = 能力結果(True, body, 根拠=bases, 参照=refs, データ=payload)
    payload["応答SHA256"] = _指紋(_結果辞書(結果))
    if len(_符号(_結果辞書(結果))) > 2000000:
        return 能力結果(False, "", 保留理由="応答記録サイズ上限")
    return 結果


class 応答構成器:
    def 実行(self, 報告: tuple[能力結果, ...], 仕様: 応答仕様 | None = None) -> 能力結果:
        try:
            if 仕様 is not None and not isinstance(仕様, 応答仕様):
                raise ValueError("応答仕様型不正")
            return _構成(deepcopy(報告), 仕様 if 仕様 is not None else 応答仕様())
        except (TypeError, ValueError, KeyError, AttributeError, RecursionError, OverflowError) as exc:
            return 能力結果(False, "", 保留理由=f"応答構成契約違反:{type(exc).__name__}")


def 応答記録整合(結果: 能力結果) -> bool:
    """元資料からの比較と回答構成を再実行して検査する。真正性は証明しない。"""
    try:
        if not isinstance(結果, 能力結果) or 結果.成立 is not True:
            return False
        _結果辞書(結果)
        if 結果.データ["版"] != 応答構成版:
            return False
        reports = tuple(能力結果を復元(r) for r in 結果.データ["元報告"])
        spec = 応答仕様(**結果.データ["仕様"])
        expected = 応答構成器().実行(reports, spec)
        # 合成器が付加する消費資料の来歴は、回答の根拠として使った資料と同一であること。
        value = {**_結果辞書(結果), "参照": [r.辞書化() for r in sorted(
            _参照結合(結果.参照), key=lambda r: r.識別子)]}
        return expected.成立 and _符号(_結果辞書(expected)) == _符号(value)
    except (TypeError, ValueError, KeyError, AttributeError, RecursionError, OverflowError):
        return False
