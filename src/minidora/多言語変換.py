'日本語を基底に、限定した数値記載と文書依頼を日英間で対訳する。\n\n語彙は呼出側の資料。未知語・未知構文を部分訳で隠さない。翻訳と実行は別責任。\n'
from __future__ import annotations

from collections.abc import Callable
from copy import deepcopy
from dataclasses import asdict, dataclass
from datetime import date
from hashlib import sha256
import json
import re
import unicodedata

from .能力合成 import _結果辞書
from .製品版.型 import 能力結果

多言語版 = "MINIDORA-多言語-v0.1"
_比較英語 = {"一致": "exactly", "以上": "at least", "以下": "at most",
             "未満": "less than", "超": "greater than"}
_比較日本語 = {"一致": "", "以上": "以上", "以下": "以下", "未満": "未満", "超": "超"}
_抽出語 = {"数字": "numbers", "URL": "URLs", "キーワード": "keywords"}
_数 = r"[+-]?[0-9]{1,64}(?:\.[0-9]{1,64})?(?:[eE][+-]?[0-9]{1,3})?"


class _未対応(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class 対訳語:
    識別子: str
    種別: str
    日本語: str
    英語: str


def _符号(value) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _文字(value, maximum=128):
    if type(value) is not str or not value or value != value.strip() or len(value) > maximum:
        raise _未対応("語・識別子の型または長さが不正")
    if any(unicodedata.category(c) in ("Cc", "Cf", "Zl", "Zp", "Cs") for c in value):
        raise _未対応("制御文字を含む語は未対応")


class _対訳辞書:
    def __init__(self, words: tuple[対訳語, ...]):
        if type(words) is not tuple or len(words) > 128:
            raise _未対応("対訳語は128件以内のtuple")
        self.語, self.表層 = {}, {}
        for row in words:
            if type(row) is not 対訳語 or row.種別 not in ("対象", "属性", "条件", "単位"):
                raise _未対応("対訳語の型または役割が不正")
            for x in (row.識別子, row.日本語, row.英語):
                _文字(x, 80)
            if row.識別子 in self.語:
                raise _未対応("対訳語ID重複")
            for lang, surface in (("ja", row.日本語), ("en", row.英語)):
                if any(c in surface for c in '。！？!?"「」;\\\n\r'):
                    raise _未対応("語彙に文境界または引用区切りが含まれる")
                if lang == "ja" and (any(c.isspace() for c in surface) or "、" in surface):
                    raise _未対応("日本語の対訳語に未対応の空白・区切り")
                if lang == "en" and (not surface.isascii() or not re.fullmatch(r"[A-Za-z0-9_%+./-]+(?: [A-Za-z0-9_%+./-]+)*", surface)):
                    raise _未対応("英語の対訳語に未対応の文字")
                key = (lang, row.種別, surface)
                if key in self.表層:
                    raise _未対応("同じ表層に複数の意味があり曖昧")
                self.表層[key] = row.識別子
            self.語[row.識別子] = row

    def パターン(self, kind, lang):
        values = [k[2] for k in self.表層 if k[:2] == (lang, kind)]
        return "(?:" + "|".join(re.escape(s) for s in sorted(values, key=lambda s: (-len(s), s))) + ")" if values else r"(?!)"

    def 表示(self, key, lang):
        row = self.語[key]
        return row.日本語 if lang == "ja" else row.英語

    def 数値文法(self, lang):
        term = lambda kind: self.パターン(kind, lang)
        if lang == "ja":
            return re.compile(
                r'(?:(?P<時点>[0-9]{4}-[0-9]{2}-[0-9]{2})時点、)?'
                + rf'(?:条件「(?P<条件>{term("条件")})」では、)?'
                + rf'(?P<対象>{term("対象")})の(?P<属性>{term("属性")})は'
                + rf'(?P<値>{_数}) (?P<単位>{term("単位")})'
                + r'(?P<比較>以上|以下|未満|超)?(?P<否定>ではない|ではありません|です|である)。?')
        return re.compile(
            r'(?:(?:As of|as of) (?P<時点>[0-9]{4}-[0-9]{2}-[0-9]{2}), )?'
            + rf'(?:(?:Under|under) condition "(?P<条件>{term("条件")})", )?'
            + rf'(?:The|the) (?P<属性>{term("属性")}) of (?P<対象>{term("対象")}) is '
            + r'(?P<否定>not )?(?:(?P<比較>exactly|at least|at most|less than|greater than) )?'
            + rf'(?P<値>{_数}) (?P<単位>{term("単位")})\.?')


def _数値を読む(text, lang, lex):
    match = lex.数値文法(lang).fullmatch(text)
    if match is None:
        raise _未対応("数値記載の未知語・構文・条件・留保")
    fields = match.groupdict()
    if fields["時点"] is not None:
        try:
            date.fromisoformat(fields["時点"])
        except ValueError as exc:
            raise _未対応("不正な日付") from exc
    # 語の切り方が別の語彙組合せでも成立するなら、一つの正規表現解を特権化しない。
    pairs = []
    for left in lex.語.values():
        if left.種別 != "対象":
            continue
        for right in lex.語.values():
            if right.種別 != "属性":
                continue
            a, b = lex.表示(left.識別子, lang), lex.表示(right.識別子, lang)
            needle = a + "の" + b if lang == "ja" else b + " of " + a
            given = fields["対象"] + "の" + fields["属性"] if lang == "ja" else fields["属性"] + " of " + fields["対象"]
            if needle == given:
                pairs.append((left.識別子, right.識別子))
    if len(pairs) != 1:
        raise _未対応("対象と属性の区切りが曖昧")
    op = fields["比較"]
    if lang == "en":
        op = {v: k for k, v in _比較英語.items()}.get(op, "一致")
    else:
        op = op or "一致"
    meaning = {"種別": "数値記載", "対象": pairs[0][0], "属性": pairs[0][1],
               "値": fields["値"], "単位": lex.表層[(lang, "単位", fields["単位"])],
               "比較": op, "否定": fields["否定"] in ("not ", "ではない", "ではありません"),
               "条件": lex.表層[(lang, "条件", fields["条件"])] if fields["条件"] is not None else None,
               "時点": fields["時点"]}
    spans = {key: match.span(key) for key in fields if fields[key] is not None}
    return meaning, spans


def _数値を書く(value, lang, lex):
    obj, attr, unit = (lex.表示(value[k], lang) for k in ("対象", "属性", "単位"))
    when, condition = value["時点"], value["条件"]
    if lang == "ja":
        prefix = (when + "時点、" if when else "")
        prefix += f'条件「{lex.表示(condition, lang)}」では、' if condition else ""
        return (prefix + obj + "の" + attr + "は" + value["値"] + " " + unit
                + _比較日本語[value["比較"]] + ("ではない。" if value["否定"] else "です。"))
    prefix = ("As of " + when + ", " if when else "")
    prefix += f'under condition "{lex.表示(condition, lang)}", ' if condition else ""
    return (prefix + "the " + attr + " of " + obj + " is " + ("not " if value["否定"] else "")
            + _比較英語[value["比較"]] + " " + value["値"] + " " + unit + ".")


_対象英語 = r'(?:the text|the original text|the previous answer|the result|it|document "[^";\r\n]{1,80}")'
_対象日本語 = r'(?:本文|元の本文|前の回答|その結果|それ|資料「[^「」;\r\n]{1,80}」)'


def _依頼を読む(text, lang, lex):
    del lex
    if lang == "en":
        prefix = r"(?:(?:Please|please) )?"
        patterns = (
            ("要約", prefix + rf'(?:Summarize|summarize) (?P<対象>{_対象英語})(?: in (?P<上限>at most |exactly )(?P<行数>[1-8]) lines?)?\.?'),
            ("抽出", prefix + rf'(?:Extract|extract) (?P<抽出>numbers|URLs|keywords) from (?P<対象>{_対象英語})\.?'),
            ("整形", prefix + rf'(?:Format|format) (?P<対象>{_対象英語}) as bullet points\.?'))
    else:
        patterns = (
            ("要約", rf'(?P<対象>{_対象日本語})を(?:(?P<行数>[1-8])行(?P<上限>以内)?で)?要約して。?'),
            ("抽出", rf'(?P<対象>{_対象日本語})から(?P<抽出>数字|URL|キーワード)を抽出して。?'),
            ("整形", rf'(?P<対象>{_対象日本語})を箇条書きにして。?'))
    for 作用, pattern in patterns:
        match = re.fullmatch(pattern, text)
        if match is None:
            continue
        fields = match.groupdict()
        target = fields["対象"]
        英語対象語対応 = {"the text": "本文", "the original text": "元の本文", "the previous answer": "前の回答",
                   "the result": "その結果", "it": "それ"}
        if lang == "en":
            target = 英語対象語対応.get(target, target)
            if target.startswith('document "'):
                target = '資料「' + target[10:-1] + '」'
        if target.startswith("資料「"):
            name = target[3:-1]
            _文字(name, 80)
            if any(c in name for c in ';"「」'):
                raise _未対応("資料名の区切りが不正")
        fields["対象"] = target
        kind = fields.get("抽出")
        if lang == "en" and kind:
            kind = {v: k for k, v in _抽出語.items()}[kind]
        meaning = {"種別": "文書依頼", "操作": 作用, "対象": target, "抽出": kind,
                   "行数": int(fields["行数"]) if fields.get("行数") is not None else None,
                   "行数条件": ("以内" if fields.get("上限") in ("at most ", "以内") else "一致") if 作用 == "要約" and fields.get("行数") is not None else None}
        return meaning, {key: match.span(key) for key, val in match.groupdict().items() if val is not None}
    raise _未対応("文書依頼の未知構文・否定・追加条件")


def _依頼を書く(value, lang, lex):
    del lex
    target, 作用 = value["対象"], value["操作"]
    if lang == "ja":
        if 作用 == "要約":
            if value["行数"] is None:
                return target + "を要約して"
            return f'{target}を{value["行数"]}行' + ("以内" if value["行数条件"] == "以内" else "") + "で要約して"
        if 作用 == "抽出":
            return target + "から" + value["抽出"] + "を抽出して"
        return target + "を箇条書きにして"
    names = {"本文": "the text", "元の本文": "the original text", "前の回答": "the previous answer",
             "その結果": "the result", "それ": "it"}
    target = names.get(target, 'document "' + target[3:-1] + '"')
    if 作用 == "要約":
        if value["行数"] is None:
            return "Summarize " + target + "."
        bound = "at most" if value["行数条件"] == "以内" else "exactly"
        noun = "line" if value["行数"] == 1 else "lines"
        return f'Summarize {target} in {bound} {value["行数"]} {noun}.'
    if 作用 == "抽出":
        return "Extract " + _抽出語[value["抽出"]] + " from " + target + "."
    return "Format " + target + " as bullet points."


def 対訳を変換(本文: str, 入力言語: str, 出力言語: str, *, 種別: str,
               対訳: tuple[対訳語, ...] = (), 最大出力バイト数: int = 32768,
               停止要求: Callable[[], bool] | None = None) -> 能力結果:
    """原文・日本語基底・役割・訳文対応を保持する。未知行があれば全体を保留する。"""
    def stop():
        if 停止要求 is not None:
            value = 停止要求()
            if type(value) is not bool or value:
                raise _未対応("停止要求または停止判定の型不正")
    try:
        stop()
        if type(本文) is not str or not 本文.strip() or len(本文.encode("utf-8")) > 32768:
            raise _未対応("原文は空でない32768バイト以内の文字列")
        if any(unicodedata.category(c) in ("Cf", "Zl", "Zp", "Cs") or (unicodedata.category(c) == "Cc" and c not in '\n\r') for c in 本文):
            raise _未対応("原文に未対応制御文字")
        if 入力言語 not in ("ja", "en") or 出力言語 not in ("ja", "en") or 入力言語 == 出力言語:
            raise _未対応("明示した日英間の異なる言語コードが必要")
        if type(最大出力バイト数) is not int or not 1 <= 最大出力バイト数 <= 131072:
            raise _未対応("出力予算が不正")
        if 種別 not in ("数値記載", "文書依頼"):
            raise _未対応("翻訳対象の種別が未対応")
        lex = _対訳辞書(対訳)
        parser, writer = (_数値を読む, _数値を書く) if 種別 == "数値記載" else (_依頼を読む, _依頼を書く)
        meanings, records, residuals, outputs, canonical = [], [], [], [], []
        offset, position = 0, 0
        lines = 本文.splitlines(keepends=True)
        if len(lines) > 64:
            raise _未対応("行数上限")
        for raw in lines:
            stop()
            text = raw.strip()
            start = offset + len(raw) - len(raw.lstrip())
            offset += len(raw)
            if not text:
                continue
            if len(text) > 2048:
                raise _未対応("一行の長さ上限")
            try:
                meaning, spans = parser(text, 入力言語, lex)
                if 種別 == "文書依頼" and meaning["対象"] == "その結果" and not meanings:
                    raise _未対応("依頼内の前工程がない結果参照")
                japanese = writer(meaning, "ja", lex)
                target = writer(meaning, 出力言語, lex)
                # 表現先の構文を読み直し、保持すべき役割値が同一か確認する。
                after, target_spans = parser(target, 出力言語, lex)
                pivot, _ = parser(japanese, "ja", lex)
                if after != meaning or pivot != meaning:
                    raise _未対応("言語間の役割再構成不一致")
                if outputs:
                    position += 1
                records.append({"原文開始": start, "原文終了": start + len(text), "原文": text,
                    "訳文開始": position, "訳文終了": position + len(target), "訳文": target,
                    "原文役割": {k: [start+a, start+b] for k, (a, b) in spans.items()},
                    "訳文役割": {k: [position+a, position+b] for k, (a, b) in target_spans.items()}})
                position += len(target)
                meanings.append(meaning); outputs.append(target); canonical.append(japanese)
            except _未対応 as exc:
                residuals.append({"開始": start, "終了": start+len(text), "原文": text, "理由": str(exc)})
        if residuals:
            return 能力結果(False, "", 保留理由="未解釈部分があるため全文の対訳を保留",
                データ={"版": 多言語版, "原文": 本文, "残差": residuals, "解釈済み行数": len(meanings)})
        body = "\n".join(outputs)
        if len(body.encode("utf-8")) > 最大出力バイト数:
            return 能力結果(False, "", 保留理由="対訳の出力予算不足。途中切断しない",
                データ={"必要バイト数": len(body.encode()), "指定上限": 最大出力バイト数})
        資料 = {"版": 多言語版, "入力": {"本文": 本文, "入力言語": 入力言語,
                "出力言語": 出力言語, "種別": 種別, "対訳": [asdict(w) for w in 対訳],
                "最大出力バイト数": 最大出力バイト数},
                "日本語基底": "\n".join(canonical), "意味列": meanings, "対応": records,
                "表層変更": "空白・終止・依頼の丁寧表現は規定表層へ構成。原文は別途保持",
                "保証範囲": "指定語彙・有限構文の役割保持。辞書の翻訳妥当性と事実性は未確認",
                "実行権限": 'なし。翻訳結果は資料'}
        raw = _符号({"本文": body, "データ": 資料})
        if len(raw) > 500000:
            raise _未対応("翻訳記録サイズ上限")
        資料["記録SHA256"] = sha256(raw).hexdigest()
        stop()
        return 能力結果(True, body, データ=資料)
    except Exception as exc:
        return 能力結果(False, "", 保留理由="多言語契約・制御不成立:" + type(exc).__name__,
            データ={"診断": str(exc) if isinstance(exc, _未対応) else type(exc).__name__})


def 翻訳記録整合(結果: 能力結果) -> bool:
    '元入力と語彙資料から再構成する。同一実装の再検査であり独立した翻訳評価ではない。'
    try:
        _結果辞書(結果)
        if not 結果.成立 or 結果.保留理由 or 結果.根拠:
            return False
        情報源 = deepcopy(結果.データ["入力"])
        if set(情報源) != {"本文", "入力言語", "出力言語", "種別", "対訳", "最大出力バイト数"}:
            return False
        情報源["対訳"] = tuple(対訳語(**w) for w in 情報源["対訳"])
        expected = 対訳を変換(**情報源)
        return expected.成立 and expected.本文 == 結果.本文 and expected.データ == 結果.データ
    except Exception:
        return False
