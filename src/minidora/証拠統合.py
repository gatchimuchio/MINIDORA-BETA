"""本文に明示された数値主張を、原文・条件・時点に結び付けて比較する。

局所文法における記載の分析であり、資料の真偽・独立性・世界の事実を認定しない。
分析の成立と記載値の採用は別の関門。原文の未解釈部分を削除して成功にしない。
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
from datetime import date
from fractions import Fraction
from hashlib import sha256
import json
import re
import unicodedata
from urllib.parse import urldefrag

from .能力合成 import _結果辞書, _参照結合
from .製品版.型 import 能力結果, 参照資料

証拠統合版 = "MINIDORA-証拠統合-v0.1"
# 単位は型と正確な倍率。物体・測定値・benchmarkの知識ではない。
_単位 = {
    "V": ("V", "1"), "mV": ("V", "0.001"), "kV": ("V", "1000"),
    "A": ("A", "1"), "mA": ("A", "0.001"),
    "W": ("W", "1"), "kW": ("W", "1000"),
    "m": ("m", "1"), "cm": ("m", "0.01"), "mm": ("m", "0.001"), "km": ("m", "1000"),
    "s": ("s", "1"), "ms": ("s", "0.001"),
    "kg": ("kg", "1"), "g": ("kg", "0.001"),
    "Pa": ("Pa", "1"), "kPa": ("Pa", "1000"),
    "Hz": ("Hz", "1"), "kHz": ("Hz", "1000"),
    "%": ("%", "1"), "個": ("個", "1"), "人": ("人", "1"), "円": ("円", "1"),
}
_数 = r"[+\-＋－]?[0-9０-９]+(?:[.．][0-9０-９]+)?(?:[eE][+\-]?[0-9]{1,3})?"
_文 = re.compile(
    r'(?:(?P<時点>[0-9]{4}-[0-9]{2}-[0-9]{2})時点、)?'
    r'(?:条件「(?P<条件>[^「」\r\n]{1,80})」では、)?'
    r'(?P<対象>[^\s。！？「」、]{1,80}?)の(?P<属性>[^\s。！？「」、]{1,80}?)は\s*'
    + rf'(?P<数>{_数})\s*(?P<単位>[A-Za-z%％個人円]+)'
    r'(?P<比較>以上|以下|未満|超)?\s*(?P<終止>ではありません|ではない|である|です|だ)?。?'
)
_比較 = {None: "一致", "以上": "以上", "以下": "以下", "未満": "未満", "超": "超"}
_反転 = {"一致": "不一致", "以上": "未満", "以下": "超", "未満": "以上", "超": "以下"}


def _文字検査(value: str, limit: int = 80) -> None:
    if type(value) is not str or not 0 < len(value) <= limit or value != value.strip():
        raise ValueError("識別文字列不正")
    if re.search(r'[\s。！？「」、\x00-\x1f\x7f]', value):
        raise ValueError("識別文字列に未対応文字")
    value.encode("utf-8")


@dataclass(frozen=True, slots=True)
class 証拠照合要求:
    対象: str
    属性: str
    単位: str
    条件: str | None = None
    時点: str | None = None
    最低資料系統数: int = 1

    def 検証(self) -> None:
        for x in (self.対象, self.属性, self.単位):
            _文字検査(x)
        if self.単位 not in _単位:
            raise ValueError("未対応単位")
        if self.条件 is not None:
            _文字検査(self.条件)
        if self.時点 is not None:
            if type(self.時点) is not str or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", self.時点):
                raise ValueError("時点形式不正")
            date.fromisoformat(self.時点)
        if type(self.最低資料系統数) is not int or not 1 <= self.最低資料系統数 <= 32:
            raise ValueError("資料系統数範囲外")


def _有理数(text: str) -> Fraction:
    text = unicodedata.normalize("NFKC", text)
    if len(text) > 128:
        raise ValueError("数値桁数上限")
    if "e" in text.lower() and abs(int(text.lower().split("e")[1])) > 100:
        raise ValueError("数値指数上限")
    return Fraction(text)


def _数表記(x: Fraction) -> str:
    """この版の10進入力・10進倍率の結果を、丸めず有限小数で表示する。"""
    sign = "-" if x < 0 else ""
    n, d = abs(x.numerator), x.denominator
    integer, remain = divmod(n, d)
    if not remain:
        return sign + str(integer)
    digits = []
    while remain:
        digit, remain = divmod(remain * 10, d)
        digits.append(str(digit))
        if len(digits) > 256:
            raise ValueError("小数表示上限")
    return sign + str(integer) + "." + "".join(digits)


def _共通域(claims: list[dict]) -> dict:
    lower, upper = None, None
    lower_closed = upper_closed = False
    excluded = set()
    for c in claims:
        x = Fraction(c["値"])
        op = c["比較"]
        if op == "不一致":
            excluded.add(x)
        if op in ("一致", "以上", "超"):
            closed = op != "超"
            if lower is None or x > lower:
                lower, lower_closed = x, closed
            elif x == lower:
                lower_closed &= closed
        if op in ("一致", "以下", "未満"):
            closed = op != "未満"
            if upper is None or x < upper:
                upper, upper_closed = x, closed
            elif x == upper:
                upper_closed &= closed
    empty = (lower is not None and upper is not None and
             (lower > upper or lower == upper and
              (not lower_closed or not upper_closed or lower in excluded)))
    single = not empty and lower is not None and lower == upper
    return {"空": empty, "一点": _数表記(lower) if single else None,
            "下限": _数表記(lower) if lower is not None else None, "下限を含む": lower_closed,
            "上限": _数表記(upper) if upper is not None else None, "上限を含む": upper_closed,
            "除外値": [_数表記(x) for x in sorted(excluded)]}


def _資料系統(refs: tuple[参照資料, ...]) -> dict[str, str]:
    """同じURLまたは同じ本文を連結して系統化する。独立性の推定はしない。"""
    parent = {r.識別子: r.識別子 for r in refs}
    def root(x):
        while x != parent[x]:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x
    seen = {}
    for r in refs:
        keys = [("本文", sha256(r.本文.encode()).hexdigest())]
        if r.URL:
            keys.append(("URL", urldefrag(r.URL)[0]))
        for key in keys:
            if key in seen:
                a, b = root(r.識別子), root(seen[key])
                parent[max(a, b)] = min(a, b)
            else:
                seen[key] = r.識別子
    return {r.識別子: root(r.識別子) for r in refs}


def _記録hash(資料: dict) -> str:
    return sha256(json.dumps(資料, ensure_ascii=False, sort_keys=True,
                             separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def 証拠記録整合(結果: 能力結果) -> bool:
    """記録・本文・資料の連結整合。署名・外部認証・意味の再検証ではない。"""
    try:
        _結果辞書(結果)
        資料 = dict(結果.データ)
        expected = 資料.pop("記録SHA256")
        return (資料["版"] == 証拠統合版 and 資料["報告本文"] == 結果.本文
                and 資料["資料原本"] == [r.辞書化() for r in sorted(_参照結合(結果.参照), key=lambda x: x.識別子)]
                and 資料["報告根拠"] == list(結果.根拠)
                and 結果.成立 is True and 結果.保留理由 == ""
                and 資料["採用可"] is (資料["判定"] == "記載値一致")
                and expected == _記録hash(資料))
    except (KeyError, TypeError, ValueError, RecursionError):
        return False


class 証拠統合器:
    """数値記載の比較報告を作る。矛盾時も報告は作るが採用値を出さない。"""

    def 実行(self, 要求: 証拠照合要求, 資料: tuple[参照資料, ...]) -> 能力結果:
        try:
            if not isinstance(要求, 証拠照合要求):
                raise ValueError("照合要求型不正")
            要求.検証()
            if type(資料) is not tuple or not 1 <= len(資料) <= 32:
                raise ValueError("資料数範囲外")
            _結果辞書(能力結果(True, "", 参照=資料))
            refs = tuple(sorted(_参照結合(資料), key=lambda r: r.識別子))
            if sum(len(r.本文.encode()) for r in refs) > 500_000:
                raise ValueError("資料本文サイズ上限")
            families = _資料系統(refs)
            base_unit, multiplier = _単位[要求.単位]
            claims, residuals, unrelated = [], [], []
            for 情報源 in refs:
                # 改行・句点を保持したまま局所単位へ分離。小数点では切らない。
                for part in re.finditer(r"[^。\n]+。?", 情報源.本文):
                    raw = part[0]
                    text = raw.strip()
                    if not text:
                        continue
                    start = part.start() + len(raw) - len(raw.lstrip())
                    end = part.end() - (len(raw) - len(raw.rstrip()))
                    証拠 = {"参照ID": 情報源.識別子, "開始": start, "終了": end, "原文": text}
                    if len(claims) + len(residuals) + len(unrelated) >= 512:
                        raise ValueError("記載数上限")
                    match = _文.fullmatch(text)
                    if match is None:
                        residuals.append({**証拠, "理由": "未対応記載・条件・文脈"})
                        continue
                    fields = match.groupdict()
                    if fields["対象"] != 要求.対象 or fields["属性"] != 要求.属性:
                        if f"{要求.対象}の{要求.属性}は" in text:
                            residuals.append({**証拠, "理由": "対象と属性の境界が曖昧"})
                        else:
                            unrelated.append({**証拠, "理由": "別対象または別属性"})
                        continue
                    try:
                        if fields["時点"] is not None:
                            date.fromisoformat(fields["時点"])
                        unit = unicodedata.normalize("NFKC", fields["単位"])
                        unit_base, scale = _単位[unit]
                        if unit_base != base_unit:
                            raise ValueError("次元不一致")
                        value = _有理数(fields["数"]) * Fraction(scale)
                    except (KeyError, ValueError):
                        residuals.append({**証拠, "理由": "単位・数値・時点が未対応または不整合"})
                        continue
                    op = _比較[fields["比較"]]
                    if fields["終止"] in ("ではない", "ではありません"):
                        op = _反転[op]
                    claims.append({**証拠, "主張ID": f"主張:{len(claims)+1:04d}",
                                   "対象": fields["対象"], "属性": fields["属性"],
                                   "条件": fields["条件"], "時点": fields["時点"],
                                   "元数値": fields["数"], "元単位": fields["単位"],
                                   "値": _数表記(value), "単位": base_unit, "比較": op,
                                   "資料系統": families[情報源.識別子]})
            groups = []
            keys = sorted({(c["条件"], c["時点"]) for c in claims},
                          key=lambda k: (k[0] or "", k[1] or ""))
            for condition, moment in keys:
                members = [c for c in claims if (c["条件"], c["時点"]) == (condition, moment)]
                common = _共通域(members)
                状態 = "記載競合" if common["空"] else "記載値一致" if common["一点"] is not None else "範囲のみ"
                groups.append({"条件": condition, "時点": moment, "状態": 状態, "共通域": common,
                               "主張ID": [c["主張ID"] for c in members],
                               "資料系統数": len({c["資料系統"] for c in members})})
            selected = [g for g in groups if (要求.条件 is None or g["条件"] == 要求.条件)
                        and (要求.時点 is None or g["時点"] == 要求.時点)]
            未知_範囲 = any((要求.条件 is not None and g["条件"] is None or
                                 要求.時点 is not None and g["時点"] is None) for g in groups)
            reasons = []
            if not selected:
                reasons.append("対象記載なし")
            if any(g["状態"] == "記載競合" for g in selected):
                reasons.append("記載競合")
            if residuals:
                reasons.append("未解釈記載あり")
            if 未知_範囲:
                reasons.append("適用条件または時点が未記載")
            if len(selected) > 1:
                reasons.append("適用範囲未選択")
            if any(g["状態"] == "範囲のみ" for g in selected):
                reasons.append("一意値なし")
            if any(g["資料系統数"] < 要求.最低資料系統数 for g in selected):
                reasons.append("資料系統数不足")
            adopted = not reasons
            value = (_数表記(Fraction(selected[0]["共通域"]["一点"]) / Fraction(multiplier))
                     if adopted else None)
            範囲 = (f'条件={selected[0]["条件"] or "未記載"}、時点={selected[0]["時点"] or "未記載"}。'
                     if adopted else "")
            report = (f"{要求.対象}の{要求.属性}: "
                      + (f"記載値は{value} {要求.単位}で一致。" if adopted else "採用保留（" + "、".join(reasons) + "）。")
                      + 範囲 + "資料上の記載比較であり、事実性・出典独立性は未確認。")
            資料 = {"版": 証拠統合版, "要求": asdict(要求), "判定": "記載値一致" if adopted else "採用保留",
                    "採用可": adopted, "採用値": value, "採用単位": 要求.単位,
                    "採用条件": selected[0]["条件"] if adopted else None,
                    "採用時点": selected[0]["時点"] if adopted else None,
                    "理由": reasons, "主張": claims, "群": groups, "残差": residuals, "対象外": unrelated,
                    "資料原本": [r.辞書化() for r in refs], "報告本文": report,
                    "報告根拠": [c["主張ID"] for c in claims],
                    "資料独立性": "未確認", "事実性": "未確認",
                    "条件未記載の解釈": "未記載同士の記載比較に限定。現実で同じ条件とは認定しない"}
            資料["記録SHA256"] = _記録hash(資料)
            結果 = 能力結果(True, report, 根拠=tuple(c["主張ID"] for c in claims), 参照=refs, データ=資料)
            if len(json.dumps(_結果辞書(結果), ensure_ascii=False).encode()) > 1_500_000:
                raise ValueError("証拠記録サイズ上限")
            return 結果
        except (TypeError, ValueError, AttributeError, RecursionError, OverflowError) as exc:
            return 能力結果(False, "", 保留理由=f"証拠統合契約違反:{type(exc).__name__}")


def 記載値を採用(結果: 能力結果) -> 能力結果:
    """一意で競合・未解釈のない記載値のみを返す。現実の事実認定ではない。"""
    if not isinstance(結果, 能力結果) or not 結果.成立 or not 証拠記録整合(結果):
        return 能力結果(False, "", 保留理由="証拠報告の不成立または整合違反")
    資料 = 結果.データ
    if not 資料["採用可"]:
        return 能力結果(False, "", 参照=結果.参照, 保留理由="、".join(資料["理由"]),
                        データ={"証拠記録SHA256": 資料["記録SHA256"], "理由": list(資料["理由"])})
    return 能力結果(True, f'{資料["採用値"]} {資料["採用単位"]}', 根拠=結果.根拠, 参照=結果.参照,
                    データ={"値": 資料["採用値"], "単位": 資料["採用単位"],
                            "条件": 資料["採用条件"], "時点": 資料["採用時点"],
                            "意味": "資料記載値。事実性未確認", "証拠記録SHA256": 資料["記録SHA256"]})
