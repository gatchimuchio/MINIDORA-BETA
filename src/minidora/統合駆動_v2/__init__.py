"""HDS内包統合の公開データ契約と部品。制御主体はminidora.HDS実行主体に一本化。"""
from .認識 import 認識区分, HDS出典, HDS認識項目, HDS認識差, 認識差分
from .依存 import HDS依存辺, 下流集合, 上流集合
from .記憶 import HDS資料, HDS記憶, HDS参照索引, HDS参照計画, HDS圧縮記憶
from .観測 import HDS観測要求, HDS観測値, HDS観測器, 必要観測を構成
from .仮説 import HDS予測, HDS仮説, HDS仮説雛型, HDS作業枝, 枝を合流, 識別対数
from .計画 import HDS作用仕様, HDS構成計画, 作用列を構成
from .検証 import HDS草案, HDS検証器, HDS検証票, HDS先行検証結果, 先行草案を検証
from .形成 import HDS経験, HDS形成関係, HDS形成採用状態, 経験から形成, 再実行で検証, 形成隔離を審査, 実行結果から経験, 形成手順を再利用
from .政策 import HDS運用政策, 停止理由, HDS阻害, HDS作用失敗, HDS計装
from .入力境界 import HDS異種表象, HDS異種入力作用

from .保存 import 保存する, 復元する

from .意味構成 import HDS命題, HDS関係規則, HDS不足抽出, 不足を抽出, 関係から仮説を構成, 仮説から枝を構成
from .自動記憶 import 原資料を圧縮
from .未来 import HDS未来制約, HDS未来状態, 未来列を構成
from .診断 import HDS失敗診断
from .適応記憶 import HDS適応記憶
from .目的保持 import HDS目的観測, 目的契約署名, 目的を観測, 計画効果を実測, 目的進展を判定
from .一時適応 import HDS一時適応キャッシュ

__all__ = [k for k in globals() if not k.startswith("_") and k not in ("annotations",)]
