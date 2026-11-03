"""実導出の作用と依存から説明文を作る。説明順を変えても根拠を追加しない。"""
from __future__ import annotations
from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class 説明節:
    工程: int
    役割: str
    命題: str
    依存: tuple[int, ...]
    本文: str


def 導出説明節(番号: int, 作用: str, 命題: str, 依存: tuple[int, ...]) -> 説明節:
    if (type(番号) is not int or 番号 < 1 or type(依存) is not tuple
            or any(type(i) is not int or not 1 <= i < 番号 for i in 依存)
            or type(命題) is not str or not 命題):
        raise ValueError('談話の導出順・命題・依存不正')
    prior = '・'.join(map(str, 依存))
    if 作用 == '資料記載':
        役割 = '記載'; text = f'資料には「{命題}」と記載されています。'
    elif 作用 in ('問いの仮定', '場合の仮定'):
        役割 = '仮定'; text = f'この検討の場合だけ、「{命題}」を仮定します。事実の追加ではありません。'
    elif 作用 == '条件適用':
        役割 = '推論'; text = f'工程{prior}の規則と前件を組み合わせ、「{命題}」を導きます。'
    elif 作用 == '全称具体化':
        役割 = '具体化'; text = f'工程{prior}の一般規則を対象に当てはめると、「{命題}」になります。'
    elif 作用 == '連言分解':
        役割 = '分解'; text = f'工程{prior}で共に述べられた内容から、「{命題}」を取り出します。'
    elif 作用 == '全場合を閉じた選言除去':
        役割 = '場合統合'; text = f'工程{prior}の全ての場合で同じ結論を導けたため、「{命題}」を導きます。どの選択肢かは確定していません。'
    elif 作用 == '任意個体からの全称導出':
        役割 = '全称'; text = f'特定の成功例の列挙ではなく、工程{prior}の任意個体についての導出から「{命題}」を得ます。'
    elif 作用 == '仮定を閉じた条件導出':
        役割 = '条件'; text = f'工程{prior}で置いた仮定の範囲を閉じ、「{命題}」という条件付きの結論にします。'
    elif 作用 in ('連言構成', '選言導入', '存在証拠', '存在の局所証人'):
        役割 = '推論'; text = f'工程{prior}を用いた{作用}によって、「{命題}」を得ます。'
    else:
        raise ValueError('未対応の導出作用を説明で補完しない:' + str(作用))
    return 説明節(番号, 役割, 命題, 依存, text)
