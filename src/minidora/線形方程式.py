"""有理数係数の連立一次方程式。一意解・自由解・解なしを分離して返す。"""
from __future__ import annotations

from collections.abc import Callable
from copy import deepcopy
from fractions import Fraction

from .記号演算 import _予算, _多項式処理, _係数, _署名結果, _失敗, 数学境界違反
from .製品版.型 import 能力結果


def 線形を解く(変数: tuple[str, ...], 方程式: tuple[dict, ...], *, 最大演算数: int = 100000,
               停止要求: Callable[[], bool] | None = None) -> 能力結果:
    try:
        予算 = _予算(最大演算数, 停止要求)
        engine = _多項式処理(変数, 予算)
        if not 変数 or type(方程式) is not tuple or len(方程式) > 16:
            raise 数学境界違反("線形問題は1〜8変数、0〜16方程式")
        n, m = len(変数), len(方程式)
        original = []
        for equation in 方程式:
            if type(equation) is not dict or set(equation) != {"左辺", "右辺"}:
                raise 数学境界違反("方程式の項目不正")
            poly = engine.加算(engine.読む(equation["左辺"]), engine.読む(equation["右辺"]), -1)
            if any(sum(p) > 1 for p in poly):
                raise 数学境界違反("非線形方程式は未対応")
            row = []
            for i in range(n):
                p = [0]*n; p[i] = 1
                row.append(poly.get(tuple(p), Fraction(0)))
            row.append(-poly.get(engine.零次数, Fraction(0)))
            original.append(row)
        matrix = deepcopy(original)
        transform = [[Fraction(i == j) for j in range(m)] for i in range(m)]
        pivots, operations = [], []
        rank = 0
        for col in range(n):
            予算.進める()
            row = next((r for r in range(rank, m) if matrix[r][col]), None)
            if row is None:
                continue
            if row != rank:
                matrix[row], matrix[rank] = matrix[rank], matrix[row]
                transform[row], transform[rank] = transform[rank], transform[row]
                operations.append({"操作": "交換", "行": [rank, row]})
            divisor = matrix[rank][col]
            matrix[rank] = [_係数(x / divisor) for x in matrix[rank]]
            transform[rank] = [_係数(x / divisor) for x in transform[rank]]
            operations.append({"操作": "定数倍", "行": rank, "倍率": str(1/divisor)})
            for r in range(m):
                if r == rank or not matrix[r][col]:
                    continue
                factor = matrix[r][col]
                for j in range(n+1):
                    予算.進める()
                    matrix[r][j] = _係数(matrix[r][j] - factor*matrix[rank][j])
                for j in range(m):
                    予算.進める()
                    transform[r][j] = _係数(transform[r][j] - factor*transform[rank][j])
                operations.append({"操作": "他行加算", "対象行": r, "参照行": rank, "倍率": str(-factor)})
            pivots.append(col)
            rank += 1
        # 行変形の記録が元の全方程式に接続することを、行列積として確認する。
        for r in range(m):
            for c in range(n+1):
                予算.進める()
                value = sum((transform[r][k]*original[k][c] for k in range(m)), Fraction(0))
                if value != matrix[r][c]:
                    raise 数学境界違反("行変形の再検証不一致")
        bad = next((r for r in range(m) if not any(matrix[r][:n]) and matrix[r][n]), None)
        solution, particular, basis = None, None, []
        free = [i for i in range(n) if i not in pivots]
        contradiction = None
        if bad is not None:
            状態 = "解なし"
            contradiction = {"行": bad, "元方程式の係数": [str(v) for v in transform[bad]],
                             "導出左辺": [str(v) for v in matrix[bad][:n]], "導出右辺": str(matrix[bad][n])}
        else:
            状態 = "一意解" if rank == n else "自由解"
            values = [Fraction(0)]*n
            for r, c in enumerate(pivots):
                values[c] = matrix[r][n]
            vectors = []
            for c in free:
                vector = [Fraction(0)]*n; vector[c] = Fraction(1)
                for r, pivot in enumerate(pivots):
                    vector[pivot] = -matrix[r][c]
                vectors.append(vector)
                basis.append({"自由変数": 変数[c], "係数": {name: str(v) for name, v in zip(変数, vector)}})
            # 特解を元の全式へ代入し、基底が同次系を満たすことを確認する。
            for row in original:
                予算.進める()
                if sum((a*x for a, x in zip(row[:n], values)), Fraction(0)) != row[n]:
                    raise 数学境界違反("特解の元方程式代入不一致")
                for vector in vectors:
                    予算.進める()
                    if sum((a*x for a, x in zip(row[:n], vector)), Fraction(0)):
                        raise 数学境界違反("自由解基底の代入不一致")
            particular = {name: str(v) for name, v in zip(変数, values)}
            if 状態 == "一意解":
                solution = particular.copy()
        text = 状態
        if solution is not None:
            text += ": " + "、".join(f"{k}={v}" for k, v in solution.items())
        elif 状態 == "自由解":
            text += "。特解と自由変数の基底を保持し、自由変数を勝手に固定しません。"
        else:
            text += "。元の方程式の線形結合から矛盾を記録しました。"
        return _署名結果(text, {"処理": "線形方程式", "入力": {
            "変数": list(変数), "方程式": deepcopy(list(方程式))},
            "判定": 状態, "階数": rank, "解": solution, "特解": particular,
            "自由変数": [変数[i] for i in free] if 状態 != "解なし" else [], "基底": basis,
            "矛盾証明": contradiction,
            "元拡大行列": [[str(v) for v in row] for row in original],
            "行簡約形": [[str(v) for v in row] for row in matrix],
            "行変換行列": [[str(v) for v in row] for row in transform],
            "行操作": operations, "検査": "行変換積と、可解時は元式への特解・基底代入を確認",
            "解の領域": "実数。係数・特解・基底は有理数、自由変数の値は任意の実数"}, 予算)
    except Exception as exc:
        return _失敗(exc)
