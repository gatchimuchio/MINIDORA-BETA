'指定素材を見出し・段落・引用へ構成する。自由文からの内容発明ではない。\n\n素材の採用・同伴・構成は明示資料。後段の差分編集は文章編集へ分離する。\n'
from __future__ import annotations

from copy import deepcopy
from dataclasses import asdict, dataclass
from hashlib import sha256
import json
import unicodedata

from .能力合成 import _結果辞書, _参照結合
from .応答構成 import 能力結果を復元
from .製品版.型 import 能力結果

文章版 = "MINIDORA-文章作成編集-v0.1"
_役割 = ("見出し", "段落", "箇条書き", "番号付き", "引用", "条件", "留保")


class 文章境界違反(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class 文章断片:
    素材ID: str
    開始: int
    終了: int


@dataclass(frozen=True, slots=True)
class 文章単位:
    識別子: str
    種別: str
    断片: tuple[文章断片, ...]
    同伴: tuple[str, ...] = ()
    必須: bool = False
    保護: bool = False


@dataclass(frozen=True, slots=True)
class 文章仕様:
    単位群: tuple[文章単位, ...]
    採用順序: tuple[str, ...]
    最大文字数: int = 32768


@dataclass(frozen=True, slots=True)
class 保護範囲:
    識別子: str
    開始: int
    終了: int
    期待原文: str


def _符号(value) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _指紋(value) -> str:
    return sha256(_符号(value)).hexdigest()


def _文字(value, limit=32768, *, 空可=True):
    if type(value) is not str or len(value) > limit or (not 空可 and not value.strip()):
        raise 文章境界違反("文字列の型・長さ・空入力が不正")
    value.encode("utf-8")
    if any(unicodedata.category(c) in ("Cf", "Zl", "Zp", "Cs") or
           (unicodedata.category(c) == "Cc" and c not in "\t\n\r") for c in value):
        raise 文章境界違反("未対応制御文字を削除せず保留")


def _名前(value):
    _文字(value, 128, 空可=False)
    if value != value.strip() or any(c.isspace() for c in value):
        raise 文章境界違反("識別子の空白が不正")


def _範囲(start, end, text, *, 空可=False):
    if type(start) is not int or type(end) is not int or not 0 <= start <= end <= len(text):
        raise 文章境界違反("文字範囲が不正")
    if not 空可 and start == end:
        raise 文章境界違反("空の文字範囲")


def _上限(n):
    if type(n) is not int or not 1 <= n <= 100000:
        raise 文章境界違反("最大文字数は1〜100000の整数")


def _項目(raw, cls):
    if type(raw) is not dict or set(raw) != set(cls.__dataclass_fields__):
        raise 文章境界違反('構成資料の項目不一致')


def 文章仕様を復元(raw: dict) -> 文章仕様:
    _項目(raw, 文章仕様)
    if type(raw["単位群"]) not in (list, tuple) or type(raw["採用順序"]) not in (list, tuple):
        raise 文章境界違反('構成資料の列型不正')
    units = []
    for row in raw["単位群"]:
        _項目(row, 文章単位)
        if type(row["断片"]) not in (list, tuple) or type(row["同伴"]) not in (list, tuple):
            raise 文章境界違反("文章単位の列型不正")
        for piece in row["断片"]:
            _項目(piece, 文章断片)
        units.append(文章単位(row["識別子"], row["種別"],
            tuple(文章断片(**p) for p in row["断片"]), tuple(row["同伴"]), row["必須"], row["保護"]))
    return 文章仕様(tuple(units), tuple(raw["採用順序"]), raw["最大文字数"])


def _複写対応(spans, start, end, destination):
    """残した部分だけを原文範囲へ再対応する。変更文に古い出典を継承しない。"""
    out = []
    for row in spans:
        a, b = max(start, row["開始"]), min(end, row["終了"])
        if a < b:
            origin = deepcopy(row["由来"])
            origin["開始"] += a - row["開始"]
            origin["終了"] = origin["開始"] + b - a
            out.append({"開始": destination + a-start, "終了": destination + b-start, "由来": origin})
    return out


def _保護確認(rows, text):
    if type(rows) is not list or len(rows) > 256:
        raise 文章境界違反("保護範囲の列型・件数不正")
    seen, previous = set(), 0
    for row in sorted(rows, key=lambda r: r["開始"]):
        _項目(row, 保護範囲)
        _名前(row["識別子"])
        _範囲(row["開始"], row["終了"], text)
        if row["開始"] < previous or row["識別子"] in seen:
            raise 文章境界違反("保護範囲の重複・交差")
        _文字(row["期待原文"], 100000)
        if text[row["開始"]:row["終了"]] != row["期待原文"]:
            raise 文章境界違反("保護範囲と期待原文の不一致")
        previous = row["終了"]; seen.add(row["識別子"])


def _初期構成(root):
    if type(root) is not dict:
        raise 文章境界違反("文章原本の型不正")
    if root.get("種別") == "本文":
        if set(root) != {"種別", "本文", "保護", "最大文字数"}:
            raise 文章境界違反("文章原本の項目不一致")
        _上限(root["最大文字数"])
        text = root["本文"]; _文字(text, root["最大文字数"], 空可=False)
        _保護確認(root["保護"], text)
        return {"本文": text, "対応": [{"開始": 0, "終了": len(text),
                "由来": {"種別": "原本文", "ID": "原本文", "開始": 0, "終了": len(text)}}],
                "保護": deepcopy(root["保護"]), "初期採用単位": [], "初期省略単位": [],
                "初期単位位置": [], "初期未使用素材範囲": [], "直近差分": []}
    if set(root) != {"種別", "仕様", "素材"} or root["種別"] != "構成" or type(root["素材"]) is not dict:
        raise 文章境界違反("文章構成の原本不正")
    spec = 文章仕様を復元(root["仕様"])
    _上限(spec.最大文字数)
    if not 1 <= len(root["素材"]) <= 128 or type(spec.単位群) is not tuple or not 1 <= len(spec.単位群) <= 128:
        raise 文章境界違反("素材・文章単位は1〜128件")
    materials, units = {}, {}
    for key, raw in root["素材"].items():
        _名前(key)
        value = 能力結果を復元(raw)
        if not value.成立:
            raise 文章境界違反("未成立素材を文章の確定内容にしない")
        _文字(value.本文, 100000)
        materials[key] = value
    _参照結合(ref for value in materials.values() for ref in value.参照)
    for unit in spec.単位群:
        _名前(unit.識別子)
        if (unit.識別子 in units or unit.種別 not in _役割 or type(unit.必須) is not bool
                or type(unit.保護) is not bool or not 1 <= len(unit.断片) <= 32):
            raise 文章境界違反("文章単位のID・種別・指定が不正")
        if len(unit.同伴) > 128 or len(set(unit.同伴)) != len(unit.同伴):
            raise 文章境界違反("同伴指定の重複・上限")
        for item in unit.同伴:
            _名前(item)
        for part in unit.断片:
            _名前(part.素材ID)
            if part.素材ID not in materials:
                raise 文章境界違反("構成素材が不足")
            _範囲(part.開始, part.終了, materials[part.素材ID].本文)
        units[unit.識別子] = unit
    if type(spec.採用順序) is not tuple or not 1 <= len(spec.採用順序) <= 128 or len(set(spec.採用順序)) != len(spec.採用順序):
        raise 文章境界違反("採用順序の型・重複・件数不正")
    for unit in spec.単位群:
        if any(k not in units for k in unit.同伴):
            raise 文章境界違反("同伴単位が不足")
    selected, visited = [], set()
    def visit(key):
        _名前(key)
        if key not in units:
            raise 文章境界違反("採用する文章単位が未宣言")
        if key in visited:
            return
        visited.add(key); selected.append(key)
        for other in units[key].同伴:
            visit(other)
    for key in spec.採用順序:
        visit(key)
    for unit in spec.単位群:
        if unit.必須 or unit.種別 in ("条件", "留保"):
            visit(unit.識別子)
    # 128単位の上限により再帰は有限。循環する同伴は各単位を一度だけ含む。
    text, spans, protected, positions = "", [], [], []
    used = {key: [] for key in materials}
    def mark(literal, name):
        nonlocal text
        if literal:
            spans.append({"開始": len(text), "終了": len(text)+len(literal),
                          "由来": {"種別": "構成表記", "ID": name, "表記": literal, "開始": 0, "終了": len(literal)}})
            text += literal
    number = 0
    for index, key in enumerate(selected):
        unit = units[key]
        if index:
            mark("\n\n", "区切り:" + key)
        start = len(text)
        content, local = "", []
        for part in unit.断片:
            情報源 = materials[part.素材ID].本文
            literal = 情報源[part.開始:part.終了]
            local.append({"開始": len(content), "終了": len(content)+len(literal),
                "由来": {"種別": "素材", "ID": part.素材ID, "開始": part.開始, "終了": part.終了}})
            content += literal; used[part.素材ID].append((part.開始, part.終了))
        if not content.strip() or (unit.種別 == "見出し" and ("\r" in content or "\n" in content)):
            raise 文章境界違反("空の文章単位または複数行の見出し")
        number = number + 1 if unit.種別 == "番号付き" else 0
        prefix = {"見出し": "# ", "段落": "", "箇条書き": "- ", "番号付き": f"{number}. ",
                  "引用": "> ", "条件": "条件: ", "留保": "留保: "}[unit.種別]
        localpos = 0
        for li, line in enumerate(content.splitlines(keepends=True)):
            decoration = prefix if li == 0 or unit.種別 == "引用" else "  " if unit.種別 in ("箇条書き", "番号付き") else ""
            mark(decoration, "接頭:" + key + ":" + str(li))
            spans.extend(_複写対応(local, localpos, localpos+len(line), len(text)))
            text += line; localpos += len(line)
        positions.append({"単位ID": key, "種別": unit.種別, "開始": start, "終了": len(text)})
        if unit.保護 or unit.種別 in ("引用", "条件", "留保"):
            protected.append(asdict(保護範囲(key, start, len(text), text[start:])))
        if len(text) > spec.最大文字数:
            raise 文章境界違反("文章が最大文字数を超える。内容を削らず保留")
    omitted = []
    for key, value in materials.items():
        pos = 0
        for a, b in sorted(used[key]):
            if pos < a:
                omitted.append({"素材ID": key, "開始": pos, "終了": a})
            pos = max(pos, b)
        if pos < len(value.本文):
            omitted.append({"素材ID": key, "開始": pos, "終了": len(value.本文)})
    return {"本文": text, "対応": spans, "保護": protected, "初期採用単位": selected,
            "初期省略単位": [u.識別子 for u in spec.単位群 if u.識別子 not in visited],
            "初期単位位置": positions, "初期未使用素材範囲": omitted, "直近差分": []}


def _由来原文(origin, root, history):
    if origin["種別"] == "素材":
        return root["素材"][origin["ID"]]["本文"]
    if origin["種別"] == "原本文":
        return root["本文"]
    if origin["種別"] == "構成表記":
        return origin["表記"]
    if origin["種別"] == "修正文":
        event = history[origin["改訂"]-1]
        return next(r["置換文"] for r in event["修正"] if r["識別子"] == origin["ID"])
    raise 文章境界違反("未知の文章由来")


def _結果(root, history, 状態):
    text = 状態["本文"]
    limit = root["最大文字数"] if root["種別"] == "本文" else root["仕様"]["最大文字数"]
    _文字(text, limit, 空可=False)
    _保護確認(状態["保護"], text)
    pos = 0
    if len(状態["対応"]) > 8192:
        raise 文章境界違反("文章対応数上限")
    for span in 状態["対応"]:
        origin = span["由来"]
        情報源 = _由来原文(origin, root, history)
        if (span["開始"] != pos or not span["開始"] < span["終了"] <= len(text)
                or 情報源[origin["開始"]:origin["終了"]] != text[span["開始"]:span["終了"]]):
            raise 文章境界違反("文章の連続対応・由来原文が不一致")
        pos = span["終了"]
    if pos != len(text):
        raise 文章境界違反("対応のない文章が残る")
    keys = {s["由来"]["ID"] for s in 状態["対応"] if s["由来"]["種別"] == "素材"}
    refs = _参照結合(r for key in sorted(keys) for r in 能力結果を復元(root["素材"][key]).参照) if keys else ()
    資料 = {"版": 文章版, "原本": deepcopy(root), "編集履歴": deepcopy(history),
            "改訂": len(history), "本文SHA256": sha256(text.encode()).hexdigest(),
            **{k: deepcopy(v) for k, v in 状態.items() if k != "本文"},
            "初期構成情報の範囲": "初期単位位置と未使用素材範囲は編集前。現行の文字由来は対応を参照",
            "保証範囲": "指定構成・文字対応・保護範囲。意味同値性、事実性、引用選択の適切さは未判定"}
    資料["記録SHA256"] = _指紋({"本文": text, "データ": 資料})
    結果 = 能力結果(True, text, 参照=refs, データ=資料)
    if len(_符号(_結果辞書(結果))) > 2000000:
        raise 文章境界違反("文章記録サイズ上限")
    return 結果


def _失敗(exc):
    return 能力結果(False, "", 保留理由="文章処理不成立:" + type(exc).__name__,
        データ={"診断": str(exc) if isinstance(exc, 文章境界違反) else type(exc).__name__})


def _原本結果(root):
    if len(_符号(root)) > 500000:
        raise 文章境界違反("文章原本サイズ上限")
    return _結果(root, [], _初期構成(root))


def 文章を作る(素材: dict[str, 能力結果], 仕様: 文章仕様) -> 能力結果:
    try:
        if type(素材) is not dict or type(仕様) is not 文章仕様:
            raise 文章境界違反("文章素材または仕様型不正")
        root = {"種別": "構成", "仕様": asdict(仕様),
                "素材": {key: _結果辞書(v) for key, v in 素材.items()}}
        return _原本結果(root)
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
        return _失敗(exc)


def 文章を取り込む(本文: str, 保護: tuple[保護範囲, ...] = (), *, 最大文字数: int = 32768) -> 能力結果:
    try:
        if type(保護) is not tuple or any(type(p) is not 保護範囲 for p in 保護):
            raise 文章境界違反("保護範囲の型不正")
        return _原本結果({"種別": "本文", "本文": 本文, "保護": [asdict(p) for p in 保護], "最大文字数": 最大文字数})
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
        return _失敗(exc)
