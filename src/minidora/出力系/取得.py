"""駆動が作成した出力束だけを受理し、入力・駆動状態への参照を渡さない。"""
from __future__ import annotations
from ..共通契約.出力 import 出力束, 出力政策, 出力取得結果
from ..共通契約.封緘 import 封緘する
from ..コア.内容計画 import 内容計画, 内容計画を検査
from ..駆動系.契約 import 関係出力束, 構造出力束, 関係保証


def 内容を取得(束: 出力束, 政策: 出力政策) -> 出力取得結果:
    if not isinstance(束, 出力束) or not isinstance(政策, 出力政策):
        raise TypeError('出力束と出力政策が必要')
    束.__post_init__()
    束.表現.__post_init__()
    内容 = 束.内容を読む()
    # 同じ既存封緘演算を再利用し、型と値を保ったコピーを得る。
    検査 = 封緘する(内容, 最大要素=政策.最大内容要素, 最大バイト=政策.最大内容バイト)
    if 検査.署名 != 束.内容.署名:
        raise ValueError('出力正本の内容が不一致')
    if 束.内容種別 == '内容計画':
        if not isinstance(内容, 内容計画) or not 内容計画を検査(内容):
            raise ValueError('採用された内容計画が必要')
    elif 束.内容種別 == '関係':
        if not isinstance(内容, 関係出力束):
            raise TypeError('関係出力束型が必要')
        内容.__post_init__()
        if (内容.状態 == '成立') != (束.状態 == '成立'):
            raise ValueError('関係結果と出力の採否が不一致')
        if any(x.保証 == 関係保証.仮説 for x in 内容.回答):
            raise ValueError('未検証仮説を確定回答に混入できない')
    elif 束.内容種別 == '構造':
        if not isinstance(内容, 構造出力束):
            raise TypeError('構造出力束型が必要')
        if (内容.状態 == '成立') != (束.状態 == '成立'):
            raise ValueError('構造結果と出力の採否が不一致')
        if 束.状態 == '成立' and (内容.構造成果 is None or not 内容.構造成果.完了):
            raise ValueError('未完了の構造結果を成立出力にできない')
    elif 束.内容種別 == '診断':
        if 内容 is not None:
            raise ValueError('診断出力に未採用内容を紛れ込ませない')
    else:
        # 公開出力の値は基本型の木だけ。任意のホスト型をstrで隠さない。
        def 基本木(値):
            if 値 is None or type(値) in (str, bool, int, float):
                return True
            if type(値) in (tuple, list):
                return all(基本木(x) for x in 値)
            if type(値) is dict:
                return all(type(k) is str and 基本木(v) for k, v in 値.items())
            return False
        if not 基本木(内容):
            raise TypeError('値の出力は文字列鍵の基本型木だけに対応')
    # 大きい根拠・理由・必須内容も容量に含める。本文だけの上限で迂回させない。
    全域 = 封緘する((束.鍵, 束.目的署名, 束.結果署名, 束.状態, 束.内容種別,
        束.内容.木, 束.表現, 束.内容言語, 束.入力署名, 束.理由, 束.根拠,
        束.条件, 束.留保, 束.保証, 束.未充足, 束.版),
        最大要素=政策.最大内容要素, 最大バイト=政策.最大内容バイト)
    return 出力取得結果(束, 全域.バイト数)
