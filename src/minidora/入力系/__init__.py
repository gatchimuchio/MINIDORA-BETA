"""入力系の遅延公開入口。plain importでCompilerや駆動主体を起動しない。"""
from importlib import import_module

入力系版 = 'MINIDORA-入力系-v1'
_公開 = {
    '入力原本':'契約','入力出所':'契約','入力政策':'契約','入力診断':'契約','入力束':'契約',
    'HDS入力系':'統合','HDS入力コンパイラ':'統合','入力停止':'統合',
    '座標対応票':'関係射影','入力関係契約':'関係射影','関係要求へ射影':'関係射影',
}
__all__ = ['入力系版', *_公開]


def __getattr__(名前):
    if 名前 not in _公開: raise AttributeError(名前)
    値=getattr(import_module('.'+_公開[名前],__name__),名前)
    globals()[名前]=値
    return 値
