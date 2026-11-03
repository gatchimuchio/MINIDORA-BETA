"""原取得成果と未消費候補の対応を読む。意味充足・検索必要性は判断しない。"""
from copy import deepcopy
from dataclasses import asdict
import re

from .公開本文取得 import 公開URL
from .製品版.型 import 能力結果
from .知識取得 import (知識取得版, 知識継続版, 知識取得要求,
                    _記録印, _本文記録を復元, _照合語)


def 継続束を読む(成果, 要求=None):
    return _継続記録を読む(成果, 要求, 不成立=False)


def _継続記録を読む(成果, 要求, *, 不成立):
    if (not isinstance(成果, 能力結果) or type(成果.成立) is not bool or 成果.成立 is 不成立
            or (成果.保留理由 != "未消費候補がありません" if 不成立 else bool(成果.保留理由))
            or type(成果.データ) is not dict):
        raise ValueError("継続元は取得成功成果に限定します" if not 不成立 else "検証できる候補消費不成立に限定します")
    d = 成果.データ
    必須 = {"版", "状態", "意味的事実検証", "要求", "不足語", "資料", "抜粋",
            "試行", "本文取得数", "資料数", "記録SHA256", "継続束"}
    if (not 必須 <= set(d) or set(d) - 必須 - {"資源制御", "検索来歴"}
            or d["版"] != 知識取得版 or d["状態"] != ("保留" if 不成立 else "合格")
            or d["意味的事実検証"] != "未実施" or d["不足語"]
            or _記録印({k: v for k, v in d.items() if k != "記録SHA256"}) != d["記録SHA256"]):
        raise ValueError("取得継続元の欄・印が不一致です")
    if type(d["要求"]) is not dict or set(d["要求"]) != set(知識取得要求.__dataclass_fields__):
        raise ValueError("元取得要求の欄不一致")
    設定 = dict(d["要求"])
    for 名 in ("必要語", "優先ホスト"):
        if type(設定[名]) not in (tuple, list):
            raise ValueError("元取得要求の列型不一致")
        設定[名] = tuple(設定[名])
    元要求 = 知識取得要求(**設定)
    元要求.検証()
    if 要求 is not None:
        if not isinstance(要求, 知識取得要求):
            raise ValueError("継続要求の型不正")
        要求.検証()
        if _記録印(asdict(要求)) != _記録印(asdict(元要求)):
            raise ValueError("継続時に元要求・上限は変更できません")
    要求 = 元要求
    for 名, 上限 in (("資料", 要求.最大資料数), ("抜粋", 50000), ("試行", 300)):
        if type(d[名]) is not list or len(d[名]) > 上限:
            raise ValueError("取得累積記録の型・上限不正")
    if (type(d["本文取得数"]) is not int or type(d["資料数"]) is not int
            or not 要求.最低資料数 <= d["資料数"] <= 要求.最大資料数
            or not d["資料数"] <= d["本文取得数"] <= 要求.最大取得数):
        raise ValueError("取得累積数不一致")
    束 = d["継続束"]
    欄 = {"版", "要求SHA256", "候補", "消費位置", "検索済み語", "試行済みURL", "親記録SHA256"}
    if (type(束) is not dict or set(束) != 欄 or 束["版"] != 知識継続版
            or 束["要求SHA256"] != _記録印(asdict(要求))
            or 束["親記録SHA256"] is not None and
               (type(束["親記録SHA256"]) is not str or not re.fullmatch(r"[0-9a-f]{64}", 束["親記録SHA256"]))):
        raise ValueError("取得継続束の版・元要求印不一致")
    候補, 検索語, 試行URL = (束[k] for k in ("候補", "検索済み語", "試行済みURL"))
    if (type(候補) is not list or len(候補) > 要求.最大検索回数 * 要求.最大候補数
            or type(検索語) is not list or not 1 <= len(検索語) <= 要求.最大検索回数
            or any(type(x) is not str or not x.strip() or len(x) > 2048 for x in 検索語)
            or len(set(検索語)) != len(検索語)
            or type(試行URL) is not list or len(試行URL) != d["本文取得数"]
            or len(set(試行URL)) != len(試行URL)
            or type(束["消費位置"]) is not int or not 0 <= 束["消費位置"] <= len(候補)):
        raise ValueError("取得継続の候補・位置・累積数不正")
    順序 = {}
    前回 = 0
    for 行 in 候補:
        if (type(行) is not dict or set(行) != {"URL", "題名", "検索語", "回", "順"}
                or type(行["回"]) is not int or not 1 <= 行["回"] <= len(検索語)
                or 行["検索語"] != 検索語[行["回"] - 1] or 行["回"] < 前回
                or type(行["順"]) is not int or 行["順"] != 順序.get(行["回"], 0)
                or 行["順"] >= 要求.最大候補数
                or type(行["題名"]) is not str or len(行["題名"]) > 1024
                or 公開URL(行["URL"]) != 行["URL"]):
            raise ValueError("取得継続候補の原順序・公開URL不正")
        前回 = 行["回"]
        順序[前回] = 行["順"] + 1
    消費URL = {x["URL"] for x in 候補[:束["消費位置"]]}
    if any(type(u) is not str or 公開URL(u) != u or u not in 消費URL for u in 試行URL):
        raise ValueError("本文試行と消費候補の対応不一致")
    if [x.get("検索語") for x in d["試行"] if x.get("作用") == "検索"] != 検索語:
        raise ValueError("検索来歴と候補の検索語が不一致")
    refs = {r.識別子: r for r in 成果.参照}
    metadata = {x["参照ID"]: x for x in d["資料"]}
    if len(refs) != len(成果.参照) or len(metadata) != len(d["資料"]) or len(refs) != d["資料数"] or set(refs) != set(metadata):
        raise ValueError("継続原本文と資料IDの対応不一致")
    本文印, 最終先 = set(), set()
    for key, ref in refs.items():
        m = metadata[key]
        doc = _本文記録を復元(m, ref.本文)
        identity = "web:" + _記録印文字(doc.最終URL + "\n" + doc.本文SHA256)[:24]
        if (key != identity or ref.URL != doc.最終URL or ref.出典 != "公開HTTPS本文"
                or ref.公開時刻 is not None or m["公開時刻"] is not None
                or doc.要求URL not in 試行URL or m["検索語"] not in 検索語
                or doc.本文SHA256 in 本文印 or doc.最終URL in 最終先):
            raise ValueError("継続原本文・来歴・重複の不一致")
        本文印.add(doc.本文SHA256)
        最終先.add(doc.最終URL)
    covered = set()
    for p in d["抜粋"]:
        if type(p) is not dict or set(p) != {"参照ID", "開始", "終了", "本文", "一致語"}:
            raise ValueError("継続抜粋の欄不一致")
        text, a, b = refs[p["参照ID"]].本文, p["開始"], p["終了"]
        if type(a) is not int or type(b) is not int or not 0 <= a < b <= len(text) or text[a:b] != p["本文"]:
            raise ValueError("継続抜粋の原文範囲不一致")
        hits = [w for w in 要求.必要語 if _照合語(w) in _照合語(p["本文"])]
        if p["一致語"] != hits:
            raise ValueError("継続抜粋の一致語不正")
        covered.update(hits)
    if (covered != set(要求.必要語) or 成果.本文 != ("" if 不成立 else "\n\n".join(p["本文"] for p in d["抜粋"]))
            or sum(len(p["本文"]) for p in d["抜粋"]) > 要求.最大抜粋文字数
            or 成果.根拠 != tuple(refs)):
        raise ValueError("継続原本文・必要語・累積抜粋上限不一致")
    return deepcopy(束)


def _記録印文字(文):
    from hashlib import sha256
    return sha256(文.encode("utf-8")).hexdigest()


def 継続進展を検査(前, 次, 要求):
    旧, 新 = 継続束を読む(前, 要求), 継続束を読む(次, 要求)
    if (新["親記録SHA256"] != 前.データ["記録SHA256"]
            or any(旧[k] != 新[k] for k in ("版", "要求SHA256", "候補", "検索済み語"))
            or 新["消費位置"] <= 旧["消費位置"]
            or 新["試行済みURL"][:len(旧["試行済みURL"])] != 旧["試行済みURL"]
            or 次.データ["資料数"] != 前.データ["資料数"] + 1
            or 次.データ["本文取得数"] <= 前.データ["本文取得数"]
            or 次.参照[:len(前.参照)] != 前.参照
            or 次.データ.get("検索来歴") != 前.データ.get("検索来歴")
            or any(次.データ[k][:len(前.データ[k])] != 前.データ[k] for k in ("資料", "抜粋", "試行"))
            or any(x.get("作用") == "検索" for x in 次.データ["試行"][len(前.データ["試行"]):])):
        raise ValueError("取得継続に新規原本文の進展・原値保持がありません")
    return True


def 継続不成立を検査(前, 次, 要求):
    """旧本文は保持したまま候補を消費し終えた実不成立だけを検証する。"""
    旧 = 継続束を読む(前, 要求)
    新 = _継続記録を読む(次, 要求, 不成立=True)
    追加試行 = 次.データ["試行"][len(前.データ["試行"]):]
    新URL = 新["試行済みURL"][len(旧["試行済みURL"]):]
    if (新["親記録SHA256"] != 前.データ["記録SHA256"]
            or any(旧[k] != 新[k] for k in ("版", "要求SHA256", "候補", "検索済み語"))
            or not 旧["消費位置"] < 新["消費位置"] == len(新["候補"])
            or 新["試行済みURL"][:len(旧["試行済みURL"])] != 旧["試行済みURL"]
            or 次.データ["本文取得数"] - 前.データ["本文取得数"] != len(新URL)
            or 次.参照 != 前.参照 or 次.根拠 != 前.根拠
            or 次.データ.get("検索来歴") != 前.データ.get("検索来歴")
            or any(次.データ[k] != 前.データ[k] for k in ("資料", "抜粋", "資料数"))
            or 次.データ["試行"][:len(前.データ["試行"])] != 前.データ["試行"]
            or len(追加試行) != 新["消費位置"] - 旧["消費位置"]
            or any(x.get("作用") not in ("重複URL", "重複資料", "本文非採用", "本文取得失敗") for x in 追加試行)
            or [x.get("URL") for x in 追加試行 if x.get("作用") != "重複URL"] != 新URL):
        raise ValueError("継続不成立の候補消費・旧本文・原値が不一致")
    消費済み = set(旧["試行済みURL"]) | {x["最終URL"] for x in 前.データ["資料"]}
    for 候補, 試行 in zip(旧["候補"][旧["消費位置"]:新["消費位置"]], 追加試行):
        URL = 候補["URL"]
        if (試行.get("URL") != URL
                or (試行["作用"] == "重複URL") != (URL in 消費済み)):
            raise ValueError("継続不成立の原候補順序・実重複が不一致")
        消費済み.add(URL)
    return True
