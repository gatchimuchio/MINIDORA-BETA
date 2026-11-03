"""MINIDORA βに関係する受入回帰313件を再実行する。ベンチマークではない。"""
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tests"))

MODULES = (
    "test_HDS学習機械循環_v2",
    "test_HDS内包統合_v2",
    "test_HDS内包統合_v2追加",
    "test_HDS原子的更新非退行",
    "test_HDS非退行包絡",
    "test_HDS実行主体_v1",
    "test_HDS目的保持_v1",
    "test_HDS形成隔離復帰_v1",
    "test_MINIDORAコア閉包_v3",
    "test_MINIDORAコア完成_v2",
    "test_HDSコア入力優先_v1",
    "test_HDS正本継承循環_v1",
    "test_HDS配線境界_v3",
    "test_HDS自律接続_v3",
    "test_実行系_HDS_v1",
    "test_β三機駆動",
    "目的関係修正_v1.test_実行契約",
    "目的関係修正_v1.test_循環回帰",
    "目的関係修正_v1.test_有限探索",
    "目的関係修正_v1.test_目的証拠",
    "目的関係修正_v1.test_計画契約",
)

if __name__ == "__main__":
    suite = unittest.defaultTestLoader.loadTestsFromNames(MODULES)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    raise SystemExit(0 if result.wasSuccessful() else 1)
