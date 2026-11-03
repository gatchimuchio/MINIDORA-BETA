from __future__ import annotations

from collections.abc import Sequence
from importlib import import_module
import sys

# 公開APIは保持し、実体は要求された時だけ読む。plain importで旧K3/HDS/HTTP/主体を起動しない。
_公開経路 = {
    'HDS作用供給器': ('HDS実行主体', 'HDS作用供給器'),
    'HDS運用セッション': ('HDS運用', 'HDS運用セッション'),
    'HDS製品ミニドラ': ('HDS運用.製品', 'HDS製品ミニドラ'),
    # v3内包統合。従来同名公開API（特にHDS作用記録）は上書きしない。
    'HDS認識項目': ('統合駆動_v2', 'HDS認識項目'),
    'HDS出典': ('統合駆動_v2', 'HDS出典'),
    'HDS認識差': ('統合駆動_v2', 'HDS認識差'),
    'HDS仮説': ('統合駆動_v2', 'HDS仮説'),
    'HDS予測': ('統合駆動_v2', 'HDS予測'),
    'HDS作業枝': ('統合駆動_v2', 'HDS作業枝'),
    'HDS命題': ('統合駆動_v2', 'HDS命題'),
    'HDS関係規則': ('統合駆動_v2', 'HDS関係規則'),
    'HDS不足抽出': ('統合駆動_v2', 'HDS不足抽出'),
    'HDS観測要求': ('統合駆動_v2', 'HDS観測要求'),
    'HDS観測器': ('統合駆動_v2', 'HDS観測器'),
    'HDS観測値': ('統合駆動_v2', 'HDS観測値'),
    'HDS依存辺': ('統合駆動_v2', 'HDS依存辺'),
    'HDS資料': ('統合駆動_v2', 'HDS資料'),
    'HDS記憶': ('統合駆動_v2', 'HDS記憶'),
    'HDS圧縮記憶': ('統合駆動_v2', 'HDS圧縮記憶'),
    'HDS参照索引': ('統合駆動_v2', 'HDS参照索引'),
    'HDS参照計画': ('統合駆動_v2', 'HDS参照計画'),
    'HDS草案': ('統合駆動_v2', 'HDS草案'),
    'HDS検証器': ('統合駆動_v2', 'HDS検証器'),
    'HDS検証票': ('統合駆動_v2', 'HDS検証票'),
    'HDS運用政策': ('統合駆動_v2', 'HDS運用政策'),
    'HDS阻害': ('統合駆動_v2', 'HDS阻害'),
    'HDS作用失敗': ('統合駆動_v2', 'HDS作用失敗'),
    'HDS計装': ('統合駆動_v2', 'HDS計装'),
    'HDS作用仕様': ('統合駆動_v2', 'HDS作用仕様'),
    'HDS構成計画': ('統合駆動_v2', 'HDS構成計画'),
    'HDS形成関係': ('統合駆動_v2', 'HDS形成関係'),
    'HDS経験': ('統合駆動_v2', 'HDS経験'),
    'HDS未来制約': ('統合駆動_v2', 'HDS未来制約'),
    'HDS未来状態': ('統合駆動_v2', 'HDS未来状態'),
    'HDS失敗診断': ('統合駆動_v2', 'HDS失敗診断'),
    'HDS異種表象': ('統合駆動_v2', 'HDS異種表象'),
    'HDS異種入力作用': ('統合駆動_v2', 'HDS異種入力作用'),
    'Crossref参照供給器': ('Crossref参照', 'Crossref参照供給器'),
    'EuropePMC参照供給器': ('EuropePMC参照', 'EuropePMC参照供給器'),
    'HDS構文化器成果': ('HDS構文化記録', 'HDS構文化器成果'),
    'HDSIR': ('HDS中間表現', 'HDSIR'),
    'HDSコア入力束': ('HDSコア入力', 'HDSコア入力束'),
    'HDSコア入力へ': ('HDSコア入力射影', 'HDSコア入力へ'),
    'HDSIRネイティブ適合器': ('K3_HDSネイティブ', 'HDSIRネイティブ適合器'),
    'HDSIR復元': ('HDS再生', 'HDSIR復元'),
    'HDSIR知識適合器': ('HDS資料K', 'HDSIR知識適合器'),
    'HDSIR辞書化': ('HDS再生', 'HDSIR辞書化'),
    'HDSK3結果': ('K3_HDSネイティブ', 'HDSK3結果'),
    'HDSコンパイラProtocol': ('HDS適合器', 'HDSコンパイラProtocol'),
    'HDSカーネルコンパイラProtocol': ('HDS適合器', 'HDSカーネルコンパイラProtocol'),
    'HDS独立カーネルコンパイル': ('HDS適合器', 'HDS独立カーネルコンパイル'),
    'HDSコア入力コンパイラProtocol': ('HDS適合器', 'HDSコア入力コンパイラProtocol'),
    'HDS独立コア入力コンパイル': ('HDS適合器', 'HDS独立コア入力コンパイル'),
    'HDSコンパイラパイプライン版': ('HDS構文化処理系列_v1_4', 'HDSコンパイラパイプライン版'),
    'HDSコンパイル束': ('HDS構文化器_v1', 'HDSコンパイル束'),
    'HDS選択コンパイル束': ('HDS構文化器_v1', 'HDS選択コンパイル束'),
    'HDSチェックリスト項目': ('HDS構文化記録_v1_1', 'HDSチェックリスト項目'),
    'HDS一時証拠統合': ('HDS作業状態', 'HDS一時証拠統合'),
    'HDS作業検査点': ('HDS作業状態', 'HDS作業検査点'),
    'HDS作業状態': ('HDS作業状態', 'HDS作業状態'),
    'HDS作業状態構築': ('HDS作業状態', 'HDS作業状態構築'),
    'HDS作業統計': ('HDS作業状態', 'HDS作業統計'),
    'HDS作業関係': ('HDS作業状態', 'HDS作業関係'),
    'HDS作用差分構造': ('HDS構文化記録_v1_3', 'HDS作用差分構造'),
    'HDS作用差分構造生成': ('HDS構文化作用差分', 'HDS作用差分構造生成'),
    'HDS作用種別': ('hds統合判断主体', 'HDS作用種別'),
    'HDS作用要求': ('hds統合判断主体', 'HDS作用要求'),
    'HDS作用記録': ('HDS構文化記録_v1_3', 'HDS作用記録'),
    'HDS保持契約': ('HDS構文化記録', 'HDS保持契約'),
    'HDS候補共同状態更新': ('HDS作業状態', 'HDS候補共同状態更新'),
    'HDS候補共同項目': ('HDS作業状態', 'HDS候補共同項目'),
    'HDS候補横断調停': ('HDS候補再照合', 'HDS候補横断調停'),
    'HDS候補診断': ('K3_HDSネイティブ', 'HDS候補診断'),
    'HDS候補証拠': ('HDS候補再照合', 'HDS候補証拠'),
    'HDS候補調停結果': ('HDS候補再照合', 'HDS候補調停結果'),
    'HDS判断主体': ('トリニティ文脈', 'HDS判断主体'),
    'HDS努力水準': ('HDS探索方針', 'HDS努力水準'),
    'HDS原理探索要求': ('HDS構文化記録', 'HDS原理探索要求'),
    'HDS原理段階': ('HDS構文化記録', 'HDS原理段階'),
    'HDS参照観測要求': ('HDS観測計画', 'HDS参照観測要求'),
    'HDS参照観測要求群': ('HDS観測計画', 'HDS参照観測要求群'),
    'HDS参照予算': ('HDS参照', 'HDS参照予算'),
    'HDS参照予算選択': ('HDS参照', 'HDS参照予算選択'),
    'HDS参照問合せ候補': ('HDS参照', 'HDS参照問合せ候補'),
    'HDS参照検索': ('HDS参照', 'HDS参照検索'),
    'HDS失敗署名Bank': ('HDS構文化失敗集', 'HDS失敗署名Bank'),
    'HDS失敗署名BankSnapshot': ('HDS構文化記録_v1_2', 'HDS失敗署名BankSnapshot'),
    'HDS失敗署名候補': ('HDS構文化記録_v1_1', 'HDS失敗署名候補'),
    'HDS失敗署名状態': ('HDS構文化記録_v1_1', 'HDS失敗署名状態'),
    'HDS失敗署名記録': ('HDS構文化記録_v1_2', 'HDS失敗署名記録'),
    'HDS失敗観測': ('HDS構文化記録_v1_2', 'HDS失敗観測'),
    'HDS実行核': ('HDS中間表現', 'HDS実行核'),
    'HDS実行主体': ('HDS実行主体', 'HDS実行主体'),
    'HDS実行状態': ('HDS実行主体', 'HDS実行状態'),
    'HDS状態差': ('HDS実行主体', 'HDS状態差'),
    'HDS作用機会': ('HDS実行主体', 'HDS作用機会'),
    'HDS作用結果': ('HDS実行主体', 'HDS作用結果'),
    'HDS終端': ('HDS実行主体', 'HDS終端'),
    'HDS関数作用': ('HDS実行主体', 'HDS関数作用'),
    'HDS駆動コア': ('HDS駆動コア', 'HDS駆動コア'),
    'HDS参照取得作用': ('HDS汎用作用', 'HDS参照取得作用'),
    'HDS計算実行作用': ('HDS汎用作用', 'HDS計算実行作用'),
    'HDS能力モジュール作用': ('HDS能力作用', 'HDS能力モジュール作用'),
    'HDS模型評価作用': ('HDS模型作用', 'HDS模型評価作用'),
    'HDS目的計画作用': ('HDS計画作用', 'HDS目的計画作用'),
    'HDS能力合成作用': ('HDS計画作用', 'HDS能力合成作用'),
    'HDS寄与関門再照合': ('HDS作業状態', 'HDS寄与関門再照合'),
    'HDS座標': ('HDS中間表現', 'HDS座標'),
    'HDS後続利用記録': ('HDS構文化記録_v1_3', 'HDS後続利用記録'),
    'HDS意味IR化': ('HDS構文化処理系列_v1_4', 'HDS意味IR化'),
    'HDS意味作用': ('HDS中間表現', 'HDS意味作用'),
    'HDS意味専用計画器': ('HDS構文化処理系列_v1_4', 'HDS意味専用計画器'),
    'HDS抽出規則改善候補': ('HDS構文化記録_v1_2', 'HDS抽出規則改善候補'),
    'HDS探索方針': ('HDS探索方針', 'HDS探索方針'),
    'HDS探索方針選択': ('HDS探索方針', 'HDS探索方針選択'),
    'HDS改善対象': ('HDS構文化記録_v1_2', 'HDS改善対象'),
    'HDS文脈': ('HDS適合器', 'HDS文脈'),
    'HDS暗黙知記録': ('HDS構文化記録_v1_1', 'HDS暗黙知記録'),
    'HDS残差': ('HDS中間表現', 'HDS残差'),
    'HDS状態ノード': ('HDS構文化記録_v1_1', 'HDS状態ノード'),
    'HDS状態差記録': ('HDS構文化記録_v1_3', 'HDS状態差記録'),
    'HDS状態遷移図': ('HDS構文化記録_v1_1', 'HDS状態遷移図'),
    'HDS独立コンパイル': ('HDS適合器', 'HDS独立コンパイル'),
    'HDS監査参照候補': ('HDS構文化記録_v1_1', 'HDS監査参照候補'),
    'HDS監査状態': ('HDS構文化記録', 'HDS監査状態'),
    'HDS監査要求': ('HDS構文化記録', 'HDS監査要求'),
    'HDS監査項目': ('HDS構文化記録', 'HDS監査項目'),
    'HDS知識投入結果': ('HDS資料K', 'HDS知識投入結果'),
    'HDS計算コンパイル成果': ('HDS構文化器_v1', 'HDS計算コンパイル成果'),
    'HDS計算降下': ('HDS計算降下', 'HDS計算降下'),
    'HDS計算降下バックエンド': ('HDS構文化処理系列_v1_4', 'HDS計算降下バックエンド'),
    'HDS証拠事実': ('HDS資料K', 'HDS証拠事実'),
    'HDS証拠状態複製': ('HDS資料K', 'HDS証拠状態複製'),
    'HDS認知世界差分': ('HDS構文化記録_v1_1', 'HDS認知世界差分'),
    'HDS認知世界断片': ('HDS構文化記録', 'HDS認知世界断片'),
    'HDS調停済証拠': ('HDS候補再照合', 'HDS調停済証拠'),
    'HDS遷移辺': ('HDS構文化記録_v1_1', 'HDS遷移辺'),
    'HDS選択問題': ('HDS選択実行系', 'HDS選択問題'),
    'HDS選択実行結果': ('HDS選択実行系', 'HDS選択実行結果'),
    'HDS選択推論実行': ('HDS選択実行系', 'HDS選択推論実行'),
    'HDS関係': ('HDS中間表現', 'HDS関係'),
    'HDS駆動ミニドラ': ('実行系_HDS_v1', 'HDS駆動ミニドラ'),
    'HDS駆動選択実行': ('HDS統合実行系', 'HDS駆動選択実行'),
    'HDS駆動選択結果': ('HDS統合実行系', 'HDS駆動選択結果'),
    'K3相当能力核': ('K3機能', 'K3相当能力核'),
    'K3能力結果': ('K3機能', 'System結果'),
    'LAYER0仕様版': ('第0層', 'LAYER0仕様版'),
    'LAYER0参照コミット': ('第0層', 'LAYER0参照コミット'),
    'LAYER0機能責任': ('第0層', 'LAYER0機能責任'),
    'LAYER0正本リポジトリ': ('第0層', 'LAYER0正本リポジトリ'),
    'Layer0': ('第0層', 'Layer0'),
    'MINIDORAHDS判断主体': ('hds統合判断主体', 'MINIDORAHDS判断主体'),
    'MINIDORA認知世界': ('hds統合判断主体', 'MINIDORA認知世界'),
    'OpenAlex参照供給器': ('HTTP参照', 'OpenAlex参照供給器'),
    'Trinity文脈系': ('トリニティ文脈', 'Trinity文脈系'),
    'Trinity記憶監査': ('トリニティ文脈', 'Trinity記憶監査'),
    'Wikipedia参照供給器': ('HTTP参照', 'Wikipedia参照供給器'),
    'run_k3_equivalence_外部評価': ('K3評価', 'run_k3_equivalence_外部評価'),
    'ミニドラ': ('実行系', 'ミニドラ'),
    '一般知識参照供給器': ('標準参照', '一般知識参照供給器'),
    '主体主幹': ('主体', '主体主幹'),
    '主体整合結果': ('主体', '主体整合結果'),
    '主体更新提案': ('主体', '主体更新提案'),
    '主体更新記録': ('主体', '主体更新記録'),
    '主体状態': ('主体', '主体状態'),
    '作用': ('命令', '作用'),
    '値状態': ('HDS中間表現', '値状態'),
    '公開HDSコンパイラ': ('HDS構文化器_v1', '公開HDSコンパイラ'),
    '公開HDSコンパイラ方針': ('HDS構文化器_v1', '公開HDSコンパイラ方針'),
    '参照供給器': ('参照', '参照供給器'),
    '参照矛盾数': ('参照', '参照矛盾数'),
    '参照記録': ('参照', '参照記録'),
    '命令': ('命令', '命令'),
    '命令計算降下': ('命令計算降下', '命令計算降下'),
    '固定参照供給器': ('参照', '固定参照供給器'),
    '実行文脈': ('計算実行器', '実行文脈'),
    '実行状態': ('採否', '実行状態'),
    '意味列': ('言語構造', '意味列'),
    '手順': ('命令', '手順'),
    '採否': ('採否', '採否'),
    '採否結果': ('採否', '採否結果'),
    '文字知識': ('言語基底', '文字知識'),
    '標準言語基底P': ('言語基底', '標準言語基底P'),
    '標準計算実行境界': ('計算実行境界', '標準計算実行境界'),
    '結果': ('実行系', '結果'),
    '自然言語器': ('言語', '自然言語器'),
    '複合参照供給器': ('参照', '複合参照供給器'),
    '要求': ('実行系', '要求'),
    '規模測定': ('規模測定', '規模測定'),
    '規模測定版': ('規模測定', '規模測定版'),
    '規模測定結果': ('規模測定', '規模測定結果'),
    '言語基底P': ('言語基底', '言語基底P'),
    '言語基底版': ('言語基底', '言語基底版'),
    '言語計画': ('言語', '言語計画'),
    '言語関係抽出': ('言語構造', '言語関係抽出'),
    '言語関係構造': ('言語構造', '言語関係構造'),
    '計算中間表現': ('計算中間表現', '計算中間表現'),
    '計算中間表現版': ('計算中間表現', '計算中間表現版'),
    '計算作用': ('計算中間表現', '計算作用'),
    '計算値': ('計算中間表現', '計算値'),
    '計算値種別': ('計算中間表現', '計算値種別'),
    '計算命令': ('計算中間表現', '計算命令'),
    '計算実行器': ('計算実行器', '計算実行器'),
    '計算実行境界': ('計算実行境界', '計算実行境界'),
    '計算実行境界版': ('計算実行境界', '計算実行境界版'),
    '計算実行結果': ('計算中間表現', '計算実行結果'),
    '計算履歴': ('計算中間表現', '計算履歴'),
    '記憶主体': ('トリニティ文脈', '記憶主体'),
    '語彙知識': ('言語基底', '語彙知識'),
}
_互換ワイルドカード = ("模型_v05",)

def _ロード済み公開名を正規化() -> None:
    'submodule importがpackage属性へ置いた同名モジュールを公開API実体へ戻す。'
    for public_name, (モジュール_name, 情報源_name) in _公開経路.items():
        loaded = sys.modules.get(f"{__name__}.{モジュール_name}")
        if loaded is None or not hasattr(loaded, 情報源_name):
            continue
        current = globals().get(public_name)
        if current is loaded or public_name not in globals():
            globals()[public_name] = getattr(loaded, 情報源_name)

def __getattr__(name: str):
    経路 = _公開経路.get(name)
    if 経路 is not None:
        モジュール_name, 情報源_name = 経路
        モジュール = import_module(f".{モジュール_name}", __name__)
        value = getattr(モジュール, 情報源_name)
        globals()[name] = value
        _ロード済み公開名を正規化()
        return globals().get(name, value)
    for モジュール_name in _互換ワイルドカード:
        モジュール = import_module(f".{モジュール_name}", __name__)
        _ロード済み公開名を正規化()
        if name in getattr(モジュール, "__all__", ()) and hasattr(モジュール, name):
            value = getattr(モジュール, name)
            globals()[name] = value
            return value
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")

class _遅延公開名(Sequence):
    _cache: tuple[str, ...] | None = None
    def _names(self) -> tuple[str, ...]:
        if self._cache is None:
            names = set(_公開経路)
            for モジュール_name in _互換ワイルドカード:
                モジュール = import_module(f".{モジュール_name}", __name__)
                _ロード済み公開名を正規化()
                names.update(getattr(モジュール, "__all__", ()))
            self._cache = tuple(sorted(names))
        return self._cache
    def __len__(self): return len(self._names())
    def __getitem__(self, index): return self._names()[index]
    def __iter__(self): return iter(self._names())

__all__ = _遅延公開名()

def __dir__():
    return sorted(set(globals()) | set(_公開経路))


# 大小文字だけ異なる旧入口は二重配置せず、要求時だけ正本へ接続する。
# importプロトコルの固定メソッド名は外部境界であり、内部概念名ではない。
class _旧名接続:
    _ミニドラ旧名接続 = True
    _対応 = {
        __name__ + ".hds作業状態": "HDS作業状態",
        __name__ + ".hds統一状態循環": "HDS統一状態循環",
    }

    def find_spec(self, 名前, 経路=None, 対象=None):
        if 名前 not in self._対応:
            return None
        from importlib.util import spec_from_loader
        from types import SimpleNamespace
        # 標準importlibの固定フックへ、日本語名の処理を局所的に接続する。
        接続 = SimpleNamespace(create_module=self._正本読込, exec_module=self._読込済み)
        return spec_from_loader(名前, 接続)

    def _正本読込(self, 仕様):
        正本 = import_module("." + self._対応[仕様.name], __name__)
        _ロード済み公開名を正規化()
        return 正本

    def _読込済み(self, モジュール):
        # 正本は読込済み。旧名で二重実行しない。
        pass


if not any(getattr(接続, "_ミニドラ旧名接続", False) for 接続 in sys.meta_path):
    sys.meta_path.insert(0, _旧名接続())
