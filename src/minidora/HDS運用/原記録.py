"""HDS採用前に原記録更新候補を構成する。原文と失効履歴を削除しない。"""
from __future__ import annotations
import base64
import binascii
from copy import deepcopy
from hashlib import sha256
import json
import zlib
from ..長文脈管理 import 長文脈庫, 長文脈版, 文脈登録
from ..監査改善会話解釈 import JSONを厳格に読む
from ..製品版.型 import 能力結果
from .値 import 指紋, 正準, 結果を保存, 結果を復元

原記録接続版 = "HDS運用原記録-v3"
_旧接続版 = "HDS運用原記録-v1"
_旧可逆接続版 = "HDS運用原記録-v2"
_可逆保存版 = "HDS原記録可逆保存-v1"
_有限可逆保存版 = "HDS原記録可逆保存-v2"
_符号列上限 = {_可逆保存版: 1_000_000, _有限可逆保存版: 2_000_000}
_圧縮境界 = 500_000
_復号上限 = 16_000_000


def _正準バイト(値):
    return json.dumps(値, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")


def _結果を符号化(結果, *, 保存版=_有限可逆保存版):
    if type(保存版) is not str or 保存版 not in _符号列上限:
        raise ValueError("原記録成果の保存版不正")
    原 = 結果を保存(結果)
    本文 = _正準バイト(原)
    if len(本文) <= _圧縮境界:
        return 原
    if len(本文) > _復号上限:
        raise ValueError("原記録成果の復号サイズ上限")
    符号 = {"版": 保存版, "符号化": "zlib+base64", "原バイト数": len(本文),
            "原SHA256": sha256(本文).hexdigest(),
            "符号列": base64.b64encode(zlib.compress(本文, level=9)).decode("ascii")}
    # 不圧縮材料を符号化の付加量で従来の記録上限へ到達させない。
    return 符号 if (len(符号["符号列"]) <= _符号列上限[保存版]
                   and len(_正準バイト(符号)) < len(本文)) else 原


def _符号の元長(符号):
    if (type(符号) is not dict or set(符号) != {"版", "符号化", "原バイト数", "原SHA256", "符号列"}
            or type(符号["版"]) is not str or 符号["版"] not in _符号列上限 or 符号["符号化"] != "zlib+base64"
            or type(符号["原バイト数"]) is not int or not _圧縮境界 < 符号["原バイト数"] <= _復号上限
            or type(符号["原SHA256"]) is not str or len(符号["原SHA256"]) != 64
            or any(字 not in "0123456789abcdef" for 字 in 符号["原SHA256"])
            or type(符号["符号列"]) is not str or len(符号["符号列"]) > _符号列上限[符号["版"]]):
        raise ValueError("原記録成果の可逆保存形式不正")
    return 符号["原バイト数"]


def 原記録成果を復元(記録):
    """旧成果はそのまま読み、新しい符号表現だけを原交換値へ戻す。"""
    値 = 記録["内容"]["データ"].get("運用結果")
    if type(値) is not dict or "版" not in 値:
        return deepcopy(値)
    元長 = _符号の元長(値)
    try:
        圧縮 = base64.b64decode(値["符号列"], validate=True)
        if base64.b64encode(圧縮).decode("ascii") != 値["符号列"]:
            raise ValueError("原記録成果の符号列が非正準")
        復号器 = zlib.decompressobj()
        本文 = 復号器.decompress(圧縮, 元長 + 1)
    except (binascii.Error, zlib.error, UnicodeError) as 例外:
        raise ValueError("原記録成果の符号列不正") from 例外
    if (len(本文) != 元長 or not 復号器.eof or 復号器.unused_data or 復号器.unconsumed_tail
            or sha256(本文).hexdigest() != 値["原SHA256"]):
        raise ValueError("原記録成果の復号長・終端・余剰・本文印不一致")
    原 = JSONを厳格に読む(本文.decode("utf-8"), 最大バイト数=_復号上限)
    if _正準バイト(原) != 本文:
        raise ValueError("原記録成果の交換JSONが非正準")
    結果 = 結果を復元(原)
    外側 = 記録["内容"]
    if (記録["種別"] != "成果" or not 結果.成立 or 外側["成立"] is not True
            or 原["内容"]["本文"] != 外側["本文"] or 原["内容"]["参照"] != 外側["参照"]):
        raise ValueError("原記録成果と外側の成立・本文・参照が不一致")
    return 原


def _保存容量を検査(保存物):
    """符号化で従来の原記録保存容量を増やさず、失効した成果も計数する。"""
    庫 = 保存物["庫"]
    使用量 = len(_正準バイト(庫))
    上限 = 庫["上限"][1]
    if 保存物["版"] != 原記録接続版 and 庫["版"] != 長文脈版:
        raise ValueError("旧原記録版に新しい単一記録上限を混入しない")
    if 保存物["版"] == 原記録接続版:
        上限 = min(上限, _復号上限)
    for 事象 in 庫["履歴"]:
        for 行 in 事象["追加"]:
            符号 = 行["内容"]["データ"].get("運用結果")
            if type(符号) is not dict or "版" not in 符号:
                continue
            if (保存物["版"] == _旧接続版 or 保存物["版"] == _旧可逆接続版
                    and 符号.get("版") != _可逆保存版):
                raise ValueError("旧原記録版に新しい可逆成果を混入しない")
            使用量 += _符号の元長(符号) - len(_正準バイト(符号))
            if 使用量 > 上限:
                raise ValueError("原記録の復号後保存サイズ上限。更新を確定しない")
            原記録成果を復元(行)
    if 使用量 > 上限:
        raise ValueError("原記録の復号後保存サイズ上限。更新を確定しない")


class 運用原記録:
    def __init__(self, セッションID, *, 保存版=原記録接続版):
        if type(保存版) is not str or 保存版 not in (_旧接続版, _旧可逆接続版, 原記録接続版):
            raise ValueError("運用原記録の保存版不正")
        self.ID = セッションID
        self.版 = 保存版
        self.庫 = 長文脈庫(セッションID, 最大単一記録バイト数=2_000_000 if 保存版 == 原記録接続版 else 1_000_000)
        self.資料対応 = {}
        self.連番 = 0

    def 保存(self):
        return {"版": self.版, "資料対応": deepcopy(self.資料対応),
                "連番": self.連番, "庫": json.loads(self.庫.保存文字列())}

    @classmethod
    def 復元(cls, 保存物, セッションID):
        if (type(保存物) is not dict or set(保存物) != {"版", "資料対応", "連番", "庫"}
                or 保存物["版"] not in (_旧接続版, _旧可逆接続版, 原記録接続版) or type(保存物["資料対応"]) is not dict
                or type(保存物["連番"]) is not int or not 0 <= 保存物["連番"] <= 16384):
            raise ValueError("運用原記録の形式不正")
        個体 = cls(セッションID, 保存版=保存物["版"])
        個体.庫 = 長文脈庫.復元(json.dumps(保存物["庫"], ensure_ascii=False, allow_nan=False))
        if 個体.庫.起点().セッションID != セッションID:
            raise ValueError("原記録の所有セッションが不一致")
        個体.資料対応 = deepcopy(保存物["資料対応"])
        個体.連番 = 保存物["連番"]
        for 識別子 in 個体.資料対応.values():
            if not 個体.庫.原記録(識別子)["現行"]:
                raise ValueError("現行資料が旧原記録を参照")
        _保存容量を検査(保存物)
        return 個体

    def 資料を照合(self, 資料):
        if set(資料) != set(self.資料対応):
            raise ValueError("資料と原記録の対応数不一致")
        for 名前, 値 in 資料.items():
            記録 = self.庫.原記録(self.資料対応[名前])
            内容 = 記録["内容"]
            if (not 記録["現行"] or 記録["種別"] != "資料"
                    or 内容["データ"] != {"運用資料": 正準(値), "名前": 名前}
                    or 内容["本文"] != 値["本文"]
                    or 内容["参照"] != 値["結果"]["内容"]["参照"]):
                raise ValueError("原記録と現行資料の本体・版・参照が不一致")

    def 同期候補(self, 入力, 更新, 回答, 依存, *, 知識範囲=False, 全資料範囲=False,
             失効成果ID=()):
        """呼出元と別の庫へ候補を作る。HDS未採用時に元の庫は不変。"""
        候補 = type(self).復元(self.保存(), self.ID)
        候補.資料を照合(入力["資料"])
        if (type(失効成果ID) is not tuple or len(失効成果ID) > 4
                or len(set(失効成果ID)) != len(失効成果ID)
                or any(type(名) is not str for 名 in 失効成果ID)
                or 更新 and 失効成果ID):
            raise ValueError("会話訂正の失効成果指定が不正です")
        for 名 in 失効成果ID:
            旧成果 = 候補.庫.原記録(名)
            if 旧成果["種別"] != "成果" or not 旧成果["現行"]:
                raise ValueError("会話訂正の元成果が現行の実成果ではありません")
        if 更新 and 更新["行為"] == "初期化":
            return type(self)(self.ID, 保存版=_旧可逆接続版 if self.版 == _旧接続版 else self.版)
        if 更新:
            名前, 資料 = 更新["名前"], 更新["資料"]
            既存 = 入力["資料"].get(名前)
            if 既存 == 資料:
                return 候補
            候補.連番 += 1
            識別子 = "資料:" + str(候補.連番) + ":" + 指紋((名前, 資料["版"]))[:32]
            元 = 結果を復元(資料["結果"])
            内容 = 能力結果(True, 元.本文, 参照=元.参照,
                          データ={"運用資料": 正準(資料), "名前": 名前})
            旧 = (候補.資料対応[名前],) if 名前 in 候補.資料対応 else ()
            # 全体照会は資料追加でも適用集合が変わる。旧成果は履歴として保持する。
            知識変更 = 資料["種類"] == "知識" or (既存 and 既存["種類"] == "知識")
            集合成果 = []
            for 事象 in 候補.保存()["庫"]["履歴"]:
                for 行 in 事象["追加"]:
                    記録 = 行["内容"]["データ"]
                    if (行["種別"] == "成果" and (記録.get("全資料範囲") is True or 知識変更 and 記録.get("知識範囲") is True)
                            and 候補.庫.原記録(行["識別子"])["現行"]):
                        集合成果.append(行["識別子"])
            旧 = tuple(dict.fromkeys((*旧, *集合成果)))
            候補.庫.更新(候補.庫.起点(), (文脈登録(識別子, 内容),),
                        失効ID=旧, 理由="利用者による資料更新" if 旧 else "")
            候補.資料対応[名前] = 識別子
        else:
            # 成功回答を保存するが、元資料の代わりの事実・指示としては登録しない。
            候補.連番 += 1
            識別子 = "成果:" + str(候補.連番) + ":" + 指紋(入力["原文"])[:24]
            for 名前, 版 in 依存.items():
                if 名前 not in 入力["資料"] or 入力["資料"][名前]["版"] != 版:
                    raise ValueError("成果の依存資料が不一致")
            内容 = 能力結果(True, 回答.本文, 参照=回答.参照,
                          データ={"運用結果": _結果を符号化(回答, 保存版=_有限可逆保存版 if 候補.版 == 原記録接続版 else _可逆保存版), "原要求": 入力["原文"],
                                  "依存版": 正準(依存), "知識範囲": 知識範囲, "事実認定": False,
                                  **({"全資料範囲": True} if 全資料範囲 else {})})
            候補.庫.更新(候補.庫.起点(), (文脈登録(識別子, 内容, "成果",
                        tuple(候補.資料対応[名前] for 名前 in sorted(依存))),),
                        失効ID=失効成果ID,
                        理由="利用者による依頼条件の訂正" if 失効成果ID else "")
        # 旧v1の実更新だけ従来のv2へ進める。単一上限と旧履歴は昇格・再圧縮しない。
        if 候補.版 == _旧接続版:
            候補.版 = _旧可逆接続版
        _保存容量を検査(候補.保存())
        return 候補
