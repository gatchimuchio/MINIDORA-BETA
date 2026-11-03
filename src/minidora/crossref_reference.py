"""旧英字名の互換入口。現行正本は `Crossref参照.py`。"""
from importlib import import_module as _読込
_正本 = _読込(".Crossref参照", __package__)
for _名, _値 in vars(_正本).items():
    if _名 not in {"__name__", "__package__", "__loader__", "__spec__", "__file__", "__cached__"}:
        globals()[_名] = _値
