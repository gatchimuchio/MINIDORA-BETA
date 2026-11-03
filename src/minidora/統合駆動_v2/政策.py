"""資源・権限・再照合の政策。認識の真偽や模型成立条件には使わない。"""
from __future__ import annotations
from dataclasses import dataclass
from enum import StrEnum
from .値 import 文字, 文字列組, 整数


class 停止理由(StrEnum):
    目的達成 = "目的達成"
    知識不足 = "知識不足"
    観測不足 = "観測不足"
    証拠競合 = "証拠競合"
    作用不足 = "作用不足"
    予算枯渇 = "予算枯渇"
    方針制約 = "方針制約"
    権限制約 = "権限制約"
    外部作用失敗 = "外部作用失敗"
    依存未閉包 = "依存未閉包"
    検証不成立 = "検証不成立"
    契約違反 = "契約違反"
    未知失敗 = "未知失敗"
    無進展 = "無進展"
    明示停止 = "明示停止"


@dataclass(frozen=True, slots=True)
class HDS阻害:
    種別: 停止理由
    対象: str
    詳細: str
    修復可能: bool = False
    必要状態: tuple[str, ...] = ()
    必要認識: tuple[str, ...] = ()

    def __post_init__(self):
        if not isinstance(self.種別, 停止理由):
            raise TypeError("停止理由型が必要")
        文字(self.対象)
        文字(self.詳細)
        if type(self.修復可能) is not bool:
            raise TypeError("修復可能はbool")
        文字列組(self.必要状態)
        文字列組(self.必要認識)
        if self.種別 in (停止理由.方針制約, 停止理由.権限制約) and self.修復可能:
            raise ValueError("方針/権限制約を修復対象として迂回できない")


class HDS作用失敗(RuntimeError):
    def __init__(self, 阻害: HDS阻害):
        if not isinstance(阻害, HDS阻害):
            raise TypeError("構造化された阻害が必要")
        self.阻害 = 阻害
        super().__init__(阻害.詳細)


@dataclass(frozen=True, slots=True)
class HDS運用政策:
    最大資源: int = 4096
    初期作用予算: int = 16
    予算増分: int = 16
    大域間隔: int = 3
    探索深さ: int = 12
    最大探索状態: int = 2048
    最大内部生成: int = 128
    最大観測要求: int = 128
    参照再利用回数: int = 3
    禁止作用: tuple[str, ...] = ()
    許可権限: tuple[str, ...] = ()
    圧縮開始文字数: int = 1024
    圧縮最大文字数: int = 512
    自動形成: bool = True
    版: str = "HDS運用政策-v3"
    最大機会検討: int = 8192

    def __post_init__(self):
        for n in ("最大資源", "初期作用予算", "予算増分", "大域間隔", "探索深さ", "最大探索状態", "最大内部生成", "最大観測要求", "参照再利用回数", "圧縮開始文字数", "圧縮最大文字数"):
            整数(getattr(self, n), n, 1)
        整数(self.最大機会検討, "最大機会検討", 1)
        整数(self.圧縮最大文字数, "圧縮最大文字数", 32)
        if type(self.自動形成) is not bool:
            raise TypeError("自動形成はbool")
        文字列組(self.禁止作用)
        文字列組(self.許可権限)
        文字(self.版)

    def 許可判定(self, 作用ID: str, 必要権限: tuple[str, ...] = ()) -> 停止理由 | None:
        if 作用ID in self.禁止作用:
            return 停止理由.方針制約
        if not set(必要権限).issubset(self.許可権限):
            return 停止理由.権限制約
        return None


@dataclass(frozen=True, slots=True)
class HDS計装:
    作用実行数: int = 0
    状態差発生数: int = 0
    後続消費数: int = 0
    依存失効数: int = 0
    観測実行数: int = 0
    動的計画数: int = 0
    学習計画補正数: int = 0
    修復選択数: int = 0
    大域再照合数: int = 0
    予算拡張数: int = 0
    検証失敗数: int = 0
    消費資源: int = 0
    探索状態数: int = 0
    制約除外数: int = 0
    生成件数: int = 0
    形成再利用数: int = 0
    外部入力数: int = 0
    自動形成数: int = 0
    形成再検証数: int = 0
    再現作用数: int = 0
    意味構成数: int = 0
    仮説生成数: int = 0
    枝生成数: int = 0
    圧縮数: int = 0
    未来監査数: int = 0
    失敗診断数: int = 0
    目的進展数: int = 0
    目的無進展数: int = 0

    供給検討数: int = 0
    供給時間ns: int = 0
    探索実行数: int = 0
    探索予算拡張数: int = 0
    探索上限除外数: int = 0
    機会検討数: int = 0
    機会時間ns: int = 0
    構成前除外数: int = 0
    目的関連除外数: int = 0
    無進展再試行除外数: int = 0
    計画時間ns: int = 0
    作用時間ns: int = 0
    射影時間ns: int = 0

