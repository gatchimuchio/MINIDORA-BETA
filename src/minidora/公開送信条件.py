"""公開検索の送信前条件と安全な監査表現。検索の必要性・回答の意味は決めない。"""
from __future__ import annotations

from dataclasses import asdict, is_dataclass
import re
import unicodedata

公開送信条件版 = "公開送信条件-v1"
安全監査表現版 = "安全監査表現-v1"

# 形式として観測できる値だけを扱う。任意の私的意味・独自鍵形式の不在は保証しない。
_形式 = (
    ("メール", re.compile(r"[A-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Z0-9-]+(?:\.[A-Z0-9-]+)+", re.I)),
    ("電話", re.compile(r"(?<!\d)(?:\+81[ -]?|0)(?:\d[ -]?){8,11}\d(?!\d)")),
    ("認証値", re.compile(r"\bBearer\s+[A-Za-z0-9._~+/=-]+", re.I)),
    ("認証値", re.compile(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b")),
    ("認証値", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)")),
    ("秘密値", re.compile(r"(?:パスワード|暗証番号|秘密鍵|APIキー|API[_ -]?KEY|ACCESS[_ -]?TOKEN|認証トークン)\s*(?:は|が|=|:|：)\s*(?:[「\"]([^」\"]+)[」\"]|([^\s、。;；]+))", re.I)),
    ("私的値", re.compile(r"(?:私の氏名|氏名|私の住所|住所|非公開情報|機密情報|秘密情報)\s*(?:は|が|=|:|：)\s*(?:[「\"]([^」\"]+)[」\"]|([^\n、。;；]+))")),
    ("秘匿URL", re.compile(r"https?://[^\s<>「」\"]*(?:[?&](?:key|token|password|secret|signature|credential)=)[^\s<>「」\"]*", re.I)),
)


def _照合値(値):
    return unicodedata.normalize("NFKC", 値).casefold()


def _値を観測(本文, *, 人名=False, ラベル=True):
    if type(本文) is not str:
        raise TypeError("公開送信検査の本文は文字列")
    正規 = unicodedata.normalize("NFKC", 本文)
    群 = []
    for 分類, 式 in _形式:
        if not ラベル and 分類 in ("秘密値", "私的値"):
            continue
        for 一致 in 式.finditer(正規):
            値 = next((v for v in 一致.groups() if v), 一致[0])
            群.append((分類, _照合値(値)))
            if 分類 in ("秘密値", "私的値") and not 一致.group(1):
                # 無引用の機械値の直後に付く日本語の説明語尾を値へ混入させない。
                # 全観測値も残すので、原文の値を短い値へ置き換えない。
                機械値 = re.match(r"[A-Za-z0-9._~+/=-]{4,}", 値)
                if 機械値:
                    群.append((分類, _照合値(機械値[0])))
                語尾なし = re.sub(r"(?:です|である|だ)$", "", 値)
                if len(語尾なし) >= 4:
                    群.append((分類, _照合値(語尾なし)))
    if 人名 and 本文:
        from .会話字句 import 会話字句を観測
        # 固定辞書の人名候補は公開人物である証明ではない。未知名を推測しない。
        for 項 in 会話字句を観測(本文):
            if 項.品詞[:3] == ("名詞", "固有名詞", "人名"):
                群.append(("人名候補", _照合値(項.表層)))
    return tuple(群)


class 公開送信停止(ValueError):
    def __init__(self, 分類):
        許可 = {名 for 名, _ in _形式} | {"人名候補", "文脈未束縛"}
        if type(分類) not in (tuple, list, set) or not set(分類) <= 許可:
            raise TypeError("公開送信停止の分類不正")
        self.分類 = tuple(sorted(set(分類)))
        # 自由文・原検索語・値のhashを例外や公開記録へ含めない。
        super().__init__("公開送信条件不成立")

    def 記録(self):
        return {"版": 公開送信条件版, "状態": "送信停止", "分類": list(self.分類)}


class 公開送信条件:
    """実会話で観測した保護値を当回だけ保持。意味保存物へは組み込まない。"""

    def __init__(self, 原文群=(), *, 人名=True):
        if type(原文群) is not tuple or any(type(v) is not str for v in 原文群):
            raise TypeError("公開送信条件の原文群は文字列tuple")
        self._保護値 = tuple(dict.fromkeys(v for 原文 in 原文群 for v in _値を観測(原文, 人名=人名)))

    def _保護分類(self, 本文):
        正規 = _照合値(本文)
        密 = re.sub(r"\s+", "", 正規)
        return {名 for 名, 値 in self._保護値 if 値 and
                (値 in 正規 or len(値) >= 4 and re.sub(r"\s+", "", 値) in 密)}

    def 検査(self, *語群):
        分類 = set()
        for 語 in 語群:
            観測 = _値を観測(語, 人名=True)
            分類.update(名 for 名, _ in 観測)
            # HDSが必要語へ分割した既観測値も検査する。原値は変換しない。
            分類.update(self._保護分類(語))
        分類.update(self._保護分類("".join(語群)))
        if 分類:
            raise 公開送信停止(分類)

    def 安全表現(self, 値):
        """新しい公開表現を作る。内部意味・旧監査原本を無言変換しない。"""
        if is_dataclass(値) and not isinstance(値, type):
            return self.安全表現(asdict(値))
        if isinstance(値, dict):
            def 葉(項):
                if isinstance(項, dict):
                    return [v for 子 in 項.values() for v in 葉(子)]
                if isinstance(項, (tuple, list, set)):
                    return [v for 子 in 項 for v in 葉(子)]
                return [str(項)] if type(項) in (str, int, float) else []
            # 個別値では見えなくても、辞書の葉を連結して既観測値を復元できる場合。
            群 = [v for v in 葉(値) if not self._保護分類(v)]
            if self._保護分類("".join(群)):
                return {"版": 安全監査表現版, "状態": "分割された非公開値を含む構造を遮蔽"}
            出力 = {}
            for 順, (k, v) in enumerate(値.items()):
                鍵 = self.安全表現(str(k))
                if 鍵 == "[非公開値を含む文字列]":
                    鍵 = "非公開項目:" + str(順)
                if 鍵 in 出力:
                    raise ValueError("安全監査表現の鍵衝突")
                出力[鍵] = self.安全表現(v)
            return 出力
        if isinstance(値, (tuple, list, set)):
            if self._保護分類("".join(v for v in 値 if type(v) is str)):
                return ["[非公開値を含む文字列]" if type(v) is str else self.安全表現(v) for v in 値]
            return [self.安全表現(v) for v in 値]
        if type(値) is str:
            # 全文置換なら原位置・hash・機密値の一部を露出しない。
            # 公開説明の「パスワードは…」を新しい秘密値として推測しない。
            if _値を観測(値, ラベル=False) or self._保護分類(値):
                return "[非公開値を含む文字列]"
        if type(値) in (int, float) and self._保護分類(str(値)):
            return "[非公開値を含む数値]"
        return 値


def 公開語を検査(検索語, 必要語=(), 条件=None):
    if 条件 is not None and not isinstance(条件, 公開送信条件):
        raise TypeError("公開送信条件の型不正")
    (条件 if 条件 is not None else 公開送信条件()).検査(検索語, *必要語)
