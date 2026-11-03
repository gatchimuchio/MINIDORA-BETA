"""ブラウザへ渡す公開資源を、明示URL・GET・既存の検査済みHTTPS経路へ限定する。"""
from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from urllib.parse import urlsplit

from .公開本文取得 import 公開URL, 公開本文取得器

ブラウザ版 = "MINIDORA-ブラウザ閲覧-v0.1"


class ブラウザ境界違反(ValueError):
    pass


def 正規URL(value: str) -> str:
    normalized = 公開URL(value)
    if urlsplit(value).fragment or value != normalized:
        raise ブラウザ境界違反("正規化済み公開HTTPS URLを指定する。断片・暗黙書換えは未対応")
    return normalized


@dataclass(frozen=True, slots=True)
class ブラウザ資源:
    URL: str
    内容種別: str
    本体: bytes
    原CSP: str = ""


class 公開資源供給器:
    """Cookie・Authorizationを引き継がず、公開IPへ固定した既存取得器を利用する。"""
    def __init__(self):
        self._取得器 = 公開本文取得器(timeout=8, 最大バイト数=1_000_000, 最大転送数=0)

    def __call__(self, url: str) -> ブラウザ資源:
        url = 正規URL(url)
        status, headers, body = self._取得器._一回取得(url)
        if status != 200:
            raise ブラウザ境界違反("HTTP状態不成立。転送は追従しない:" + str(status))
        if headers.get("content-encoding", "identity").lower() not in ("", "identity"):
            raise ブラウザ境界違反("圧縮応答は未対応")
        if "content-disposition" in headers:
            raise ブラウザ境界違反("ダウンロード応答は未対応")
        return ブラウザ資源(url, headers.get("content-type", ""), body, headers.get("content-security-policy", ""))


class ブラウザ通信境界:
    'ブラウザ要求を継続しない。検査した応答だけを経路.fulfillで供給する。'
    def __init__(self, 許可URL: tuple[str, ...], 供給器=None):
        if type(許可URL) is not tuple or not 1 <= len(許可URL) <= 64:
            raise ブラウザ境界違反("許可URLは1〜64件のtuple")
        approved = tuple(正規URL(u) for u in 許可URL)
        if len(set(approved)) != len(approved) or len({urlsplit(u).netloc for u in approved}) != 1:
            raise ブラウザ境界違反("許可URLの重複・異なる生成元")
        self.許可URL = approved
        self._供給 = 供給器 if 供給器 is not None else 公開資源供給器()
        self.履歴: list[dict] = []
        self.予定文書: str | None = None
        self.受信量 = 0
        self.失敗 = ""

    def 遮断(self, reason: str):
        self.失敗 = self.失敗 or reason

    def 応答(self, URL: str, 方法: str, 種別: str, *, 主文書: bool = False) -> ブラウザ資源:
        if self.失敗:
            raise ブラウザ境界違反(self.失敗)
        if len(self.履歴) >= 64:
            self.遮断("資源要求数上限")
            raise ブラウザ境界違反(self.失敗)
        row = {"URL": URL, "方法": 方法, "種別": 種別, "状態": "拒否"}
        self.履歴.append(row)
        try:
            if 方法 != "GET" or type(主文書) is not bool:
                raise ブラウザ境界違反("GET以外の要求は禁止")
            url = 正規URL(URL)
            if url not in self.許可URL:
                raise ブラウザ境界違反("未許可URLへの要求")
            if 種別 == "document" and (not 主文書 or url != self.予定文書):
                raise ブラウザ境界違反("未指示のページ移動・子フレーム")
            allowed = {"document": ("text/html",),
                       "script": ("text/javascript", "application/javascript"),
                       "stylesheet": ("text/css",),
                       "fetch": ("application/json", "text/plain"),
                       "xhr": ("application/json", "text/plain")}
            if 種別 not in allowed:
                raise ブラウザ境界違反("未対応の資源種別")
            asset = self._供給(url)
            if (type(asset) is not ブラウザ資源 or asset.URL != url or type(asset.本体) is not bytes
                    or type(asset.内容種別) is not str or len(asset.内容種別) > 256
                    or any(c in asset.内容種別 for c in "\r\n")
                    or type(asset.原CSP) is not str or len(asset.原CSP) > 8192
                    or any(c in asset.原CSP for c in "\r\n")):
                raise ブラウザ境界違反("資源供給の契約違反")
            mime = asset.内容種別.split(";", 1)[0].strip().lower()
            if mime not in allowed[種別]:
                raise ブラウザ境界違反("要求と内容種別の不一致")
            if not 0 < len(asset.本体) <= 1_000_000 or self.受信量 + len(asset.本体) > 4_000_000:
                raise ブラウザ境界違反("資源サイズ・累積受信量上限")
            # 初版はUTF-8のみ。ブラウザの置換復号で失敗を隠さない。
            parts = asset.内容種別.lower().split(";")[1:]
            if parts and any(p.strip() not in ("charset=utf-8", 'charset="utf-8"') for p in parts):
                raise ブラウザ境界違反("資源文字コードはUTF-8のみ")
            asset.本体.decode("utf-8", "strict")
            self.受信量 += len(asset.本体)
            row.update(状態="供給", バイト数=len(asset.本体), SHA256=sha256(asset.本体).hexdigest())
            return asset
        except Exception as exc:
            reason = str(exc) if isinstance(exc, ブラウザ境界違反) else "資源取得故障:" + type(exc).__name__
            row["理由"] = reason
            self.遮断(reason)
            raise ブラウザ境界違反(reason) from exc
