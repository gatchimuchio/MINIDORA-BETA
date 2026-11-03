'実HDSの原文を、対象・作用・引数・依存へ局所射影する。\n\n全文を消費する小文法。語彙は作用を示し、モジュール名や工程列を返さない。\n任意の日本語理解・既存構文化器の代用品ではない。未知部分は残差として停止する。\n'
from __future__ import annotations
import ast
from copy import deepcopy
from dataclasses import dataclass
import re
from .HDS中間表現 import HDSIR, 値状態
from .汎用要求IR import 汎用要求IR, 目的指定
from .製品版.型 import 能力結果
from .要求解釈 import _符号

@dataclass(frozen=True, slots=True)
class 目的射影結果:
    要求: 汎用要求IR | None
    HDS保持: HDSIR | None
    理由: str = ''
    局所解消: tuple[str, ...] = ()

    @property
    def 成立(self):
        return self.要求 is not None and not self.理由

# 各述語は成果の意味に対応する。呼び出す能力・前処理の並びはここに置かない。
_述語 = {
    '微分': '微分結果', '積分': '積分結果', '展開': '正規化結果', '整理': '正規化結果',
    '計算': '定数値',
    '数字を抽出': '数字列', '数字抽出': '数字列', '数値を抽出': '数字列',
    'URLを抽出': 'URL列', 'キーワードを抽出': 'キーワード列',
    '箇条書きに': '箇条書き文', '要約': '抽出要約文', 'JSONに変換': 'JSON変換文書',
    'コードの構造を説明': 'コード構造報告',
}
_語尾 = ('してください', 'して下さい', 'してくれ', 'して', 'する', 'せよ')
_特殊 = {'解いて': '方程式結果', '解いてください': '方程式結果',
          '取り出して': '文書単一値', '取り出してください': '文書単一値',
          '一意解を求めて': '一意解', '一意解を求めてください': '一意解'}
_表層 = {stem + tail: effect for stem, effect in _述語.items() for tail in _語尾}
_表層.update(_特殊)
_式型 = {'微分結果', '積分結果', '正規化結果', '定数値'}
_引用 = re.compile(r'「([^「」]*)」')
_識別子 = re.compile(r'[A-Za-z_][A-Za-z_0-9]*\Z')


def _節(text):
    start, quoted = 0, False
    for i, char in enumerate(text):
        if char == '「':
            if quoted: raise ValueError('入れ子の引用は未対応')
            quoted = True
        elif char == '」':
            if not quoted: raise ValueError('引用の閉じが過剰')
            quoted = False
        elif not quoted and char in '、。；\n':
            if text[start:i].strip(): yield start, i
            start = i + 1
    if quoted: raise ValueError('引用が閉じていない')
    if text[start:].strip(): yield start, len(text)


def _変数(expr):
    # 構文のみを読む。式の計算・能力実行をここで行わない。
    if len(expr) > 4096: raise ValueError('式が長すぎる')
    node = ast.parse(expr, mode="eval")
    allowed = (ast.Expression, ast.BinOp, ast.UnaryOp, ast.Constant, ast.Name,
               ast.Load, ast.Add, ast.Sub, ast.Mult, ast.Div, ast.Pow, ast.USub, ast.UAdd)
    rows = list(ast.walk(node))
    if len(rows) > 512 or any(not isinstance(x, allowed) for x in rows):
        raise ValueError('未対応の数式構文')
    if any(isinstance(x, ast.Constant) and (type(x.value) not in (int, float)) for x in rows):
        raise ValueError('数式に非数値が含まれる')
    names = sorted({x.id for x in rows if isinstance(x, ast.Name)})
    if len(names) > 16 or any(not _識別子.fullmatch(n) for n in names):
        raise ValueError('変数の宣言範囲外')
    return names


def _数式資料(text, goal, args):
    if goal in ('方程式結果', '一意解'):
        equations, names = [], set()
        for eq in text.split(';'):
            if eq.count('=') != 1: raise ValueError('等式をセミコロンで区切る')
            left, right = eq.split('=')
            names.update(_変数(left.strip())); names.update(_変数(right.strip()))
            equations.append({'左辺': left.strip(), '右辺': right.strip()})
        return 能力結果(True, text, データ={'変数': sorted(names), '方程式': equations}), '方程式'
    names = _変数(text)
    if '変数' in args and args['変数'] not in names:
        names = sorted([*names, args['変数']])
    return 能力結果(True, text, データ={'式': text, '変数': names}), '数式'


def _HDS照合(ir, literal_spans, 参照_spans, equation_spans):
    if ir.入力言語 != 'ja' or ir.出力言語 not in (None, 'ja'):
        raise ValueError('目的射影の入出力は日本語')
    coords = ir.座標辞書()
    if len(coords) != len(ir.座標): raise ValueError('HDS座標ID重複')
    sources = [c for c in ir.座標 if c.種別 == '情報源_text']
    if len(sources) != 1 or sources[0].内容 != ir.原文:
        raise ValueError('HDS原文座標不一致')
    allowed = {'情報源_text', '言語.normalized', '文脈.言語', '制御.選択意図',
               '値.数量', '属性.単位', '対象.主題語', '目的.検索焦点'}
    # 引用された形式等式だけを対象とする。一般命題の等価関係へは広げない。
    resolved_ids, 関係_ids = set(), set()
    for 関係 in ir.関係:
        if (関係.種別 == '等価' and 関係.条件 == ('検索述語==',)
                and 関係.値状態 == 値状態.確定
                and len(関係.始点) == len(関係.終点) == 1):
            left, right = coords.get(関係.始点[0]), coords.get(関係.終点[0])
            if left is None or right is None or type(left.内容) is not str or type(right.内容) is not str:
                raise ValueError('等式の座標欠落')
            fragment = re.sub(r'\s+', '', left.内容 + '=' + right.内容)
            if not any(fragment in re.sub(r'\s+', '', ir.原文[a:b]) for a, b in equation_spans):
                raise ValueError('引用した形式等式とHDS関係が不一致')
            resolved_ids.update((*関係.始点, *関係.終点))
            関係_ids.add(関係.関係ID)
    for c in ir.座標:
        if (c.座標ID in resolved_ids or (関係_ids and c.種別 == '関係.述語' and c.内容 == '=')):
            if c.値状態 != 値状態.確定: raise ValueError('未確定の等式座標')
            continue
        if c.値状態 != 値状態.確定 or c.種別 not in allowed:
            raise ValueError('未処理HDS座標:' + c.座標ID)
        if c.種別 == '制御.選択意図' and c.内容 != '通常':
            raise ValueError('未処理の否定・選択意図')
    for r in ir.関係:
        if r.関係ID in 関係_ids: continue
        if (r.値状態 != 値状態.確定 or r.条件
                or any(k not in coords for k in (*r.始点, *r.終点))):
            raise ValueError('未解消のHDS関係')
        if r.種別 != '数量単位':
            raise ValueError('未対応のHDS関係:' + r.種別)
    solved, reasons = ['形式等式へ局所射影:' + x for x in sorted(関係_ids)], set()
    for r in ir.残差:
        occurrences = tuple(re.finditer(re.escape(r.原文), ir.原文)) if r.原文 else ()
        covered = occurrences and all(any(a <= m.start() and m.end() <= b
            for a, b in (*参照_spans, *literal_spans)) for m in occurrences)
        if r.種別 != '未解共参照' or r.影響座標 or not covered:
            raise ValueError('未解消HDS残差:' + r.種別)
        solved.append(r.残差ID); reasons.add(r.理由)
    if any(loss not in reasons for a in ir.意味作用履歴 for loss in a.損失):
        raise ValueError('未解消のHDS意味損失')
    return tuple(solved)


def HDSから目的要求(ir: HDSIR, materials: dict[str, 能力結果], *,
                  前回成果: tuple[tuple[str, 能力結果], ...] = ()) -> 目的射影結果:
    kept = None
    try:
        if type(ir) is not HDSIR or type(ir.原文) is not str or not 0 < len(ir.原文) <= 8192:
            raise ValueError('実HDSIRと有限の原文が必要')
        if len(_符号(ir)) > 2000000: raise ValueError('HDSIRサイズ上限')
        kept = deepcopy(ir)
        if type(materials) is not dict or len(materials) > 32:
            raise ValueError('名前付き素材が必要')
        if (type(前回成果) is not tuple or any(type(r) is not tuple or len(r) != 2
                or type(r[0]) is not str or type(r[1]) is not 能力結果 for r in 前回成果)):
            raise ValueError('前回成果の型不正')
        values = deepcopy(materials)
        if any(type(k) is not str or type(v) is not 能力結果 for k, v in values.items()):
            raise ValueError('素材型不正')
        # 名前空間を分離し、利用者資料名と内部の目的名を衝突させない。
        資料, kinds, params, goals = {}, {}, {}, []
        named = {k: f'素材:{i}' for i, k in enumerate(values)}
        literal_spans, ref_spans, equation_spans = [], [], []
        previous, original_target = None, None
        for a, b in _節(ir.原文):
            if len(goals) >= 16: raise ValueError('目的数上限')
            raw = ir.原文[a:b]
            clause = raw.strip()
            prefix = re.match(r'(?:まず|次に|続けて|最後に)\s*', clause)
            if prefix: clause = clause[prefix.end():]
            endings = [e for e in _表層 if clause.endswith(e)]
            if not endings: raise ValueError('未対応の述語又は条件:' + clause)
            ending = max(endings, key=len)
            effect = _表層[ending]
            head = clause[:-len(ending)].strip()
            情報源 = None; value = None; declared = None; args = {}
            target = re.match(r'(?:(JSON|CSV|Python)資料|資料)「([^「」]+)」', head)
            literal = _引用.match(head)
            if target:
                fmt, name = target.groups()
                if name not in values: raise ValueError('資料がない:' + name)
                情報源, value = named[name], values[name]
                declared = {'JSON': 'JSON原文', 'CSV': 'CSV原文', 'Python': 'コード本文'}.get(fmt, '本文')
                head = head[target.end():]
            elif head.startswith('その結果') or head.startswith('それ'):
                term = 'その結果' if head.startswith('その結果') else 'それ'
                情報源 = previous
                if 情報源 is None:
                    if term == 'その結果' or len(前回成果) != 1:
                        raise ValueError('一意な照応先がない')
                    情報源, value, declared = '過去:0', 前回成果[0][1], '本文'
                pos = ir.原文.find(term, a, b)
                ref_spans.append((pos, pos + len(term)))
                head = head[len(term):]
            elif head.startswith('元の資料'):
                情報源 = original_target
                if 情報源 is None: raise ValueError('元の資料が未指定')
                head = head[len('元の資料'):]
            elif literal:
                情報源, value, declared = '引用:' + str(len(goals)), 能力結果(True, literal.group(1)), '本文'
                start = ir.原文.find(literal.group(0), a, b)
                literal_spans.append((start, start + len(literal.group(0))))
                head = head[literal.end():]
            elif head.startswith('本文'):
                name = '本文' if '本文' in values else next(iter(values), None) if len(values) == 1 else None
                if name is None: raise ValueError('本文の対象が不明')
                情報源, value, declared = named[name], values[name], '本文'
                head = head[2:]
            else:
                情報源 = previous
                if 情報源 is None and len(values) == 1:
                    name = next(iter(values)); 情報源, value, declared = named[name], values[name], '本文'
            head = head.strip()
            for particle in ('から', 'を', 'の'):
                if head.startswith(particle): head = head[len(particle):].strip(); break
            if effect in ('微分結果', '積分結果'):
                match = re.fullmatch(r'([A-Za-z_][A-Za-z_0-9]*)で', head)
                if not match: raise ValueError('対象変数を明示する')
                args['変数'] = match.group(1)
            elif effect == '文書単一値':
                match = re.fullmatch(r'位置「([^「」]*)」の(数値|文字列|真偽|空値)を', head)
                if not match: raise ValueError('位置と値の型を明示する')
                args = {'位置': match.group(1), '型': match.group(2)}
            elif head:
                raise ValueError('未解釈の修飾又は条件:' + head)
            if 情報源 is None: raise ValueError('対象が未指定')
            if value is not None:
                if effect in _式型 or effect in ('方程式結果', '一意解'):
                    if value.データ.get('処理') == '記号演算':
                        from .記号演算 import 数学記録整合
                        if not 数学記録整合(value): raise ValueError('数学記録の不一致')
                        declared = {'積分': '積分結果'}.get(value.データ['入力']['操作'], '正規化結果')
                    elif value.データ:
                        raise ValueError('未知資料を数式に読み替えない')
                    else:
                        value, declared = _数式資料(value.本文, effect, args)
                        if declared == '方程式' and literal:
                            equation_spans.append(literal_spans[-1])
                if 情報源 in kinds and kinds[情報源] != declared:
                    raise ValueError('同じ素材への型宣言が競合')
                資料[情報源], kinds[情報源] = value, declared
            if original_target is None: original_target = 情報源
            gid, param = f'目的:{len(goals)}', f'引数:{len(goals)}'
            goals.append(目的指定(gid, 情報源, effect, param, (a, b)))
            params[param] = args
            previous = gid
        if not goals: raise ValueError('処理目的がない')
        solved = _HDS照合(kept, literal_spans, ref_spans, equation_spans)
        consumed = {g.対象 for g in goals}
        req = 汎用要求IR(ir.原文, 資料, kinds, tuple(goals), params,
                        tuple(g.識別子 for g in goals if g.識別子 not in consumed)).固定複製()
        return 目的射影結果(req, kept, 局所解消=solved)
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError, SyntaxError) as exc:
        return 目的射影結果(None, kept, str(exc))
