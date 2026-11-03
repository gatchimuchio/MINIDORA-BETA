"""出力系の公開入口。importだけで表現・送達・駆動・Compilerを起動しない。"""
from importlib import import_module

_経路 = {
    'HDS出力系': ('.統合', 'HDS出力系'),
    'ファイル送達器': ('.射影', 'ファイル送達器'),
}
_契約 = ('出力束', '表現契約', '出力政策', '表現器', '表現結果', '送達器', '送達要求', '受領票', '送達記録', '出力返却')
__all__ = [*_経路, *_契約]


def __getattr__(名前):
    if 名前 in _経路:
        道, 項 = _経路[名前]
        return getattr(import_module(道, __name__), 項)
    if 名前 in _契約:
        return getattr(import_module('..共通契約.出力', __name__), 名前)
    raise AttributeError(名前)
