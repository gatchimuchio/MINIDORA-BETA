"""配備時の固定設定で取得器を組み立てる。検索の要否は既存HDSが決める。"""
from __future__ import annotations

from datetime import datetime, timezone
import re

from .検索送信予算 import 固定保存先, 検索送信予算設定


class Google起動設定不正(ValueError):
    """設定値やSDKの自由文を表示しない起動停止。"""


def _必須値(設定, 名称):
    値 = 設定.get(名称)
    if (type(値) is not str or not 値 or 値 != 値.strip()
            or len(値) > 4096 or re.search(r"[\x00-\x1f\x7f]", 値)):
        raise Google起動設定不正("未設定又は不正: " + 名称)
    return 値


def _UTC日時(設定, 名称):
    値 = _必須値(設定, 名称)
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z", 値):
        raise Google起動設定不正("秒単位UTCの設定が必要: " + 名称)
    try:
        return datetime.strptime(値, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    except ValueError:
        raise Google起動設定不正("UTC日時不正: " + 名称) from None


def Google固定設定を読む(設定):
    """値を当回入力・セッションから受け取らず、起動時に一度固定する。"""
    try:
        プロジェクト = _必須値(設定, "MINIDORA_GOOGLE_PROJECT_ID")
        アプリ = _必須値(設定, "MINIDORA_GOOGLE_SEARCH_APP_ID")
        SA = _必須値(設定, "MINIDORA_GOOGLE_RUNTIME_SERVICE_ACCOUNT")
        ホスト = tuple(_必須値(設定, "MINIDORA_GOOGLE_PUBLIC_HOSTS").split(","))
        if any(not 名称 or 名称 != 名称.strip() or "." not in 名称
               or 名称.endswith(".localhost") for 名称 in ホスト):
            raise Google起動設定不正("公開ホスト設定不正")
        上限値 = _必須値(設定, "MINIDORA_GOOGLE_BUDGET_LIMIT")
        if not re.fullmatch(r"[1-9][0-9]{0,3}", 上限値):
            raise Google起動設定不正("累積検索上限設定不正")
        予算 = 検索送信予算設定(
            予算ID=_必須値(設定, "MINIDORA_GOOGLE_BUDGET_ID"),
            プロジェクトID=プロジェクト, アプリID=アプリ,
            保存先=固定保存先(_必須値(設定, "MINIDORA_GOOGLE_BUDGET_BUCKET"),
                          _必須値(設定, "MINIDORA_GOOGLE_BUDGET_OBJECT")),
            開始UTC=_UTC日時(設定, "MINIDORA_GOOGLE_BUDGET_START_UTC"),
            終了UTC=_UTC日時(設定, "MINIDORA_GOOGLE_BUDGET_END_UTC"),
            累積上限=int(上限値))
        if (not re.fullmatch(r"[a-z][a-z0-9-]{4,28}[a-z0-9]@" + re.escape(プロジェクト)
                            + r"\.iam\.gserviceaccount\.com", SA)):
            raise Google起動設定不正("同一プロジェクトの実行SA設定が必要")
        return 予算, ホスト, SA
    except Google起動設定不正:
        raise
    except Exception:
        raise Google起動設定不正("Google検索の固定設定不正") from None


def GoogleCloud取得器を構成(設定):
    予算設定, ホスト, SA = Google固定設定を読む(設定)
    # 通常ローカル入口にはCloud専用の依存を要求しない。構成時に通信しない。
    try:
        from .Google認証 import GoogleADC認証器
        from .Google検索 import GoogleCloud知識取得器
        from .検索送信予算 import GCS検索送信予算
        認証 = GoogleADC認証器.CloudRun(予算設定.プロジェクトID, SA)
        保存認証 = 認証.固定保存先(予算設定.保存先.バケット, 予算設定.保存先.オブジェクト)
        送信予算 = GCS検索送信予算(予算設定, 保存認証)
        return GoogleCloud知識取得器(予算設定.プロジェクトID, 予算設定.アプリID,
                                  認証, 送信予算=送信予算, 許可ホスト=ホスト, 公開条件必須=True)
    except Exception:
        raise Google起動設定不正("Google検索の起動構成不正") from None
