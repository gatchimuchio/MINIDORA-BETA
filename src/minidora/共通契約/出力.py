"""駆動が決めた内容と、その表現・送達を交換する不変契約。可変実行主体を持たない。"""
from __future__ import annotations
from dataclasses import dataclass, field
from hashlib import sha256
from typing import Callable
from .封緘 import 封緘値, 封緘する
from ..コア.値 import 文字, 整数, 文字列組


@dataclass(frozen=True, slots=True)
class 表現契約:
    形式: str = 'テキスト'
    言語: str = 'ja'
    詳細: bool = True
    最大文字数: int = 100000
    最大バイト: int = 1000000
    必須内容: tuple[str, ...] = ()
    表現器ID: str = '標準'
    表現器版: str = 'v1'
    版: str = '出力表現-v1'

    def __post_init__(self):
        for 名 in ('形式', '言語', '表現器ID', '表現器版', '版'):
            文字(getattr(self, 名), 名)
        if type(self.詳細) is not bool:
            raise TypeError('詳細指定はbool')
        整数(self.最大文字数, '表現文字上限', 1, 1_000_000)
        整数(self.最大バイト, '表現バイト上限', 1, 16_000_000)
        文字列組(self.必須内容, '必須表現')

    @property
    def 署名(self):
        return 封緘する(self).署名


@dataclass(frozen=True, slots=True)
class 出力政策:
    最大受付件数: int = 1024
    最大保持バイト: int = 64_000_000
    最大内容バイト: int = 16_000_000
    最大内容要素: int = 200000
    最大送達件数: int = 2048
    最大送達試行: int = 3

    def __post_init__(self):
        for 名 in ('最大受付件数', '最大保持バイト', '最大内容バイト', '最大内容要素', '最大送達件数', '最大送達試行'):
            整数(getattr(self, 名), 名, 1, 256_000_000 if 'バイト' in 名 else 1_000_000)
        if self.最大送達試行 > 16:
            raise ValueError('送達試行上限は16')


@dataclass(frozen=True, slots=True)
class 出力束:
    案件ID: str
    出力ID: str
    判断版: int
    目的署名: str
    結果署名: str
    状態: str
    内容種別: str
    内容: 封緘値
    表現: 表現契約 = 表現契約()
    内容言語: str = 'ja'
    入力署名: str = ''
    理由: tuple[str, ...] = ()
    根拠: tuple[str, ...] = ()
    条件: tuple[str, ...] = ()
    留保: tuple[str, ...] = ()
    保証: tuple[str, ...] = ()
    未充足: tuple[str, ...] = ()
    版: str = 'MINIDORA-出力束-v1'

    def __post_init__(self):
        for 名 in ('案件ID', '出力ID', '目的署名', '結果署名', '内容言語', '版'):
            文字(getattr(self, 名), 名)
        整数(self.判断版, '判断版', 1)
        if self.状態 not in ('成立', '保留', '失敗', '停止', '予算枯渇', '観測待ち', '競合'):
            raise ValueError('出力状態が未定義')
        if self.内容種別 not in ('内容計画', '値', '関係', '構造', '診断'):
            raise ValueError('出力内容の種類が未定義')
        if not isinstance(self.内容, 封緘値) or not isinstance(self.表現, 表現契約):
            raise TypeError('封緘内容と表現契約が必要')
        if not isinstance(self.入力署名, str):
            raise TypeError('入力署名は文字列')
        for 名 in ('理由', '根拠', '条件', '留保', '保証', '未充足'):
            文字列組(getattr(self, 名), 名, 一意=False)
        if self.状態 != '成立' and self.内容種別 not in ('診断', '関係', '構造'):
            raise ValueError('未採用の値・内容計画を回答として出力できない')
        if self.状態 == '成立' and self.内容種別 == '診断':
            raise ValueError('診断だけの出力は内容成立ではない')

    @property
    def 鍵(self):
        return self.案件ID, self.出力ID, self.判断版

    @property
    def 内容署名(self):
        # 表現形式だけが変わっても判断内容の同一性は保持する。
        return 封緘する((self.鍵, self.目的署名, self.結果署名, self.状態, self.内容種別,
            self.内容.署名, self.内容言語, self.入力署名, self.理由, self.根拠, self.条件,
            self.留保, self.保証, self.未充足, self.版)).署名

    @property
    def 署名(self):
        return 封緘する((self.内容署名, self.表現.署名)).署名

    def 内容を読む(self):
        return self.内容.復元()


@dataclass(frozen=True, slots=True)
class 出力取得結果:
    束: 出力束
    保持バイト: int
    操作履歴: tuple[str, ...] = ('取得',)


@dataclass(frozen=True, slots=True)
class 表現結果:
    出力署名: str
    内容署名: str
    契約署名: str
    本文: str
    媒体型: str
    合格: bool
    診断: tuple[str, ...] = ()
    操作履歴: tuple[str, ...] = ('取得', '変換')

    def __post_init__(self):
        for 名 in ('出力署名', '内容署名', '契約署名', '媒体型'):
            文字(getattr(self, 名), 名)
        if type(self.本文) is not str or type(self.合格) is not bool:
            raise TypeError('表現本文はstr、合格はbool')
        self.本文.encode('utf-8', errors='strict')
        文字列組(self.診断, '表現診断', 一意=False)
        if self.合格 and (not self.本文 or self.診断):
            raise ValueError('合格表現には非空本文が必要で、未解決診断を残せない')
        if not self.合格 and self.本文:
            raise ValueError('不成立の表現本文を送達可能な結果に残さない')

    @property
    def バイト列(self):
        return self.本文.encode('utf-8')

    @property
    def 本文署名(self):
        return sha256(self.バイト列).hexdigest()

    @property
    def 署名(self):
        return 封緘する(self).署名


@dataclass(frozen=True, slots=True)
class 表現器:
    ID: str
    変換: Callable = field(repr=False, compare=False)
    検証: Callable = field(repr=False, compare=False)
    形式: tuple[str, ...] = ('テキスト',)
    言語: tuple[str, ...] = ('ja',)
    媒体型: str = 'text/plain; charset=utf-8'
    版: str = 'v1'

    def __post_init__(self):
        for 名 in ('ID', '媒体型', '版'):
            文字(getattr(self, 名), 名)
        文字列組(self.形式, '表現形式'); 文字列組(self.言語, '表現言語')
        if not self.形式 or not self.言語 or not callable(self.変換) or not callable(self.検証):
            raise ValueError('表現器には明示された対応形式と言語、変換・検証が必要')


@dataclass(frozen=True, slots=True)
class 送達要求:
    送達ID: str
    案件ID: str
    出力ID: str
    判断版: int
    表現署名: str
    本文署名: str
    本文: bytes
    媒体型: str


@dataclass(frozen=True, slots=True)
class 受領票:
    送達ID: str
    本文署名: str
    状態: str
    理由: str = ''

    def __post_init__(self):
        文字(self.送達ID); 文字(self.本文署名)
        if self.状態 not in ('受領', '未受理'):
            raise ValueError('受領票は受領・未受理のいずれか。曖昧な結果を成功にしない')
        if not isinstance(self.理由, str):
            raise TypeError('受領理由は文字列')


@dataclass(frozen=True, slots=True)
class 送達器:
    ID: str
    送る: Callable = field(repr=False, compare=False)
    重複排除対応: bool = False
    版: str = 'v1'

    def __post_init__(self):
        文字(self.ID); 文字(self.版)
        if not callable(self.送る) or type(self.重複排除対応) is not bool:
            raise TypeError('送達関数と重複排除契約が必要')


@dataclass(frozen=True, slots=True)
class 送達記録:
    送達ID: str
    表現署名: str
    送達先: str
    状態: str
    試行回数: int
    理由: tuple[str, ...] = ()

    def __post_init__(self):
        if self.状態 not in ('送達済み', '未受理', '結果不明', '停止', '表現不成立', '失効', '試行上限'):
            raise ValueError('送達状態が未定義')
        整数(self.試行回数, '送達試行', 0)

    @property
    def 返却成立(self):
        return self.状態 == '送達済み'


@dataclass(frozen=True, slots=True)
class 出力返却:
    表現: 表現結果
    送達: 送達記録
    内容成立: bool

    @property
    def 返却成立(self):
        return self.送達.返却成立

    @property
    def 完了(self):
        return self.内容成立 and self.返却成立

    @property
    def 本文(self):
        return self.表現.本文
