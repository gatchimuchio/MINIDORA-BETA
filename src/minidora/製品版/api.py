from __future__ import annotations
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import base64, binascii, hashlib, hmac, json, os, re, mimetypes, socket
from collections import deque
from threading import Lock
from time import monotonic
from pathlib import Path
from urllib.parse import urlparse
from .製品チャット import 製品ミニドラ

API版 = "MINIDORA-PRODUCT-API-v1"

def HTTPBasic許可値(設定=None, *, 必須=False):
    """Cloudの審査入口は固定usernameと強いpasswordのSHA256で認証する。"""
    設定 = os.environ if 設定 is None else 設定
    名前 = 設定.get("MINIDORA_HTTP_BASIC_USERNAME")
    ハッシュ = 設定.get("MINIDORA_HTTP_BASIC_PASSWORD_SHA256")
    if 名前 is None and ハッシュ is None and not 必須:
        return None
    if (type(名前) is not str or not re.fullmatch(r"[A-Za-z0-9._-]{1,64}", 名前)
            or type(ハッシュ) is not str or not re.fullmatch(r"[a-f0-9]{64}", ハッシュ)):
        raise ValueError("HTTP Basicのusernameとpassword SHA256を両方明示してください")
    return 名前.encode("ascii"), ハッシュ

def CloudRun許可値():
    """配備で実測した外部Host/Originのみ許可する。"""
    ホスト = frozenset(x.strip() for x in os.getenv("MINIDORA_CLOUD_HOSTS", "").split(",") if x.strip())
    生成元 = frozenset(x.strip() for x in os.getenv("MINIDORA_CLOUD_ORIGINS", "").split(",") if x.strip())
    if not ホスト or not 生成元:
        raise ValueError("Cloud RunのHostとOriginを明示してください")
    if any(len(x) > 253 or not re.fullmatch(r"[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?", x)
           or any(not 部分 or len(部分) > 63 or 部分.startswith("-") or 部分.endswith("-")
                  for 部分 in x.split(".")) for x in ホスト):
        raise ValueError("Cloud RunのHostはポートなしのDNS名を指定してください")
    if any(x != "https://" + urlparse(x).netloc or urlparse(x).netloc not in ホスト for x in 生成元):
        raise ValueError("Cloud RunのOriginは許可Hostと一致するhttps生成元を指定してください")
    return ホスト, 生成元

def _json(handler: BaseHTTPRequestHandler, status: int, body: dict):
    raw = json.dumps(body, ensure_ascii=False).encode("utf-8")
    handler.send_response(status)
    handler.send_header("Content-Type","application/json; charset=utf-8")
    handler.send_header("Content-Length",str(len(raw)))
    handler.send_header("Cache-Control","no-store")
    if status == 401:
        handler.send_header("WWW-Authenticate", 'Basic realm="MINIDORA Judge", charset="UTF-8"')
    if status >= 400:
        # 未読の不正POST本体を、次のHTTP要求として扱わせない。
        handler.close_connection = True
        handler.send_header("Connection", "close")
    if not getattr(handler.server, "同一生成元限定", False) and not getattr(handler.server, "CloudRun境界", False):
        handler.send_header("Access-Control-Allow-Origin", os.getenv("MINIDORA_CORS_ORIGIN","*"))
    handler.end_headers(); handler.wfile.write(raw)
    if status >= 400:
        handler._拒否接続を終了()

class APIHandler(BaseHTTPRequestHandler):
    server_version = "MINIDORA-Product/1"
    protocol_version = "HTTP/1.1"
    max_body = 256_000

    def setup(self):
        super().setup()
        self.connection.settimeout(15)

    def parse_request(self):
        if not super().parse_request():
            return False
        # 未対応の方法も含め、画面・API・healthの全要求で製品処理より先に認証する。
        return self._入口許可() and self._利用者認証()

    def _利用者認証(self):
        許可値 = getattr(self.server, "HTTPBasic許可値", None)
        if 許可値 is None and not getattr(self.server, "CloudRun境界", False):
            return True
        ヘッダ = self.headers.get_all("Authorization", [])
        if 許可値 is not None and len(ヘッダ) == 1 and len(ヘッダ[0]) <= 1024:
            部分 = ヘッダ[0].split(" ")
            if len(部分) == 2 and 部分[0].casefold() == "basic":
                try:
                    名前, パスワード = base64.b64decode(部分[1], validate=True).decode("utf-8").split(":", 1)
                    名前一致 = hmac.compare_digest(名前.encode("utf-8"), 許可値[0])
                    ハッシュ一致 = hmac.compare_digest(hashlib.sha256(パスワード.encode("utf-8")).hexdigest(), 許可値[1])
                    if 名前一致 & ハッシュ一致 & bool(パスワード):
                        return True
                except (ValueError, UnicodeError, binascii.Error):
                    pass
        _json(self, 401, {"error": "authentication_required"})
        return False

    @property
    def app(self) -> 製品ミニドラ:
        return self.server.app  # type: ignore[attr-defined]

    def log_message(self, fmt, *args):
        if os.getenv("MINIDORA_HTTP_LOG","1") != "0": super().log_message(fmt,*args)

    def _拒否接続を終了(self):
        """最終応答をhalf-closeで届け、未読入力によるTCP resetを避ける。"""
        try:
            self.wfile.flush()
            self.connection.shutdown(socket.SHUT_WR)
            # 受付上限は256,000バイトのまま。Cloud Run HTTP/1の最大量まで、
            # 意味処理せず固定chunkで破棄し、絶対期限で遅延送信を打ち切る。
            残量, 期限 = 32 * 1024 * 1024, monotonic() + 2
            while 残量 > 0:
                残時間 = 期限 - monotonic()
                if 残時間 <= 0: break
                self.connection.settimeout(残時間)
                塊 = self.rfile.read1(min(65_536, 残量))
                if not 塊: break
                残量 -= len(塊)
        except OSError:
            # 切断・期限到達は接続終了。要求の採用や次要求の解釈は行わない。
            pass

    def _入口許可(self):
        """認証と独立した既存のHost/Origin境界。"""
        if getattr(self.server, "CloudRun境界", False):
            ホスト群 = self.headers.get_all("Host", [])
            生成元群 = self.headers.get_all("Origin", [])
            if (len(ホスト群) != 1 or ホスト群[0] not in self.server.許可ホスト
                    or len(生成元群) > 1 or (生成元群 and 生成元群[0] not in self.server.許可生成元)):
                _json(self, 403, {"error": "cloud_origin_required"})
                return False
            return True
        if not getattr(self.server, "同一生成元限定", False): return True
        port = self.server.server_address[1]
        allowed = {f"127.0.0.1:{port}", f"localhost:{port}"}
        host = self.headers.get("Host", "")
        origin = self.headers.get("Origin")
        if host not in allowed or (origin is not None and origin not in {"http://" + h for h in allowed}):
            _json(self, 403, {"error": "local_origin_required"})
            return False
        return True

    def do_OPTIONS(self):
        if not self._入口許可(): return
        self.send_response(204)
        if not getattr(self.server, "同一生成元限定", False) and not getattr(self.server, "CloudRun境界", False):
            self.send_header("Access-Control-Allow-Origin", os.getenv("MINIDORA_CORS_ORIGIN","*"))
        self.send_header("Access-Control-Allow-Headers","Content-Type")
        self.send_header("Access-Control-Allow-Methods","GET,POST,OPTIONS")
        self.end_headers()

    def _static(self, path: str):
        base = Path(__file__).resolve().parent / "web"
        rel = "index.html" if path == "/" else path.removeprefix("/static/")
        target = (base / rel).resolve()
        try:
            target.relative_to(base.resolve())
        except ValueError:
            return _json(self,403,{"error":"forbidden"})
        if not target.is_file():
            return _json(self,404,{"error":"not_found"})
        raw = target.read_bytes(); mime = mimetypes.guess_type(target.name)[0] or "application/octet-stream"
        self.send_response(200); self.send_header("Content-Type",mime); self.send_header("Content-Length",str(len(raw))); self.send_header("Cache-Control","no-cache"); self.end_headers(); self.wfile.write(raw)

    def do_GET(self):
        if not self._入口許可(): return
        path = urlparse(self.path).path
        if path == "/" or path.startswith("/static/"): return self._static(path)
        if path == "/health": return _json(self,200,{"ok":True,"service":"MINIDORA Product","api_version":API版})
        if path == "/api/capabilities": return _json(self,200,{"capabilities":list(self.app.能力一覧()), "request_external_read":bool(getattr(self.app, "要求単位外部読取", False)), "公開取得方針":getattr(self.app, "公開取得方針", None)})
        m = re.fullmatch(r"/api/trace/([a-f0-9]{32})", path)
        if m:
            r = self.app.監査台帳.取得(m.group(1))
            if not r: return _json(self,404,{"error":'追跡_not_found'})
            if getattr(self.server, "CloudRun境界", False):
                from .監査 import 安全監査仕様版
                if getattr(r, "仕様版", None) != 安全監査仕様版:
                    return _json(self,409,{"error":"安全表現のない旧監査は公開できません"})
            return _json(self,200,{'追跡':r.辞書化(),"valid":self.app.監査台帳.検証(m.group(1))})
        return _json(self,404,{"error":"not_found"})

    def do_POST(self):
        if not self._入口許可(): return
        if urlparse(self.path).path != "/api/chat": return _json(self,404,{"error":"not_found"})
        if self.headers.get_all("Transfer-Encoding", []) or len(self.headers.get_all("Content-Length", [])) != 1:
            return _json(self,400,{"error":"invalid_content_length"})
        if getattr(self.server, "CloudRun境界", False):
            if self.headers.get_content_type() != "application/json":
                return _json(self,415,{"error":"json_content_type_required"})
            with self.server.受付ロック:
                現在 = monotonic()
                while self.server.受付時刻 and self.server.受付時刻[0] <= 現在 - 60:
                    self.server.受付時刻.popleft()
                if len(self.server.受付時刻) >= 30:
                    return _json(self,429,{"error":"rate_limit"})
                self.server.受付時刻.append(現在)
        try: length = int(self.headers.get("Content-Length","0"))
        except ValueError: return _json(self,400,{"error":"invalid_content_length"})
        if length <= 0 or length > self.max_body: return _json(self,413,{"error":"invalid_body_size"})
        try: payload = json.loads(self.rfile.read(length).decode("utf-8"))
        except Exception: return _json(self,400,{"error":"invalid_json"})
        if not isinstance(payload, dict): return _json(self,400,{"error":"json_object_required"})
        if not isinstance(payload.get("message", ""), str) or not isinstance(payload.get("session_id", "default"), str):
            return _json(self,400,{"error":"string_fields_required"})
        message = payload.get("message","").strip(); session = payload.get("session_id","default").strip() or "default"
        if not message: return _json(self,400,{"error":"message_required"})
        if len(session) > 128 or (getattr(self.server, "CloudRun境界", False) and len(message) > 8192):
            return _json(self,400,{"error":"input_limit"})
        条件 = {"セッションID": session}
        if "external_read" in payload:
            if type(payload["external_read"]) is not bool or not getattr(self.app, "要求単位外部読取", False):
                return _json(self,400,{"error":"invalid_external_read_permission"})
            条件["外部読取許可"] = payload["external_read"]
        try: response = self.app.応答(message, **条件)
        except (ValueError, TypeError): return _json(self,400,{"error":"invalid_input"})
        except Exception:
            self.log_error("製品実行に失敗しました")
            return _json(self,500,{"error":"execution_failed"})
        return _json(self,200,response.辞書化())

def serve(app: 製品ミニドラ, host: str = "0.0.0.0", port: int | None = None, *, 同一生成元限定=False, CloudRun境界=False):
    if type(同一生成元限定) is not bool: raise ValueError("生成元制限はbool")
    if type(CloudRun境界) is not bool or (CloudRun境界 and 同一生成元限定):
        raise ValueError("Cloud Run境界とローカル限定は別の配備です")
    許可値 = CloudRun許可値() if CloudRun境界 else None
    認証値 = HTTPBasic許可値(必須=CloudRun境界)
    p = int(port or os.getenv("PORT","8080"))
    server = ThreadingHTTPServer((host,p), APIHandler); server.app = app  # type: ignore[attr-defined]
    server.同一生成元限定 = 同一生成元限定
    server.CloudRun境界 = CloudRun境界
    server.HTTPBasic許可値 = 認証値
    if 許可値 is not None:
        server.許可ホスト, server.許可生成元 = 許可値
        server.受付ロック, server.受付時刻 = Lock(), deque()
    server.serve_forever()
