"""実Chromiumの描画・限定表示操作・明示リンク移動・表取得。外部作用はGET資源供給だけ。"""
from __future__ import annotations

from collections.abc import Callable
from copy import deepcopy
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from hashlib import sha256
import json
import time
from uuid import uuid4

from .ブラウザ通信 import ブラウザ版, ブラウザ境界違反, ブラウザ通信境界, 正規URL
from .製品版.型 import 能力結果, 参照資料


def _符号(value) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")


def _印(value) -> str:
    return sha256(_符号(value)).hexdigest()


def _文字(value, limit=256):
    if type(value) is not str or not value.strip() or len(value) > limit or any(ord(c) < 32 for c in value):
        raise ブラウザ境界違反("対象ID・期待表示の型不正")


@dataclass(frozen=True, slots=True)
class ブラウザ工程:
    操作: str
    対象ID: str
    期待表示: str | None = None


@dataclass(frozen=True, slots=True)
class ブラウザ要求:
    開始URL: str
    許可URL: tuple[str, ...]
    工程: tuple[ブラウザ工程, ...]
    待機ミリ秒: int = 3000

    def 検証(self):
        正規URL(self.開始URL)
        ブラウザ通信境界(self.許可URL)
        if self.開始URL not in self.許可URL:
            raise ブラウザ境界違反("開始URLが未許可")
        if type(self.工程) is not tuple or not 1 <= len(self.工程) <= 16:
            raise ブラウザ境界違反("工程は1〜16件")
        if type(self.待機ミリ秒) is not int or not 100 <= self.待機ミリ秒 <= 10000:
            raise ブラウザ境界違反("待機上限が不正")
        for step in self.工程:
            if type(step) is not ブラウザ工程 or step.操作 not in ("表示待機", "表示操作", "リンク移動", "本文取得", "表取得"):
                raise ブラウザ境界違反("未知のブラウザ操作")
            _文字(step.対象ID, 128)
            if step.操作 in ("表示操作", "リンク移動") and step.期待表示 is None:
                raise ブラウザ境界違反("操作対象の期待表示が必要")
            if step.期待表示 is not None:
                _文字(step.期待表示, 2048)
        if self.工程[-1].操作 not in ("本文取得", "表取得"):
            raise ブラウザ境界違反("最終工程には取得を明示する")


# JavaScriptは固定したDOM操作コード。呼出側の文字列をコードとして評価しない。
_観測JS = r"""() => {
 const 文書 = document.documentElement.outerHTML;
 if (文書.length > 500000 || document.querySelectorAll('*').length > 5000) throw Error('DOM上限');
 const 可視 = e => !!e.getClientRects().length && !['hidden','collapse'].includes(getComputedStyle(e).visibility) && getComputedStyle(e).display !== 'none' && !e.closest('[hidden]');
 const 要素 = [];
 for (const e of document.querySelectorAll('[id]')) {
  if (!可視(e)) continue;
  const 本文 = e.innerText || '';
  if (本文.length > 100000 || 要素.length >= 256) throw Error('要素上限');
  const 行 = {ID:e.id, タグ:e.tagName.toLowerCase(), 本文, HTML:e.outerHTML};
  if (e instanceof HTMLAnchorElement) Object.assign(行, {リンク:e.href, ダウンロード:e.hasAttribute('download'), 別窓:!!e.target && e.target !== '_self'});
  if (e instanceof HTMLButtonElement) Object.assign(行, {ボタン型:e.type, フォーム:!!e.form, 無効:e.disabled});
  if (e instanceof HTMLTableElement) {
   if (e.rows.length > 128 || e.querySelectorAll('table').length) throw Error('表構造上限');
   行.表 = Array.from(e.rows).map(r => {
    if (!可視(r) || r.cells.length > 32) throw Error('隠れた行・列上限');
    return Array.from(r.cells).map(c => {
     if (!可視(c)) throw Error('隠れたセル');
     return {本文:c.innerText, 見出し:c.tagName==='TH', 行結合:c.rowSpan, 列結合:c.colSpan};
    });
   });
  }
  要素.push(行);
 }
 return {URL:location.href, 題名:document.title, DOM:文書, 本文:document.body ? document.body.innerText : '', 要素,
         除外:{子フレーム:document.querySelectorAll('iframe,frame').length, 図:document.querySelectorAll('canvas,svg').length}};
}"""


def 描画を観測(page) -> dict:
    資料 = page.evaluate(_観測JS)
    if len(_符号(資料)) > 1_000_000:
        raise ブラウザ境界違反("描画記録サイズ上限")
    資料["観測SHA256"] = _印(資料)
    return 資料


def _要素(observation, key, expected=None):
    rows = [r for r in observation["要素"] if r["ID"] == key]
    if len(rows) != 1:
        raise ブラウザ境界違反("可視の対象IDが一意でない")
    row = rows[0]
    if expected is not None and row["本文"] != expected:
        raise ブラウザ境界違反("対象の表示が期待値と不一致")
    return row


def 表を配列(row: dict) -> list[list[str]]:
    if row["タグ"] != "table" or not row.get("表"):
        raise ブラウザ境界違反("非空のHTML表が必要")
    width = len(row["表"][0])
    if not width or any(len(r) != width for r in row["表"]):
        raise ブラウザ境界違反("不揃いな表を補完しない")
    if any(c["行結合"] != 1 or c["列結合"] != 1 for r in row["表"] for c in r):
        raise ブラウザ境界違反("結合セルは未対応。平坦化しない")
    return [[c["本文"] for c in r] for r in row["表"]]


def 表示操作を行う(page, observation: dict, key: str, expected: str):
    row = _要素(observation, key, expected)
    if row["タグ"] != "button" or row["ボタン型"] != "button" or row["フォーム"] or row["無効"]:
        raise ブラウザ境界違反("フォーム外の有効なtype=buttonだけを表示操作する")
    # 同じJavaScriptタスク内でDOM一致を検査して対象へclickを発火する。
    page.evaluate(r"""a => {
      if (document.documentElement.outerHTML !== a.DOM || location.href !== a.URL) throw Error('旧DOM');
      const 対象 = Array.from(document.querySelectorAll('[id]')).filter(e => e.id === a.ID);
      if (対象.length !== 1) throw Error('曖昧ID');
      const e = 対象[0];
      if (!(e instanceof HTMLButtonElement) || e.type!=='button' || e.form || e.disabled || !e.getClientRects().length || e.innerText!==a.期待) throw Error('対象変更');
      e.click();
    }""", {"DOM": observation["DOM"], "URL": observation["URL"], "ID": key, "期待": expected})


class ブラウザ閲覧器:
    """1要求ごとに新しいブラウザを閉じる。共有プロファイル・ログイン・汎用入力を持たない。"""
    def __init__(self, *, 実行ファイル: str | None = None, 試験供給器=None):
        self._実行ファイル, self._試験供給 = 実行ファイル, 試験供給器

    def 実行(self, 要求: ブラウザ要求, *, 外部読取許可: bool = False,
             停止要求: Callable[[], bool] | None = None) -> 能力結果:
        network, steps, final, current = None, [], None, None
        def stop():
            if 停止要求 is not None:
                状態 = 停止要求()
                if type(状態) is not bool or 状態:
                    raise ブラウザ境界違反("停止要求・停止判定不正")
            if network is not None and network.失敗:
                raise ブラウザ境界違反(network.失敗)
        try:
            if 外部読取許可 is not True or type(要求) is not ブラウザ要求:
                raise ブラウザ境界違反("明示した閲覧要求と外部読取許可が必要")
            要求.検証(); stop()
            # 任意依存。未導入時に別実装へフォールバックして成功扱いしない。
            from playwright.sync_api import sync_playwright
            network = ブラウザ通信境界(要求.許可URL, self._試験供給)
            with sync_playwright() as driver:
                browser = driver.chromium.launch(headless=True, executable_path=self._実行ファイル,
                    args=["--disable-background-networking", "--force-webrtc-ip-handling-policy=disable_non_proxied_udp"])
                try:
                    文脈 = browser.new_context(offline=True, service_workers="block", accept_downloads=False,
                                                  viewport={"width": 1280, "height": 800}, locale="ja-JP")
                    文脈.set_default_timeout(要求.待機ミリ秒)
                    文脈.route_web_socket("**/*", lambda ws: (network.遮断("WebSocketは禁止"), ws.close()))
                    page = 文脈.new_page()
                    文脈.on("page", lambda extra: (network.遮断("別窓は禁止"), extra.close()))
                    page.on("download", lambda _: network.遮断("ダウンロードは禁止"))
                    page.on("dialog", lambda dialog: (network.遮断("ダイアログ操作は未対応"), dialog.dismiss()))
                    def supply(経路):
                        try:
                            request = 経路.request
                            asset = network.応答(request.url, request.method, request.resource_type,
                                                 主文書=request.is_navigation_request() and request.frame == page.main_frame)
                            # 文書の外部作用経路を閉じる。厳密なOS隔離やブラウザ脆弱性対策ではない。
                            csp = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src data:; frame-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"
                            if asset.原CSP:
                                csp = asset.原CSP + ", " + csp
                            経路.fulfill(status=200, headers={"content-type": asset.内容種別.split(";", 1)[0] + "; charset=utf-8",
                                "content-security-policy": csp, "x-content-type-options": "nosniff"}, body=asset.本体)
                        except Exception:
                            network.遮断(network.失敗 or "資源供給故障")
                            経路.abort()
                    文脈.route("**/*", supply)
                    def navigate(url):
                        stop()
                        if url not in 要求.許可URL:
                            raise ブラウザ境界違反("未許可のページ移動")
                        network.予定文書 = url
                        try:
                            response = page.goto(url, wait_until="domcontentloaded", timeout=要求.待機ミリ秒)
                            if response is None or response.status != 200 or page.url != url:
                                raise ブラウザ境界違反("移動結果が要求と不一致")
                        finally:
                            network.予定文書 = None
                        stop()
                    navigate(要求.開始URL)
                    for step in 要求.工程:
                        stop(); before = 描画を観測(page)
                        if step.操作 == "表示待機":
                            end = time.monotonic() + 要求.待機ミリ秒 / 1000
                            while True:
                                stop(); current = 描画を観測(page)
                                try:
                                    _要素(current, step.対象ID, step.期待表示)
                                    break
                                except ブラウザ境界違反:
                                    if time.monotonic() >= end:
                                        raise ブラウザ境界違反("期待表示の待機時間上限")
                                    page.wait_for_timeout(25)
                        elif step.操作 == "表示操作":
                            表示操作を行う(page, before, step.対象ID, step.期待表示)
                        elif step.操作 == "リンク移動":
                            row = _要素(before, step.対象ID, step.期待表示)
                            if row["タグ"] != "a" or row["ダウンロード"] or row["別窓"]:
                                raise ブラウザ境界違反("同じ窓の通常リンクが必要")
                            if 描画を観測(page) != before:
                                raise ブラウザ境界違反("リンク検査後にDOMが変化")
                            navigate(正規URL(row["リンク"]))
                        else:
                            row = _要素(before, step.対象ID, step.期待表示)
                            body = row["本文"] if step.操作 == "本文取得" else json.dumps(表を配列(row), ensure_ascii=False)
                            if not body.strip():
                                raise ブラウザ境界違反("取得本文が空")
                            final = {"本文": body, "操作": step.操作, "対象ID": step.対象ID,
                                     "観測SHA256": before["観測SHA256"]}
                        # 未処理の通信イベントを検査してから状態を確定する。
                        page.wait_for_timeout(0); stop()
                        current = 描画を観測(page)
                        steps.append({"工程": asdict(step), "前観測": before["観測SHA256"], "後観測": current["観測SHA256"]})
                    stop()
                    if final is None or final["観測SHA256"] != current["観測SHA256"]:
                        raise ブラウザ境界違反("取得後のDOM変化。旧値を採用しない")
                    version = browser.version
                finally:
                    browser.close()
            stop()
            資料 = {"版": ブラウザ版, "要求": asdict(要求), "取得": final, "最終観測": current,
                    "工程記録": steps, "資源記録": deepcopy(network.履歴), "ブラウザ版": version,
                    "取得時刻": datetime.now(timezone.utc).isoformat(), "実行ID": uuid4().hex,
                    "供給区分": "試験供給" if self._試験供給 is not None else "公開HTTPS実取得",
                    "限界": "観測時点の描画と明示操作。原HTML同一性・意味理解・事実性・全機能再現ではない"}
            資料["記録SHA256"] = _印(資料)
            if len(_符号(資料)) > 2_000_000:
                raise ブラウザ境界違反("閲覧記録上限")
            ref = 参照資料("browser:" + 資料["実行ID"], current["題名"], "Chromium描画観測",
                           current["URL"], 本文=current["本文"])
            return 能力結果(True, final["本文"], 参照=(ref,), データ=資料)
        except Exception as exc:
            reason = str(exc) if isinstance(exc, ブラウザ境界違反) else type(exc).__name__
            return 能力結果(False, "", 保留理由="ブラウザ閲覧不成立:" + reason,
                データ={"資源記録": deepcopy(network.履歴) if network else [], "工程記録": steps,
                        "診断": reason})


def ブラウザ記録整合(結果: 能力結果) -> bool:
    """記録と抽出の内部整合。再訪問・JavaScript再実行・署名ではない。"""
    try:
        if not isinstance(結果, 能力結果) or 結果.成立 is not True or 結果.保留理由:
            return False
        資料 = deepcopy(結果.データ); stamp = 資料.pop("記録SHA256")
        view = deepcopy(資料["最終観測"]); view_hash = view.pop("観測SHA256")
        if stamp != _印(資料) or view_hash != _印(view) or 資料["取得"]["観測SHA256"] != view_hash:
            return False
        row = _要素(view, 資料["取得"]["対象ID"])
        expected = row["本文"] if 資料["取得"]["操作"] == "本文取得" else json.dumps(表を配列(row), ensure_ascii=False)
        return 結果.本文 == expected == 資料["取得"]["本文"] and 資料["版"] == ブラウザ版
    except (ValueError, TypeError, KeyError):
        return False
