"""実行失敗の分類、回復契約、失敗から回復方針への決定論的射影。

分類は観測の整理であり、回復可能性の推定ではない。自動回復は開発時に
明示した契約だけを使う。資料や利用者の本文から回復権限を追加しない。
"""
from __future__ import annotations
from dataclasses import dataclass

実行回復版 = 'MINIDORA-実行回復-v0.2'

_分類 = {
    '構造未到達': '情報不足', '取得不足': '情報不足', '情報不足': '情報不足',
    '入力不足': '入力不足', '意味未確定': '意味未確定', '未解釈注記': '意味未確定',
    '前提矛盾': '前提矛盾', '能力非対応': '能力非対応', '能力不成立': '能力非対応',
    '検証失敗': '検証失敗', '入力不正': '入力不正',
    '実行環境': '実行環境', '環境失敗': '実行環境', '停止': '停止', '権限不足': '権限不足',
}
# これらを自動的に別経路へ逃がすと、要求・証拠・権限の意味が変わり得る。
_自動回復不可 = frozenset({'入力不足', '意味未確定', '前提矛盾', '入力不正', '停止', '権限不足'})
_確認必要 = frozenset({'入力不足', '意味未確定'})
_自動方式 = frozenset({'代替作用', '入力再取得', '同一作用再試行'})


def 失敗を分類(種別: str) -> str:
    return _分類.get(種別, '未分類')


@dataclass(frozen=True, slots=True)
class 回復規則:
    """失敗工程自身又は名付けられた直接入力役割の回復方法を宣言する。

    ``方式='自動'`` は既存ABI互換で、自己なら代替作用、入力役割なら
    入力再取得へ決定論的に解決する。同一作用再試行だけは明示指定し、
    実行環境系の失敗に対して有限回数だけ許可する。
    """
    失敗種別: str
    対象: str = '自己'
    入力役割: str = ''
    対象作用: tuple[str, ...] = ()
    方式: str = '自動'
    最大再試行: int = 0

    def 動作(self) -> str:
        if self.方式 == '自動':
            return '入力再取得' if self.対象 == '入力役割' else '代替作用'
        return self.方式

    def 検証(self):
        if (type(self.失敗種別) is not str or self.失敗種別 not in _分類
                or 失敗を分類(self.失敗種別) in _自動回復不可):
            raise ValueError('自動回復を許可できない失敗種別')
        if self.対象 not in ('自己', '入力役割'):
            raise ValueError('回復対象は自己又は直接入力役割')
        if type(self.入力役割) is not str or type(self.対象作用) is not tuple:
            raise ValueError('回復役割・作用の型不正')
        if type(self.方式) is not str or self.方式 not in (*_自動方式, '自動'):
            raise ValueError('回復方式が不正')
        if type(self.最大再試行) is not int or not 0 <= self.最大再試行 <= 2:
            raise ValueError('同一作用再試行上限')
        if self.対象 == '自己':
            if self.入力役割 or self.対象作用:
                raise ValueError('自己回復に別役割を指定しない')
        elif (not self.入力役割 or len(self.入力役割) > 128 or not self.対象作用
                or len(self.対象作用) > 16 or len(set(self.対象作用)) != len(self.対象作用)
                or any(type(x) is not str or not x for x in self.対象作用)):
            raise ValueError('入力回復には役割名と対象作用の許可列が必要')
        作用 = self.動作()
        if 作用 == '同一作用再試行':
            if self.対象 != '自己' or 失敗を分類(self.失敗種別) != '実行環境' or self.最大再試行 < 1:
                raise ValueError('同一作用再試行は実行環境の自己回復に限定')
        elif self.最大再試行 != 0:
            raise ValueError('再試行方式以外に再試行上限を指定しない')
        if 作用 == '入力再取得' and self.対象 != '入力役割':
            raise ValueError('入力再取得は入力役割の回復に限定')
        if 作用 == '代替作用' and self.対象 != '自己':
            raise ValueError('代替作用は自己回復に限定')
        return self


@dataclass(frozen=True, slots=True)
class 回復方針:
    """一つの失敗署名から決定した次の行為。本文からは生成しない。"""
    動作: str
    自動実行: bool
    発生目的: str
    発生作用: str
    対象目的: str = ''
    対象作用: str = ''
    禁止: tuple[tuple[str, str], ...] = ()
    契約: str = ''
    理由: str = ''
    再試行番号: int = 0
    再試行上限: int = 0


def 回復方針を決定(*, 種別: str, 分類: str, 発生目的: str, 発生作用: str,
             規則: 回復規則 | None, 再開放: tuple[str, str] | None,
             契約: str = '', 再試行済: int = 0) -> 回復方針:
    """失敗分類・開発時契約・既往試行だけから次の行為を決める。"""
    if any(type(x) is not str for x in (種別, 分類, 発生目的, 発生作用, 契約)):
        raise ValueError('回復方針入力型不正')
    if type(再試行済) is not int or 再試行済 < 0:
        raise ValueError('再試行回数不正')
    if 分類 != 失敗を分類(種別):
        raise ValueError('失敗種別と分類が不一致')
    if 規則 is None:
        作用 = '利用者確認' if 種別 in _確認必要 else '停止'
        reason = '追加入力又は意味確定が必要' if 作用 == '利用者確認' else '自動回復契約なし'
        return 回復方針(作用, False, 発生目的, 発生作用, 理由=reason)
    if type(規則) is not 回復規則:
        raise ValueError('回復規則型不正')
    規則.検証()
    作用 = 規則.動作()
    if 作用 == '同一作用再試行':
        if 再試行済 >= 規則.最大再試行:
            return 回復方針('停止', False, 発生目的, 発生作用,
                発生目的, 発生作用, 契約=契約, 理由='同一作用再試行上限',
                再試行番号=再試行済, 再試行上限=規則.最大再試行)
        return 回復方針(作用, True, 発生目的, 発生作用,
            発生目的, 発生作用, 契約=契約, 理由='明示契約による有限再試行',
            再試行番号=再試行済 + 1, 再試行上限=規則.最大再試行)
    if (type(再開放) is not tuple or len(再開放) != 2
            or any(type(x) is not str or not x for x in 再開放)):
        return 回復方針('停止', False, 発生目的, 発生作用,
            契約=契約, 理由='回復対象を計画へ一意に束縛できない')
    return 回復方針(作用, True, 発生目的, 発生作用,
        再開放[0], 再開放[1], (再開放,), 契約, '明示契約による局所再計画')
