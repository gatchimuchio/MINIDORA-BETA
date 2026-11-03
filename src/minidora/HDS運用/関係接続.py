"""関係読解・不足取得・説明を、既存HDSの能力工程へ接続する。"""
from __future__ import annotations
from ..能力合成 import 登録能力, _参照結合
from ..能力結果復元 import 能力結果を復元
from ..製品版.型 import 能力結果
from ..製品版.能力契約 import 能力文脈
from ..取得実行制御 import 制御を配送する
from .値 import 結果を保存, 結果を復元
from .関係読解 import 関係資料を読む, 関係資料を検査, 不足調査が必要, 関係調査要求
from .関係内容 import 関係内容を構成, 関係回答を照合, 関係回答を再表現, 主張内容不足, 資料条件変更内容不足

関係接続版 = "HDS関係接続-v4"
関係能力名 = ("資料関係読解", "関係不足資料取得", "取得関係読解", "関係内容構成", "関係文章照合", "関係文章再表現")
_個数 = dict(zip(関係能力名, (None, 1, 1, 1, 2, 1)))


class 関係資料能力:
    版 = 関係接続版
    優先度 = 0

    def __init__(self, 名前, 取得器=None, *, 外部許可=False):
        if 名前 not in _個数 or type(外部許可) is not bool:
            raise ValueError("関係能力名・外部許可が不正")
        self.名前, self.取得器, self.外部許可 = 名前, 取得器, 外部許可

    def _入力(self, 文脈):
        if not isinstance(文脈, 能力文脈) or type(文脈.補助) is not dict:
            raise ValueError("関係能力の構造化入力が必要")
        群, 設定 = 文脈.補助.get("合成入力"), 文脈.補助.get("合成設定", {})
        if type(群) not in (list, tuple) or not 1 <= len(群) <= 8 or type(設定) is not dict:
            raise ValueError("関係能力の入力数・設定型不正")
        if _個数[self.名前] is not None and len(群) != _個数[self.名前]:
            raise ValueError("関係能力の入力数不一致")
        if self.名前 not in ("資料関係読解", "関係文章再表現") and 設定:
            raise ValueError("関係能力の未知設定")
        値 = tuple(能力結果を復元(行["結果"]) for 行 in 群)
        if any(not 項.成立 for 項 in 値):
            raise ValueError("関係能力の上流不成立")
        return 値, 設定

    def 判定(self, 文脈):
        try:
            self._入力(文脈)
            return 1.0
        except (ValueError, TypeError, KeyError, AttributeError):
            return 0.0

    def 実行(self, 文脈):
        try:
            値, 設定 = self._入力(文脈)
            参照 = _参照結合(項 for 結果 in 値 for 項 in 結果.参照)
            if self.名前 == "資料関係読解":
                if (set(設定) != {"資料名", "要求"} or type(設定["資料名"]) is not list
                        or len(設定["資料名"]) != len(値) or len(set(設定["資料名"])) != len(値)):
                    raise ValueError("関係能力の資料名対応不一致")
                内容 = 関係資料を読む(dict(zip(設定["資料名"], 値)), 設定["要求"])
            elif self.名前 == "関係不足資料取得":
                構造 = 値[0].データ
                if not 関係資料を検査(構造) or not 構造["要求"]["不足調査"] or 構造["取得"] is not None:
                    raise ValueError("不足取得の要求・起点不正")
                if not self.外部許可 or self.取得器 is None:
                    raise ValueError("関係不足取得の外部読取未許可")
                取得 = None
                if 不足調査が必要(構造):
                    # 生の資料や導出過程を送信せず、利用者が調査を指定した問いだけを送る。
                    要求 = 関係調査要求(構造["要求"])
                    取得器 = 制御を配送する(self.取得器, 文脈.取得制御, "取得器")
                    取得 = 取得器.実行(要求, 外部読取許可=True, 停止要求=文脈.停止要求)
                    if not isinstance(取得, 能力結果):
                        raise ValueError("取得器の戻り型不正")
                    参照 = _参照結合((*参照, *取得.参照))
                内容 = {"版": "HDS関係不足取得-v1", "構造": 構造, "取得": None if 取得 is None else 結果を保存(取得)}
            elif self.名前 == "取得関係読解":
                記録 = 値[0].データ
                if set(記録) != {"版", "構造", "取得"} or 記録["版"] != "HDS関係不足取得-v1" or not 関係資料を検査(記録["構造"]):
                    raise ValueError("取得関係読解の起点不整合")
                構造 = 記録["構造"]
                if not 構造["要求"]["不足調査"] or (不足調査が必要(構造) and 記録["取得"] is None):
                    raise ValueError("必要な取得の記録欠落")
                内容 = 関係資料を読む({名: 結果を復元(行["資料"]) for 名, 行 in 構造["資料群"].items()},
                                     構造["要求"], 取得=None if 記録["取得"] is None else 結果を復元(記録["取得"]))
            elif self.名前 == "関係内容構成":
                内容 = 関係内容を構成(値[0].データ)
            elif self.名前 == "関係文章照合":
                return 関係回答を照合(値[0].データ, 値[1])
            else:
                if set(設定) != {"形式"}:
                    raise ValueError("関係再表現の欄不一致")
                return 関係回答を再表現(値[0], 設定["形式"])
            return 能力結果(True, self.名前 + "が成立しました。", 参照=参照, データ=内容)
        except 資料条件変更内容不足 as 例外:
            return 能力結果(False, "", データ=例外.診断, 保留理由="資料条件変更内容不成立:" + str(例外))
        except 主張内容不足 as 例外:
            return 能力結果(False, "", データ=例外.診断, 保留理由="関係処理不成立:" + str(例外))
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError, OverflowError) as 例外:
            return 能力結果(False, "", 保留理由="関係処理不成立:" + str(例外))

    def 登録(self):
        return 登録能力(self, 外部読取=self.名前 == "関係不足資料取得")


def 関係資料能力群(取得器, *, 外部許可=False):
    return tuple(関係資料能力(名, 取得器, 外部許可=外部許可).登録() for 名 in 関係能力名)
