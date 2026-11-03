"""資料回答の意味を変えず、有限な共有表と可逆圧縮で保存する。

共有表の印は保存の整合であり、資料・意味の採用証明ではない。呼出元は
展開後も原資料からの意味再生成を照合する。旧 plain はその版のまま読む。
"""
from __future__ import annotations

from copy import deepcopy
from hashlib import sha256
import base64
import binascii
import json
import math
import zlib

from ..能力合成 import _符号化

資料回答旧共有保存版 = "HDS資料回答共有保存-v1"
資料回答第2共有保存版 = "HDS資料回答共有保存-v2"
資料回答共有保存版 = "HDS資料回答共有保存-v3"
資料回答共有保存版群 = (資料回答旧共有保存版, 資料回答第2共有保存版, 資料回答共有保存版)
# 保存前の意味重複と保存後の量を区別する。工程の保存上限は変更しない。
_共有境界 = 500_000
_旧展開上限 = 16_000_000
_展開上限 = 32_000_000
_圧縮展開上限 = 128_000_000
_圧縮上限 = 1_000_000
_圧縮保存上限 = 1_400_000
_保存版上限 = {資料回答旧共有保存版: _旧展開上限,
              資料回答第2共有保存版: _展開上限, 資料回答共有保存版: _圧縮展開上限}
# 結果を保存した「内容/データ/意味成果」は深さ3にある。全体の32を保つ。
_意味深さ上限 = 29
_保存欄 = frozenset({"版", "意味版", "原正準SHA256", "原正準バイト数", "共有値", "参照置換", "本体"})
_圧縮保存欄 = frozenset({"版", "意味版", "原正準SHA256", "原正準バイト数", "原深さ",
                      "圧縮形式", "圧縮対象", "圧縮前SHA256", "圧縮前バイト数",
                      "圧縮SHA256", "圧縮バイト数", "圧縮本文"})


class _共有保存境界外(ValueError):
    """旧交換値では有効だが、新しい有限共有表へ収めない値。"""


def _JSON情報(値, *, 上限=_展開上限):
    """展開前にも使う有限JSON検査。別参照を複製せず長さと深さを数える。"""
    進行中 = set()
    節点数 = 0
    # 同じ呼出内の不変な原文字列だけを有限に記憶する。
    # 各出現の節点・深さ・長さ検査と、全コンテナの走査は省かない。
    文字列長 = {}
    記憶文字数 = 0

    def 読む(項, 深さ):
        nonlocal 節点数, 記憶文字数
        節点数 += 1
        if 深さ > _意味深さ上限 or 節点数 > 上限:
            raise _共有保存境界外("資料回答共有保存の深さ・節点上限")
        型 = type(項)
        if 項 is None or 型 in (str, bool, int, float):
            if 型 is float and not math.isfinite(項):
                raise ValueError("資料回答共有保存は有限JSONに限定")
            if 型 is str and len(項) > 上限:
                raise _共有保存境界外("資料回答共有保存のバイト上限")
            try:
                長さ = 文字列長.get(項) if 型 is str else None
                if 長さ is None:
                    長さ = len(json.dumps(項, ensure_ascii=False, allow_nan=False).encode("utf-8"))
            except (UnicodeError, ValueError) as 例外:
                raise ValueError("資料回答共有保存のJSON値不正") from 例外
            if 長さ > 上限:
                raise _共有保存境界外("資料回答共有保存のバイト上限")
            if (型 is str and 項 not in 文字列長 and len(文字列長) < 4096
                    and 記憶文字数 + len(項) <= 1_000_000):
                文字列長[項] = 長さ
                記憶文字数 += len(項)
            return 長さ, 深さ
        if 型 is tuple:
            # 旧交換値の組位置をJSON配列へ黙って変換せず、plainに残す。
            raise _共有保存境界外("資料回答共有保存は旧組位置を共有表へ変換しない")
        if 型 not in (dict, list) or (型 is dict and any(type(鍵) is not str for 鍵 in 項)):
            raise ValueError("資料回答共有保存は厳密なJSON型に限定")
        if id(項) in 進行中:
            raise ValueError("資料回答共有保存に循環があります")
        進行中.add(id(項))
        try:
            長さ, 最大深さ = 2, 深さ
            for 番号, (鍵, 子) in enumerate(項.items() if 型 is dict else enumerate(項)):
                if 番号:
                    長さ += 1
                if 型 is dict:
                    鍵長, _ = 読む(鍵, 深さ + 1)
                    長さ += 鍵長 + 1
                子長, 子深さ = 読む(子, 深さ + 1)
                長さ += 子長
                最大深さ = max(最大深さ, 子深さ)
                if 長さ > 上限:
                    raise _共有保存境界外("資料回答共有保存のバイト上限")
            return 長さ, 最大深さ
        finally:
            進行中.remove(id(項))

    return 読む(値, 0)


def _バイト(値, *, 上限=_展開上限):
    _JSON情報(値, 上限=上限)
    return json.dumps(値, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _意味版を検査(値, 意味版):
    if type(意味版) is not str or not 意味版 or type(値) is not dict or 値.get("版") != 意味版:
        raise ValueError("資料回答の意味保存版が失効しています。原保存を保持して再計算してください")


def _候補箇所(意味):
    """共有を認める閉じたパス。意味作用・未知記載・残差等は共有しない。"""
    箇所 = []

    def 所有又は座標(親, 鍵, 道, 種別):
        値 = 親[鍵]
        if 値 is not None and type(値) is not dict:
            raise ValueError("資料回答共有保存の所有又は座標の型不正")
        箇所.append((道, 種別, 値))

    行群 = 意味.get("記載関係", [])
    成果 = 意味.get("資料作用成果", {})
    if type(行群) is not list or type(成果) is not dict:
        raise ValueError("資料回答共有保存の記載又は作用成果の型不正")
    for 番号, 行 in enumerate(行群):
        if type(行) is not dict or type(行.get("資料作用")) is not dict:
            raise ValueError("資料回答共有保存の記載作用の型不正")
        射影 = 行["資料作用"]
        道 = ["記載関係", 番号, "資料作用"]
        if "原資料" in 射影:
            所有又は座標(射影, "原資料", [*道, "原資料"], "所有")
        座標 = 射影.get("座標", [])
        if type(座標) is not list:
            raise ValueError("資料回答共有保存の座標列の型不正")
        for 番, _ in enumerate(座標):
            所有又は座標(座標, 番, [*道, "座標", 番], "座標")
    for 欄, 種別 in (("原資料", "所有"), ("座標", "座標")):
        列 = 成果.get(欄, [])
        if type(列) is not list:
            raise ValueError("資料回答共有保存の集約列の型不正")
        for 番, _ in enumerate(列):
            所有又は座標(列, 番, ["資料作用成果", 欄, 番], 種別)
    適用 = 成果.get("適用", {})
    if type(適用) is not dict:
        raise ValueError("資料回答共有保存の適用表の型不正")
    for 名, 行 in 適用.items():
        if type(行) is not dict:
            raise ValueError("資料回答共有保存の適用行の型不正")
        if "原資料" in 行:
            所有又は座標(行, "原資料", ["資料作用成果", "適用", 名, "原資料"], "所有")
    return 箇所


def _識別(種別, 値):
    鍵 = "資料ID" if 種別 == "所有" else "座標ID"
    名 = 値.get(鍵)
    if type(名) is not str or not 名:
        raise ValueError("資料回答共有保存の共有対象ID不正")
    if 種別 == "所有" and (type(値.get("保存指紋")) is not str or not 値["保存指紋"]):
        raise ValueError("資料回答共有保存の所有保存指紋不正")
    return 種別, 名


def _共有ID(種別, 本文):
    return 種別 + ":" + sha256(種別.encode("utf-8") + b"\x00" + 本文).hexdigest()


def _箇所へ置く(本体, 道, 値):
    親 = 本体
    for 鍵 in 道[:-1]:
        親 = 親[鍵]
    親[道[-1]] = 値


def _正準本体を複製(意味, 原, *, 元長, 元深さ, 上限):
    """検査済み原正準保存と同値の独立コンテナを作り、不変値だけ再使用する。"""
    if type(原) is not bytes or type(元長) is not int or type(元深さ) is not int:
        raise ValueError("資料回答本体複製の原・宣言型不正")
    実長, 実深さ = _JSON情報(意味, 上限=上限)
    if 元長 != 実長 or 元深さ != 実深さ or len(原) != 実長:
        raise ValueError("資料回答本体複製の原・宣言長深さ不一致")
    進行中, 節点数 = set(), 0

    def 複製(項, 深さ):
        nonlocal 節点数
        節点数 += 1
        if 深さ > _意味深さ上限 or 節点数 > 上限:
            raise ValueError("資料回答本体複製の深さ・節点上限")
        型 = type(項)
        if 項 is None or 型 is str or 型 is bool or 型 is int or 型 is float:
            if 型 is float and not math.isfinite(項):
                raise ValueError("資料回答本体複製の非有限数")
            if 型 is str and len(項) > 上限:
                raise ValueError("資料回答本体複製の文字数上限")
            return 項
        if 型 is not dict and 型 is not list:
            raise ValueError("資料回答本体複製の厳密型不正")
        if id(項) in 進行中:
            raise ValueError("資料回答本体複製に循環があります")
        進行中.add(id(項))
        try:
            if 型 is dict:
                鍵群 = []
                for 鍵 in 項:
                    if type(鍵) is not str:
                        raise ValueError("資料回答本体複製の辞書鍵型不正")
                    鍵群.append(複製(鍵, 深さ + 1))
                鍵群.sort()
                return {鍵: 複製(項[鍵], 深さ + 1) for 鍵 in 鍵群}
            return [複製(子, 深さ + 1) for 子 in 項]
        finally:
            進行中.remove(id(項))

    try:
        本体 = 複製(意味, 0)
    except (KeyError, RuntimeError, RecursionError) as 例外:
        raise ValueError("資料回答本体複製中の入力不正") from 例外
    if _バイト(本体, 上限=上限) != 原:
        raise ValueError("資料回答本体複製と原正準保存が不一致")
    return 本体


def _共有表を保存(意味, *, 意味版, 保存版, 上限, 原情報=None):
    """旧版と同じ閉パス・同ID全同値だけを内部共有表へする。"""
    _意味版を検査(意味, 意味版)
    if 原情報 is None:
        try:
            元長, 元深さ = _JSON情報(意味, 上限=上限)
        except _共有保存境界外:
            # 境界外だけを旧plainへ戻す。後方のNaN・型不正・循環等も既存検査を通す。
            _符号化(意味)
            return 意味
    else:
        原, 元長, 元深さ = 原情報
    群 = {}
    for 道, 種別, 値 in _候補箇所(意味):
        if 値 is None:
            # plainの欠落を参照と解釈しない。
            continue
        名 = _識別(種別, 値)
        本文 = _バイト(値, 上限=上限)
        if 名 in 群 and 群[名][0] != 本文:
            raise ValueError("資料回答共有保存の同ID異義衝突")
        群.setdefault(名, (本文, []))[1].append(道)
    if 原情報 is None:
        原 = _バイト(意味, 上限=上限)
    if 元長 <= _共有境界:
        return json.loads(原)
    本体 = _正準本体を複製(意味, 原, 元長=元長, 元深さ=元深さ, 上限=上限)
    共有値, 参照置換 = [], []
    for (種別, _), (本文, 道群) in 群.items():
        if len(道群) < 2:
            continue
        値 = json.loads(本文)
        _, 深さ = _JSON情報(値, 上限=上限)
        if 深さ + 3 > _意味深さ上限:
            return json.loads(原)
        名 = _共有ID(種別, 本文)
        共有値.append({"共有ID": 名, "種別": 種別, "値": 値})
        for 道 in 道群:
            _箇所へ置く(本体, 道, None)
            参照置換.append({"パス": 道, "共有ID": 名})
    if not 共有値 or 元深さ + 1 > _意味深さ上限:
        return json.loads(原)
    保存 = {"版": 保存版, "意味版": 意味版,
            "原正準SHA256": sha256(原).hexdigest(), "原正準バイト数": 元長,
            "共有値": sorted(共有値, key=lambda 行: 行["共有ID"]),
            "参照置換": sorted(参照置換, key=lambda 行: _バイト(行["パス"], 上限=上限)), "本体": 本体}
    try:
        保存長, _ = _JSON情報(保存, 上限=上限)
    except _共有保存境界外:
        # 付加表で従来の有限保存範囲を狭めない。
        return json.loads(原)
    return 保存 if 保存長 < 元長 else json.loads(原)


def _共有表を読む(保存, *, 意味版, 保存版, 上限):
    """量・深さ・全候補を確認してから閉じた共有表を展開する。"""
    _JSON情報(保存, 上限=上限)
    if (set(保存) != _保存欄 or 保存["意味版"] != 意味版
            or type(保存["原正準バイト数"]) is not int
            or not _共有境界 < 保存["原正準バイト数"] <= 上限
            or type(保存["原正準SHA256"]) is not str or len(保存["原正準SHA256"]) != 64
            or any(字 not in "0123456789abcdef" for 字 in 保存["原正準SHA256"])
            or type(保存["共有値"]) is not list or not 保存["共有値"]
            or type(保存["参照置換"]) is not list or not 保存["参照置換"]):
        raise ValueError("資料回答共有保存の版・欄・量不正")
    本体 = 保存["本体"]
    _意味版を検査(本体, 意味版)
    許可 = {tuple(道): 種別 for 道, 種別, _ in _候補箇所(本体)}
    表, 同ID = {}, {}
    for 行 in 保存["共有値"]:
        if (type(行) is not dict or set(行) != {"共有ID", "種別", "値"}
                or type(行["共有ID"]) is not str or 行["種別"] not in ("所有", "座標")
                or type(行["値"]) is not dict):
            raise ValueError("資料回答共有保存の共有値形式不正")
        名, 種別 = 行["共有ID"], 行["種別"]
        本文 = _バイト(行["値"], 上限=上限)
        身元 = _識別(種別, 行["値"])
        if 名 in 表 or (身元 in 同ID and 同ID[身元] != 本文) or 名 != _共有ID(種別, 本文):
            raise ValueError("資料回答共有保存の共有ID重複・衝突・印不一致")
        同ID[身元] = 本文
        表[名] = (種別, 本文, _JSON情報(行["値"], 上限=上限)[1])
    使用, 道群 = {名: 0 for 名 in 表}, set()
    展開長 = _JSON情報(本体, 上限=上限)[0]
    for 行 in 保存["参照置換"]:
        if (type(行) is not dict or set(行) != {"パス", "共有ID"}
                or type(行["パス"]) is not list or not 行["パス"]
                or any(type(鍵) not in (str, int) for 鍵 in 行["パス"])
                or type(行["共有ID"]) is not str or 行["共有ID"] not in 表):
            raise ValueError("資料回答共有保存の参照形式不正")
        道, 名 = tuple(行["パス"]), 行["共有ID"]
        種別, 本文, 深さ = 表[名]
        if 道 in 道群 or 許可.get(道) != 種別 or len(道) + 深さ > _意味深さ上限:
            raise ValueError("資料回答共有保存の参照パス・重複・展開深さ不正")
        項 = 本体
        for 鍵 in 道:
            項 = 項[鍵]
        if 項 is not None:
            raise ValueError("資料回答共有保存の参照位置が空置換ではありません")
        道群.add(道)
        使用[名] += 1
        展開長 += len(本文) - 4  # JSON nullだけをリテラル全文へ戻す。
        if 展開長 > 上限:
            raise ValueError("資料回答共有保存の展開バイト上限")
    if 展開長 != 保存["原正準バイト数"] or any(数 < 2 for 数 in 使用.values()):
        raise ValueError("資料回答共有保存の展開長・未使用又は非重複共有値不正")
    # 長さ・深さが有限と確認されるまでは、参照値を複製しない。
    意味 = json.loads(_バイト(本体, 上限=上限))
    for 行 in 保存["参照置換"]:
        _箇所へ置く(意味, 行["パス"], json.loads(表[行["共有ID"]][1]))
    原 = _バイト(意味, 上限=上限)
    if len(原) != 展開長 or sha256(原).hexdigest() != 保存["原正準SHA256"]:
        raise ValueError("資料回答共有保存の展開本文印不一致")
    再保存 = _共有表を保存(意味, 意味版=意味版, 保存版=保存版, 上限=上限)
    if _バイト(再保存, 上限=上限) != _バイト(保存, 上限=上限):
        raise ValueError("資料回答共有保存の共有表・参照順序が非正準")
    return 意味


def _印(値):
    return (type(値) is str and len(値) == 64
            and all(字 in "0123456789abcdef" for 字 in 値))


def _厳密JSON(原):
    """圧縮で隠れた深い構文や重複keyをJSON化前に拒否する。"""
    try:
        本文 = 原.decode("utf-8", errors="strict")
        深さ, 文字内, escape = 0, False, False
        for 字 in 本文:
            if 文字内:
                if escape:
                    escape = False
                elif 字 == "\\":
                    escape = True
                elif 字 == '"':
                    文字内 = False
            elif 字 == '"':
                文字内 = True
            elif 字 in "[{":
                深さ += 1
                if 深さ > _意味深さ上限 + 1:
                    raise ValueError("資料回答圧縮保存のJSON構文深さ上限")
            elif 字 in "]}":
                深さ -= 1

        def 辞書(組):
            値 = {}
            for 鍵, 子 in 組:
                if 鍵 in 値:
                    raise ValueError("資料回答圧縮保存のJSON重複key")
                値[鍵] = 子
            return 値

        def 非有限(値):
            raise ValueError("資料回答圧縮保存の非有限JSON数")

        値 = json.loads(本文, object_pairs_hook=辞書, parse_constant=非有限)
        if _バイト(値, 上限=_圧縮展開上限) != 原:
            raise ValueError("資料回答圧縮保存の内部JSONが非正準")
        return 値
    except (UnicodeError, RecursionError, json.JSONDecodeError) as 例外:
        raise ValueError("資料回答圧縮保存のUTF8・JSON不正") from 例外


def _圧縮保存を作る(意味, *, 意味版):
    try:
        元長, 元深さ = _JSON情報(意味, 上限=_圧縮展開上限)
    except _共有保存境界外:
        _符号化(意味)
        return 意味
    原 = json.dumps(意味, ensure_ascii=False, sort_keys=True,
                   separators=(",", ":"), allow_nan=False).encode("utf-8")
    対象 = _共有表を保存(意味, 意味版=意味版, 保存版=資料回答共有保存版,
                     上限=_圧縮展開上限, 原情報=(原, 元長, 元深さ))
    if 元長 <= _共有境界:
        return 対象
    共有 = 対象.get("版") == 資料回答共有保存版
    圧縮前 = _バイト(対象, 上限=_圧縮展開上限) if 共有 else 原
    圧縮 = zlib.compress(圧縮前, level=9)
    if len(圧縮) > _圧縮上限:
        return json.loads(原)
    保存 = {"版": 資料回答共有保存版, "意味版": 意味版,
            "原正準SHA256": sha256(原).hexdigest(), "原正準バイト数": 元長, "原深さ": 元深さ,
            "圧縮形式": "zlib", "圧縮対象": "閉共有JSON-v1" if 共有 else "原正準JSON",
            "圧縮前SHA256": sha256(圧縮前).hexdigest(), "圧縮前バイト数": len(圧縮前),
            "圧縮SHA256": sha256(圧縮).hexdigest(), "圧縮バイト数": len(圧縮),
            "圧縮本文": base64.b64encode(圧縮).decode("ascii")}
    try:
        保存長, _ = _JSON情報(保存, 上限=_圧縮保存上限)
    except _共有保存境界外:
        return json.loads(原)
    return 保存 if 保存長 < 元長 else json.loads(原)


def _圧縮保存を読む(保存, *, 意味版):
    保存長, _ = _JSON情報(保存, 上限=_圧縮保存上限)
    if (set(保存) != _圧縮保存欄 or 保存["版"] != 資料回答共有保存版
            or 保存["意味版"] != 意味版 or 保存["圧縮形式"] != "zlib"
            or 保存["圧縮対象"] not in ("原正準JSON", "閉共有JSON-v1")
            or type(保存["原正準バイト数"]) is not int
            or not _共有境界 < 保存["原正準バイト数"] <= _圧縮展開上限
            or 保存長 >= 保存["原正準バイト数"]
            or type(保存["原深さ"]) is not int or not 0 <= 保存["原深さ"] <= _意味深さ上限
            or type(保存["圧縮前バイト数"]) is not int
            or not 0 < 保存["圧縮前バイト数"] <= _圧縮展開上限
            or type(保存["圧縮バイト数"]) is not int
            or not 0 < 保存["圧縮バイト数"] <= _圧縮上限
            or any(not _印(保存[欄]) for 欄 in ("原正準SHA256", "圧縮前SHA256", "圧縮SHA256"))
            or type(保存["圧縮本文"]) is not str
            or len(保存["圧縮本文"]) > ((_圧縮上限 + 2) // 3) * 4):
        raise ValueError("資料回答圧縮保存の版・型・欄・量不正")
    try:
        圧縮 = base64.b64decode(保存["圧縮本文"], validate=True)
    except (ValueError, binascii.Error) as 例外:
        raise ValueError("資料回答圧縮保存のbase64不正") from 例外
    if (base64.b64encode(圧縮).decode("ascii") != 保存["圧縮本文"]
            or len(圧縮) != 保存["圧縮バイト数"]
            or sha256(圧縮).hexdigest() != 保存["圧縮SHA256"]):
        raise ValueError("資料回答圧縮保存の圧縮印・量不一致")
    try:
        復号 = zlib.decompressobj()
        原対象 = 復号.decompress(圧縮, 保存["圧縮前バイト数"] + 1)
    except zlib.error as 例外:
        raise ValueError("資料回答圧縮保存のzlib不正") from 例外
    if (len(原対象) != 保存["圧縮前バイト数"] or not 復号.eof
            or 復号.unconsumed_tail or 復号.unused_data
            or sha256(原対象).hexdigest() != 保存["圧縮前SHA256"]):
        raise ValueError("資料回答圧縮保存の解凍量・終端・印不一致")
    対象 = _厳密JSON(原対象)
    if 保存["圧縮対象"] == "閉共有JSON-v1":
        if (type(対象) is not dict or 対象.get("版") != 資料回答共有保存版
                or 対象.get("原正準SHA256") != 保存["原正準SHA256"]
                or 対象.get("原正準バイト数") != 保存["原正準バイト数"]):
            raise ValueError("資料回答圧縮保存の内部共有表の版・原印不一致")
        意味 = _共有表を読む(対象, 意味版=意味版, 保存版=資料回答共有保存版, 上限=_圧縮展開上限)
    else:
        _意味版を検査(対象, 意味版)
        意味 = 対象
        # plain対象でも同ID相違を拒否し、共有可能な表を任意に省略しない。
        再表 = _共有表を保存(意味, 意味版=意味版, 保存版=資料回答共有保存版, 上限=_圧縮展開上限)
        if _バイト(再表, 上限=_圧縮展開上限) != 原対象:
            raise ValueError("資料回答圧縮保存の内部共有形式が非正準")
    原 = _バイト(意味, 上限=_圧縮展開上限)
    if (len(原) != 保存["原正準バイト数"] or sha256(原).hexdigest() != 保存["原正準SHA256"]
            or _JSON情報(意味, 上限=_圧縮展開上限)[1] != 保存["原深さ"]):
        raise ValueError("資料回答圧縮保存の原正準印・量・深さ不一致")
    return 意味, 原


def 資料回答意味を保存(意味, *, 意味版, 保存版=資料回答共有保存版, 保存元=None):
    """明示版で保存。v3照合時は同じ原正準の元frameをそのまま保持できる。"""
    _意味版を検査(意味, 意味版)
    if type(保存版) is not str or 保存版 not in _保存版上限:
        raise ValueError("資料回答共有保存の保存版が未対応です。原保存を保持して明示再計算してください")
    if 保存元 is not None:
        if 保存版 != 資料回答共有保存版 or type(保存元) is not dict:
            raise ValueError("資料回答圧縮保存の保存元はv3同版に限定")
        _, 原 = _圧縮保存を読む(保存元, 意味版=意味版)
        if _バイト(意味, 上限=_圧縮展開上限) != 原:
            raise ValueError("資料回答圧縮保存の保存元と原正準が不一致")
        return deepcopy(保存元)
    if 保存版 == 資料回答共有保存版:
        return _圧縮保存を作る(意味, 意味版=意味版)
    return _共有表を保存(意味, 意味版=意味版, 保存版=保存版, 上限=_保存版上限[保存版])


def 資料回答意味を読む(保存, *, 意味版):
    """strict保存から独立ビューへ戻す。読取だけで旧版を更新しない。"""
    if type(保存) is not dict or 保存.get("版") not in 資料回答共有保存版群:
        _意味版を検査(保存, 意味版)
        _符号化(保存)
        return deepcopy(保存)
    if 保存["版"] == 資料回答共有保存版:
        return _圧縮保存を読む(保存, 意味版=意味版)[0]
    return _共有表を読む(保存, 意味版=意味版, 保存版=保存["版"], 上限=_保存版上限[保存["版"]])
