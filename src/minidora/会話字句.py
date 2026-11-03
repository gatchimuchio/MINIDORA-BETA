"""固定辞書から原位置付き字句を供給する。引用域・意味役割・採否はCompilerが決める。"""
from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from importlib import metadata as 配布情報
from pathlib import Path
from threading import RLock

from .入力系.契約 import 入力政策


_処理器版 = "0.6.11"
_辞書版 = "20260723"
_辞書印 = "53fa281d11eef3769712fe1c3c892117338f9892bee6daf4dad51daa5281bb6f"
_辞書容量 = 217466039
_辞書先頭 = bytes.fromhex("344439921a019fce")
_入力バイト上限 = 入力政策().最大入力バイト
_字句鍵 = RLock()
_供給状態 = None


class 会話字句不成立(ValueError):
    """取得・型・固定版・原位置の具体的不成立。旧字句への無言の代替は行わない。"""


@dataclass(frozen=True, slots=True)
class 会話字句候補:
    表層: str
    開始: int
    終了: int
    辞書形: str
    品詞: tuple[str, str, str, str, str, str]
    活用型: str
    活用形: str
    未知語: bool
    辞書ID: int
    辞書版: str = _辞書版
    処理器版: str = _処理器版

    def __post_init__(self):
        for 名 in ("表層", "辞書形", "活用型", "活用形", "辞書版", "処理器版"):
            if type(getattr(self, 名)) is not str:
                raise 会話字句不成立("字句文字列型不正:" + 名)
        if not self.表層 or not self.辞書形:
            raise 会話字句不成立("字句表層又は辞書形が空")
        if (type(self.開始) is not int or type(self.終了) is not int
                or not 0 <= self.開始 < self.終了 or self.終了 - self.開始 != len(self.表層)):
            raise 会話字句不成立("字句codepoint範囲不正")
        if (type(self.品詞) is not tuple or len(self.品詞) != 6
                or any(type(項) is not str for 項 in self.品詞)):
            raise 会話字句不成立("字句品詞は文字列6要素tuple")
        if (self.活用型, self.活用形) != self.品詞[4:6]:
            raise 会話字句不成立("字句活用と品詞不一致")
        if type(self.未知語) is not bool or type(self.辞書ID) is not int or self.辞書ID not in (-1, 0):
            raise 会話字句不成立("字句未知語又は固定辞書ID型不正")
        if self.未知語 != (self.辞書ID == -1):
            raise 会話字句不成立("字句未知語と辞書ID不一致")
        if self.辞書版 != _辞書版 or self.処理器版 != _処理器版:
            raise 会話字句不成立("字句固定版不一致")


@dataclass(frozen=True, slots=True)
class _字句供給状態:
    辞書: object
    解析器: object
    辞書経路: Path
    ファイル状態: tuple[int, int, int, int]


def _ファイル状態(経路):
    記録 = 経路.stat()
    return 記録.st_size, 記録.st_mtime_ns, 記録.st_dev, 記録.st_ino


def _固定辞書を検査(経路):
    前 = _ファイル状態(経路)
    if 前[0] != _辞書容量:
        raise 会話字句不成立("固定辞書容量不一致")
    印 = sha256()
    with 経路.open("rb") as 資料:
        先頭 = 資料.read(8)
        if 先頭 != _辞書先頭:
            raise 会話字句不成立("固定辞書V0ヘッダー不一致")
        印.update(先頭)
        for 塊 in iter(lambda: 資料.read(1024 * 1024), b""):
            印.update(塊)
    if 印.hexdigest() != _辞書印 or _ファイル状態(経路) != 前:
        raise 会話字句不成立("固定辞書SHA256又は読取中状態不一致")
    return 前


def _配布版を検査():
    for 名, 版 in (("SudachiPy", _処理器版), ("SudachiDict-core", _辞書版)):
        try:
            実版 = 配布情報.version(名)
        except 配布情報.PackageNotFoundError as 例外:
            raise 会話字句不成立("字句必須配布が未導入:" + 名) from 例外
        if 実版 != 版:
            raise 会話字句不成立("字句固定配布版不一致:" + 名)


def _字句を初期化():
    _配布版を検査()
    経路 = Path(配布情報.distribution("SudachiDict-core").locate_file("sudachidict_core/resources/system.dic")).resolve()
    状態 = _固定辞書を検査(経路)
    from sudachipy import dictionary, tokenizer
    辞書 = dictionary.Dictionary(dict=str(経路))
    解析器 = 辞書.create(mode=tokenizer.Tokenizer.SplitMode.C, projection="surface")
    if _ファイル状態(経路) != 状態:
        辞書.close()
        raise 会話字句不成立("固定辞書が初期化中に変化")
    return _字句供給状態(辞書, 解析器, 経路, 状態)


def 会話字句を観測(本文: str) -> tuple[会話字句候補, ...]:
    """原文codepoint全覆の不変字句列。正規形で原文・識別名を置換しない。"""
    if type(本文) is not str:
        raise 会話字句不成立("字句入力はexact文字列")
    # UTF-8は1codepointあたり1byte以上。巨大文字列の符号化前にも同じ上限で拒否する。
    if len(本文) > _入力バイト上限:
        raise 会話字句不成立("字句入力文字数が原入力容量上限を超過")
    try:
        容量 = len(本文.encode("utf-8", errors="strict"))
    except UnicodeEncodeError as 例外:
        raise 会話字句不成立("字句入力がUTF-8へ厳密符号化できない") from 例外
    if 容量 > _入力バイト上限:
        raise 会話字句不成立("字句入力UTF-8容量が原入力容量上限を超過")
    if not 本文:
        return ()
    global _供給状態
    with _字句鍵:
        try:
            if _供給状態 is None:
                _供給状態 = _字句を初期化()
            _配布版を検査()
            if _ファイル状態(_供給状態.辞書経路) != _供給状態.ファイル状態:
                raise 会話字句不成立("固定辞書ファイル状態が初期化後に変化")
        except 会話字句不成立:
            raise
        except Exception as 例外:
            raise 会話字句不成立("字句初期化不成立:" + type(例外).__name__) from 例外
        try:
            列 = []
            終端 = 0
            for 形 in _供給状態.解析器.tokenize(本文):
                始, 終 = 形.begin(), 形.end()
                表層 = 形.surface()
                品詞 = 形.part_of_speech()
                if (type(始) is not int or type(終) is not int or not 終端 == 始 < 終 <= len(本文)
                        or type(表層) is not str or 表層 != 本文[始:終]):
                    raise 会話字句不成立("字句原slice又は隣接全覆不一致")
                if type(品詞) is not tuple or len(品詞) != 6:
                    raise 会話字句不成立("字句品詞6要素tuple不成立")
                列.append(会話字句候補(表層, 始, 終, 形.dictionary_form(), 品詞,
                                    品詞[4], 品詞[5], 形.is_oov(), 形.dictionary_id()))
                終端 = 終
            if 終端 != len(本文):
                raise 会話字句不成立("字句原文末尾が未供給")
            if _ファイル状態(_供給状態.辞書経路) != _供給状態.ファイル状態:
                raise 会話字句不成立("固定辞書ファイル状態が解析中に変化")
            return tuple(列)
        except 会話字句不成立:
            raise
        except Exception as 例外:
            raise 会話字句不成立("字句解析不成立:" + type(例外).__name__) from 例外
