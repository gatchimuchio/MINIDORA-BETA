"""原文位置と型を保持してJSON・CSVを読む。ファイルI/Oや意味推定は行わない。"""
from __future__ import annotations

import json
import re

from .製品版.型 import 能力結果

構造化文書版 = "MINIDORA-構造化文書-v0.1"
_数値 = re.compile(r'-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?')
_最大原文 = 262144
_最大節点 = 8192


class 文書境界違反(ValueError):
    def __init__(self, 理由: str, 位置: int | None = None):
        super().__init__(理由)
        self.位置 = 位置


def _文字(value: str) -> None:
    if type(value) is not str:
        raise 文書境界違反("文字列が必要")
    value.encode("utf-8")  # 孤立サロゲートを受理しない。


def _原文検査(text: str) -> None:
    _文字(text)
    if not text or len(text.encode("utf-8")) > _最大原文:
        raise 文書境界違反("原文の空入力・サイズ上限")
    if text.startswith("\ufeff"):
        raise 文書境界違反("BOM付き入力は未対応。文字コードを上位で明示する", 0)


def _節(種類: str, 値):
    return {"型": 種類, "値": 値}


def _位置名(name: str) -> str:
    return name.replace("~", "~0").replace("/", "~1")


def _数値検査(字句: str) -> None:
    if type(字句) is not str or not _数値.fullmatch(字句) or len(字句) > 256:
        raise 文書境界違反("数値表記の型・長さ不正")
    if re.search('[eE]', 字句):
        exponent = re.split('[eE]', 字句)[1]
        if len(exponent.lstrip('+-')) > 4 or abs(int(exponent)) > 1000:
            raise 文書境界違反("数値指数上限")


class _JSON読取:
    def __init__(self, text: str):
        self.text, self.pos, self.count = text, 0, 0
        self.spans, self.keys = {}, {}
        self.decoder = json.JSONDecoder()

    def skip(self):
        while self.pos < len(self.text) and self.text[self.pos] in ' \t\r\n':
            self.pos += 1

    def string(self):
        start = self.pos
        if self.pos >= len(self.text) or self.text[self.pos] != '"':
            raise 文書境界違反("JSON文字列が必要", self.pos)
        try:
            value, end = self.decoder.raw_decode(self.text, self.pos)
        except json.JSONDecodeError as exc:
            raise 文書境界違反("JSON文字列の構文不正", exc.pos) from exc
        _文字(value)
        if len(value) > 32768:
            raise 文書境界違反("文字列値の長さ上限", start)
        self.pos = end
        return value, start, end

    def node(self, pointer="", depth=0):
        self.count += 1
        if self.count > _最大節点 or depth > 12:
            raise 文書境界違反("JSON節点数・深さ上限", self.pos)
        self.skip()
        start = self.pos
        if start >= len(self.text):
            raise 文書境界違反("JSON値が欠落", start)
        char = self.text[self.pos]
        if char in '{[':
            self.pos += 1
            obj = char == '{'
            end = '}' if obj else ']'
            values = {} if obj else []
            self.skip()
            if self.pos < len(self.text) and self.text[self.pos] == end:
                self.pos += 1
            else:
                while True:
                    self.skip()
                    if obj:
                        key, a, b = self.string()
                        if key in values:
                            raise 文書境界違反("重複JSONキーを上書きしない", a)
                        sub = pointer + '/' + _位置名(key)
                        self.keys[sub] = {"開始": a, "終了": b}
                        self.skip()
                        if self.pos >= len(self.text) or self.text[self.pos] != ':':
                            raise 文書境界違反("JSONのコロン欠落", self.pos)
                        self.pos += 1
                        values[key] = self.node(sub, depth+1)
                    else:
                        sub = pointer + '/' + str(len(values))
                        values.append(self.node(sub, depth+1))
                    self.skip()
                    if self.pos >= len(self.text):
                        raise 文書境界違反("JSON閉じ括弧欠落", self.pos)
                    if self.text[self.pos] == end:
                        self.pos += 1
                        break
                    if self.text[self.pos] != ',':
                        raise 文書境界違反("JSON要素区切り不正", self.pos)
                    self.pos += 1
            node = _節("対象" if obj else "配列", values)
        elif char == '"':
            node = _節("文字列", self.string()[0])
        else:
            node = None
            for literal, kind, value in (("true", "真偽", True), ("false", "真偽", False), ("null", "空値", None)):
                if self.text.startswith(literal, self.pos):
                    self.pos += len(literal)
                    node = _節(kind, value)
                    break
            if node is None:
                match = _数値.match(self.text, self.pos)
                if match is None:
                    raise 文書境界違反("未対応または不正なJSON値", self.pos)
                字句 = match[0]
                _数値検査(字句)
                self.pos = match.end()
                node = _節("数値", 字句)
        self.spans[pointer] = {"原位置": pointer, "開始": start, "終了": self.pos}
        return node

    def read(self):
        root = self.node()
        self.skip()
        if self.pos != len(self.text):
            raise 文書境界違反("JSON末尾を読み飛ばさない", self.pos)
        return {"形式": "JSON", "構造": root, "対応": self.spans, "キー対応": self.keys,
                "列名": None, "見出し対応": [], "区切り": None}


def _CSV読取(text: str, delimiter: str, header: bool):
    if delimiter not in (',', '\t') or type(header) is not bool:
        raise 文書境界違反("CSV区切りはカンマかタブ、見出しはbool")
    rows, cells, row_spans = [], [], []
    pos, total = 0, 0
    while pos < len(text):
        start_row, values, spans = pos, [], []
        while True:
            start = pos
            if pos < len(text) and text[pos] == '"':
                pos += 1
                parts = []
                while True:
                    if pos >= len(text):
                        raise 文書境界違反("CSV引用符が閉じていない", start)
                    if text[pos] == '"':
                        pos += 1
                        if pos < len(text) and text[pos] == '"':
                            parts.append('"'); pos += 1
                            continue
                        break
                    parts.append(text[pos]); pos += 1
                value = ''.join(parts)
                if pos < len(text) and text[pos] not in (delimiter, '\r', '\n'):
                    raise 文書境界違反("CSV引用閉じ後の不正文字", pos)
            else:
                while pos < len(text) and text[pos] not in (delimiter, '\r', '\n'):
                    if text[pos] == '"':
                        raise 文書境界違反("CSV非引用セル内の引用符", pos)
                    pos += 1
                value = text[start:pos]
            if len(value) > 32768:
                raise 文書境界違反("CSVセル長上限", start)
            total += 1
            if total > 4096:
                raise 文書境界違反("CSV総セル数上限", start)
            values.append(value)
            spans.append({"開始": start, "終了": pos})
            if len(values) > 64:
                raise 文書境界違反("CSV列数上限", pos)
            if pos == len(text):
                break
            if text[pos] == delimiter:
                pos += 1
                continue
            if text[pos] == '\r':
                if text[pos:pos+2] != '\r\n':
                    raise 文書境界違反("CSV行区切りはLFまたはCRLF", pos)
                pos += 2
            else:
                pos += 1
            break
        else:
            raise AssertionError("到達しない")
        rows.append(values); cells.append(spans)
        row_spans.append({"開始": start_row, "終了": spans[-1]["終了"]})
        if len(rows) > 2049:
            raise 文書境界違反("CSV行数上限", pos)
    width = len(rows[0])
    if any(len(row) != width for row in rows):
        raise 文書境界違反("CSV列数不一致。空欄補完や余剰列破棄をしない")
    names, header_spans = None, []
    if header:
        names, header_spans = rows.pop(0), cells.pop(0)
        row_spans.pop(0)
        if any(not name for name in names) or len(set(names)) != len(names):
            raise 文書境界違反("CSV見出しの空値・重複")
    table = _節("配列", [_節("配列", [_節("文字列", v) for v in row]) for row in rows])
    positions = {"": {"原位置": "", "開始": 0, "終了": len(text)}}
    for i, (row, spans) in enumerate(zip(rows, cells)):
        pointer = '/' + str(i)
        positions[pointer] = {"原位置": pointer, **row_spans[i]}
        for j, span in enumerate(spans):
            cell = pointer + '/' + str(j)
            positions[cell] = {"原位置": cell, **span}
    return {"形式": "CSV", "構造": table, "対応": positions, "キー対応": {},
            "列名": names, "見出し対応": header_spans, "区切り": delimiter}


def _読む(text: str, format_: str, delimiter: str = ',', header: bool = True):
    _原文検査(text)
    if format_ == "JSON":
        if delimiter != ',' or header is not True:
            raise 文書境界違反("JSON入力にCSV設定を適用しない")
        return _JSON読取(text).read()
    if format_ == "CSV":
        return _CSV読取(text, delimiter, header)
    raise 文書境界違反("形式はJSONまたはCSVを明示する")


def _JSON出力(node: dict) -> str:
    kind, value = node['型'], node['値']
    if kind == '数値':
        _数値検査(value)
        return value
    if kind == '対象':
        return '{' + ','.join(json.dumps(k, ensure_ascii=False) + ':' + _JSON出力(v) for k, v in value.items()) + '}'
    if kind == '配列':
        return '[' + ','.join(_JSON出力(v) for v in value) + ']'
    return json.dumps(value, ensure_ascii=False, allow_nan=False)


def _失敗(exc: Exception) -> 能力結果:
    return 能力結果(False, '', 保留理由='文書処理不成立:' + type(exc).__name__,
        データ={"診断": str(exc) if isinstance(exc, 文書境界違反) else type(exc).__name__,
                "原文位置": getattr(exc, '位置', None)})


def 構造化文書を読む(原文: str, 形式: str, *, 区切り: str = ',', 見出し: bool = True) -> 能力結果:
    from .構造化文書操作 import _結果
    try:
        情報源 = {"原文": 原文, "形式": 形式, "区切り": 区切り, "見出し": 見出し}
        doc = _読む(原文, 形式, 区切り, 見出し)
        return _結果(情報源, [], doc, 原文)
    except (ValueError, TypeError, KeyError, RecursionError, OverflowError) as exc:
        return _失敗(exc)
