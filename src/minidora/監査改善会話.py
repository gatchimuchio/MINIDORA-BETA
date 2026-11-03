'追加能力の状態付き会話。既存の役割計画・統合採用・原記録失効を使う。\n\n単独時は改善統合能力群だけを明示登録する。既存の全能力構成へ接続するときは\n同じ統合セッションを渡す。別模型核、疑似HDS、外部通信の代役を作らない。\n'
from __future__ import annotations
from copy import deepcopy
from dataclasses import dataclass, asdict
from threading import Lock
from .統合実行 import 統合セッション
from .能力合成 import _結果辞書, _符号化
from .能力結果復元 import 能力結果を復元
from .会話意味 import 意味指紋
from .製品版.型 import 能力結果, 参照資料
from .命題解釈 import 命題を読む
from .監査改善接続 import 拡張命題を検討, 改善回答を検査, 改善回答版
from .監査改善計画 import 改善統合能力群, 改善目的を計画, 報告版, 改善計画版
from .監査改善会話解釈 import 改善発話を解釈, JSONを厳格に読む, 改善会話解釈版

改善会話版 = 'MINIDORA-監査改善会話-v0.3'
契約版 = {'会話': 改善会話版, '解釈': 改善会話解釈版, '計画': 改善計画版,
          '回答': 改善回答版, **報告版}


@dataclass(frozen=True, slots=True)
class 改善会話応答:
    状態: str
    本文: str
    理由: str = ''
    追跡: dict | None = None
    結果: 能力結果 | None = None

    @property
    def 成立(self):
        return self.状態 == '合格'

    def 辞書化(self):
        return {'状態': self.状態, '本文': self.本文, '理由': self.理由,
                '追跡': deepcopy(self.追跡 or {}), '結果': _結果辞書(self.結果) if self.結果 else None}


def _返答印(結果):
    # 所有ID・実行時間は再構築ごとに変わる。回答の意味・本文・採否は省略しない。
    return 意味指紋({'状態': 結果.状態, '本文': 結果.本文, '理由': 結果.理由,
                    '結果': _結果辞書(結果.結果) if 結果.結果 else None})


class 監査改善会話セッション:
    def __init__(self, セッションID='default', *, 統合=None, 最大発話=128):
        if type(最大発話) is not int or not 1 <= 最大発話 <= 128:
            raise ValueError('改善会話の発話上限不正')
        self.統合 = (統合セッション(セッションID, 基底能力=改善統合能力群()) if 統合 is None else 統合)
        if type(self.統合) is not 統合セッション or self.統合.起点().セッションID != セッションID:
            raise ValueError('同じセッションIDの実統合セッションが必要')
        actual = {r['名前']: r for r in self.統合.能力一覧()}
        for r in 改善統合能力群():
            if actual.get(r.モジュール.名前) != {'名前': r.モジュール.名前, '版': r.モジュール.版, '外部読取': False}:
                raise ValueError('改善能力の未登録・版不一致・外部作用化')
        self._単独 = 統合 is None
        point = self.統合.起点()
        self._生成所有 = (point.所有ID, point.世代)
        self._ID, self._上限 = セッションID, 最大発話
        self._資料 = {}
        self._目的 = None
        self._保留 = None
        self._成果 = []
        self._履歴 = []
        self._ロック = Lock()

    def _有効(self, row):
        now = self.統合.起点()
        if (now.所有ID, now.世代) != tuple(row['所有']):
            return False
        if any(name not in self._資料 or self._資料[name]['版'] != version
               for name, version in row['資料版'].items()):
            return False
        try:
            stored = self.統合.原記録(row['記録ID'])
            return stored['現行'] and 意味指紋(stored['内容']) == row['回答印']
        except ValueError:
            return False

    def _意味状態(self):
        return {'資料版': {k: v['版'] for k, v in sorted(self._資料.items())},
                '最後目的': deepcopy(self._目的), '保留目的': deepcopy(self._保留),
                '成果': [{'資料版': r['資料版'], '目的': r['目的'], '詳細': r['詳細'],
                          '回答印': r['回答印'], '現行': self._有効(r)} for r in self._成果]}

    def 状態(self):
        if not self._ロック.acquire(blocking=False):
            raise ValueError('改善会話の処理中')
        try:
            return deepcopy({'版': 改善会話版, '発話数': len(self._履歴), **self._意味状態()})
        finally:
            self._ロック.release()

    def 対応する(self, text, *, 継続許可=True):
        """既存会話の振分用。資料名の衝突を勝手に解決しない。実解釈は後段で全文確認。"""
        if type(text) is not str or type(継続許可) is not bool:
            return False
        value = text.strip()
        if value.startswith(('命題資料', '仮説資料', '介入資料', '本文資料', '監査改善の')):
            return True
        if value.startswith(('資料「', '資料『')):
            from .命題句 import 引用を切り出す
            try:
                name, _ = 引用を切り出す(value, 2)
                return name in self._資料
            except ValueError:
                return False
        if 継続許可 and bool(self._保留 or self._目的):
            from .依頼表現 import 再説明依頼を読む
            try:
                if 再説明依頼を読む(text) is not None:
                    return True
            except ValueError:
                return False
        return 継続許可 and bool(self._保留 or self._目的) and value.startswith(
            ('観測を', '候補を', '介入を', '問いを', '問い候補', '資料候補', '照応距離を',
             '述語別名を', '続けて', 'もう一度', '短く説明して', '詳しく説明して'))

    def _登録(self, command):
        name, kind, 作用 = command['資料'], command['種類'], command['行為']
        if 作用 == '登録' and name in self._資料:
            raise ValueError('既存資料には更新を明示する')
        if 作用 == '更新' and name not in self._資料:
            raise ValueError('更新対象の資料がない')
        if name in self._資料 and self._資料[name]['種類'] != kind:
            raise ValueError('資料更新で種類を変更しない')
        候補 = deepcopy(self._資料)
        packet = {k: deepcopy(command[k]) for k in ('種類', '本文', 'データ', '原文対応')}
        packet['版'] = 意味指紋({'名前': name, **packet})
        候補[name] = packet
        if len(候補) > 16 or len(_符号化(候補)) > 300_000:
            raise ValueError('保持資料の数・バイト上限')
        stale = tuple(r['記録ID'] for r in self._成果 if name in r['資料版']
                      and r['資料版'][name] != packet['版'] and self._有効(r))
        if stale:
            self.統合.記録を失効(self.統合.起点(), stale, 理由='利用者の明示資料更新:' + name)
        # 上限・原記録失効の確定後だけ入替える。旧原記録は削除しない。
        self._資料 = 候補
        return 改善会話応答('合格', f'{kind}資料「{name}」を{作用}しました。内容の真実性は認定していません。',
                         追跡={'版': 改善会話版, '資料版': packet['版'], '失効成果数': len(stale),
                               '原文対応': packet['原文対応']})

    def _確認(self, task, reason, message, *, candidates=()):
        versions = {task['資料']: self._資料[task['資料']]['版']} if task['資料'] in self._資料 else {}
        self._保留 = {'目的': deepcopy(task), '理由': reason, '資料版': versions, '候補': list(candidates)}
        return 改善会話応答('確認待ち', message, reason, {'版': 改善会話版, '保留目的': deepcopy(self._保留)})

    def _要求(self, task):
        情報源, kind = self._資料[task['資料']], task['種類']
        if not (情報源['種類'] in ('命題', '本文') if kind == '読解' else 情報源['種類'] == kind):
            raise ValueError('目的と資料種類の不一致')
        permitted = {'命題': {'問い', '問い候補', '資料候補', '照応距離', '述語別名'},
                     '仮説': {'観測', '仮説候補'}, '介入': {'介入'},
                     '読解': {'問い', '述語別名'}}[kind]
        if not set(task['変更']) <= permitted:
            raise ValueError('この目的に適用できない訂正欄')
        request = ({'資料': [{'名前': task['資料'], '本文': 情報源['本文']}]}
                   if kind in ('命題', '読解') else deepcopy(情報源['データ']))
        request.update(deepcopy(task['変更']))
        return request

    def _実行(self, task, original, stop, *, detail=True, previous=None):
        if task['資料'] not in self._資料:
            return self._確認(task, '資料不足', f'資料「{task["資料"]}」の登録が必要です。登録後に「続けて」と指定してください。')
        request = self._要求(task)
        key = {'命題': '問い', '仮説': '観測', '介入': '介入', '読解': None}[task['種類']]
        if key is not None and key not in request:
            return self._確認(task, '入力不足:' + key, key + 'を明示してください。例：' +
                {'問い': '問いを「P」にして', '観測': '観測を「Wet」にして', '介入': '介入を「B=偽」にして'}[key])
        if task['種類'] == '命題' and previous is None:
            try:
                candidates = 命題を読む(request['問い'])
                if len(candidates) > 1 and '問い候補' not in request:
                    choices = [{'番号': i + 1, '読み': c.読み} for i, c in enumerate(candidates)]
                    return self._確認(task, '問い候補未確定', '問いの読みが複数あります。\n' +
                        '\n'.join(f'{r["番号"]}. {r["読み"]}' for r in choices) + '\n例：問い候補1で続けて', candidates=choices)
                preview = 拡張命題を検討(request)['判定結果']
                if preview['判定'] == '解釈依存' and not request.get('資料候補'):
                    choices = [{'番号': c['場合'], '判定': c['判定結果']['判定'],
                                '選択': c['選択'], '照応解消': c['照応解消']} for c in preview['場合別']]
                    return self._確認(task, '資料候補未確定', '資料の読みで結論が分岐します。\n' +
                        '\n'.join(f'{r["番号"]}. {r["判定"]}：{r["選択"]}' for r in choices) + '\n例：資料候補1で続けて', candidates=choices)
            except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
                self._保留 = {'目的': deepcopy(task), '理由': '意味未確定',
                              '資料版': {task['資料']: self._資料[task['資料']]['版']}, '候補': []}
                return 改善会話応答('保留', '命題の未解釈部分を保持します。' + str(exc), '意味未確定',
                                   {'版': 改善会話版, '保留目的': deepcopy(self._保留)})
        情報源 = self._資料[task['資料']]
        ref = 参照資料('改善資料:' + 情報源['版'], task['資料'], '利用者提供資料', 本文=情報源['本文'])
        value = 能力結果(True, original, 根拠=(ref.識別子,), 参照=(ref,), データ=request)
        report, dependencies = None, ()
        if previous is not None:
            if not self._有効(previous):
                raise ValueError('再説明の元成果は失効しています。「もう一度」で再検討してください')
            old = 能力結果を復元(self.統合.原記録(previous['記録ID'])['内容'])
            if not 改善回答を検査(old.データ):
                raise ValueError('保存回答の再計算不一致')
            report = 能力結果(True, '保存報告', 参照=old.参照, データ=old.データ['報告'])
            dependencies = (previous['記録ID'],)
        start = self.統合.起点()
        planned = 改善目的を計画(task['種類'], value, self.統合.能力一覧(), 詳細=detail, 元報告=report)
        packed = self.統合.準備(planned.計画, planned.資料, 依頼文=original, 依存記録=dependencies)
        if packed.起点 != start:
            raise ValueError('意味解釈中に統合状態が変更された')
        executed = self.統合.実行(packed, 停止要求=stop)
        追跡 = {'版': 改善会話版, '意味目的': deepcopy(task), '要求': request, '資料版': 情報源['版'],
                 '目的印': planned.目的印, '工程作用': list(planned.工程作用),
                 '採用記録ID': list(executed.採用記録ID), '起点': asdict(executed.起点),
                 '更新後': asdict(executed.更新後), '統合状態': executed.状態,
                 '合成監査印': executed.実行.ルートハッシュ if executed.実行 else '',
                 '合成監査整合': executed.実行.監査整合() if executed.実行 else False}
        if not executed.成立:
            # 不成立を別の質問や弱い制約へ置換しない。停止時は会話目的も変えない。
            if executed.状態 != '中止':
                self._保留 = {'目的': deepcopy(task), '理由': '実行不成立',
                              '資料版': {task['資料']: 情報源['版']}, '候補': []}
            return 改善会話応答(executed.状態, '検討を採用できませんでした。' + executed.理由,
                             executed.理由, 追跡)
        結果 = executed.出力[0][1]
        receipt = executed.採用記録ID[0]
        self._成果.append({'記録ID': receipt, '目的': deepcopy(task), '詳細': detail,
                           '資料版': {task['資料']: 情報源['版']}, '回答印': 意味指紋(_結果辞書(結果)),
                           '所有': [executed.更新後.所有ID, executed.更新後.世代]})
        self._目的, self._保留 = deepcopy(task), None
        return 改善会話応答('合格', 結果.本文, 追跡=追跡, 結果=結果)

    def _処理(self, command, stop):
        作用, original = command['行為'], command['原文']
        if 作用 in ('登録', '更新'):
            return self._登録(command)
        if 作用 == '状態':
            # 可変の所有IDではなく、意味状態を表示する。
            return 改善会話応答('合格', _符号化(self._意味状態()).decode('utf-8'))
        if 作用 == '取消':
            self._保留 = None
            return 改善会話応答('合格', '監査改善の確認待ちを取り消しました。採用済み原記録は保持します。')
        if 作用 == '検討':
            task = {k: deepcopy(command[k]) for k in ('種類', '資料', '変更')}
            task['起点発話'] = original
            if '詳細' in command:
                task['詳細'] = command['詳細']
            return self._実行(task, original, stop, detail=task.get('詳細', True))
        if 作用 == '再表現':
            if self._保留:
                raise ValueError('未解決の目的があります。確認を完了又は明示取消してから再説明する')
            if not self._成果:
                raise ValueError('再説明する採用成果がない')
            row = self._成果[-1]
            if not self._有効(row):
                raise ValueError('元成果が失効しています。「もう一度」で再検討してください')
            if command.get('相対指定') and row['詳細'] == command['詳細']:
                raise ValueError('現在の表示段階からの追加変更は未対応です。短い説明と詳しい説明を切り替えられます')
            return self._実行(row['目的'], original, stop, detail=command['詳細'], previous=row)
        target = deepcopy(self._保留['目的'] if self._保留 else self._目的)
        if target is None:
            raise ValueError('継続・訂正する目的がない')
        if 作用 == '選択':
            pending = self._保留
            expected = command['欄'] + '未確定'
            if pending is None or pending['理由'] != expected:
                raise ValueError('対応する意味候補の確認待ちがない')
            current = {name: self._資料[name]['版'] for name in pending['資料版'] if name in self._資料}
            if current != pending['資料版']:
                raise ValueError('確認中に資料が更新されています。「もう一度」で候補を再生成してください')
            if command['番号'] not in {c['番号'] for c in pending['候補']}:
                raise ValueError('提示された候補番号の範囲外')
            target['変更'][command['欄']] = command['番号']
        elif 作用 == '訂正':
            target['変更'].update(command['変更'])
            if set(command['変更']) & {'問い', '照応距離', '述語別名'}:
                target['変更'].pop('資料候補', None)
                if '問い' in command['変更']:
                    target['変更'].pop('問い候補', None)
        elif 作用 == '継続':
            if not self._保留:
                raise ValueError('確認待ちの目的がない')
            versions = self._保留['資料版']
            if any(n not in self._資料 or self._資料[n]['版'] != v for n, v in versions.items()):
                raise ValueError('確認中の資料版が変わっています。「もう一度」で再検討してください')
        elif 作用 == '再実行':
            # 旧資料上の選択番号を新しい資料へ持ち越さない。
            if self._保留:
                versions = self._保留['資料版']
            else:
                versions = self._成果[-1]['資料版'] if self._成果 else {}
            if any(n not in self._資料 or self._資料[n]['版'] != v for n, v in versions.items()):
                target['変更'].pop('資料候補', None)
                target['変更'].pop('問い候補', None)
        else:
            raise ValueError('未対応の会話行為')
        return self._実行(target, original, stop, detail=target.get('詳細', True))

    def 応答(self, 原文, *, 停止要求=None):
        if not self._ロック.acquire(blocking=False):
            return 改善会話応答('保留', '同じ改善会話の処理中です。', '処理中')
        try:
            self.統合._停止(停止要求)
            if type(原文) is not str or not 原文.strip() or len(原文) > 8192:
                raise ValueError('会話原文の型・上限')
            if len(self._履歴) >= self._上限 or len(_符号化(self._履歴)) + len(原文.encode('utf-8')) + 1024 > 1_000_000:
                return 改善会話応答('保留', '改善会話の保存予算上限です。原記録を自動削除していません。', '保存予算上限')
            try:
                command = 改善発話を解釈(原文)
                self.統合._停止(停止要求)
                結果 = self._処理(command, 停止要求)
                if '解釈根拠' in command:
                    from dataclasses import replace
                    結果 = replace(結果, 追跡={**(結果.追跡 or {}), '依頼解釈': command['解釈根拠'], '原依頼': 原文})
            except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
                結果 = 改善会話応答('保留', '処理を確定しません。' + str(exc), '入力・意味・状態不成立')
            if 結果.状態 != '中止':
                event = {'原文': 原文, '状態': 結果.状態, '返答印': _返答印(結果),
                         '前ハッシュ': self._履歴[-1]['ハッシュ'] if self._履歴 else 意味指紋(契約版)}
                event['ハッシュ'] = 意味指紋(event)
                self._履歴.append(event)
            return 結果
        except InterruptedError:
            return 改善会話応答('中止', '停止要求により採用前に中止しました。', '停止要求')
        except (ValueError, TypeError, RecursionError) as exc:
            return 改善会話応答('保留', '処理を確定しません。' + str(exc), '入力・停止判定不正')
        finally:
            self._ロック.release()

    def 保存文字列(self):
        if not self._単独:
            raise ValueError('共有統合セッションの全履歴を、この部分会話だけで保存・復元しない')
        if not self._ロック.acquire(blocking=False):
            raise ValueError('改善会話の処理中')
        try:
            point = self.統合.起点()
            if ((point.所有ID, point.世代) != self._生成所有
                    or len(self.統合._履歴) != len(self._成果)
                    or any(self._有効(r) != all(self._資料.get(n, {}).get('版') == v
                        for n, v in r['資料版'].items()) for r in self._成果)):
                raise ValueError('会話行為外から統合状態が変更されています。部分履歴だけでは保存できません')
            packet = {'版': 改善会話版, '契約': 契約版, 'セッションID': self._ID,
                      '最大発話': self._上限, '履歴': self._履歴,
                      '状態印': 意味指紋(self._意味状態())}
            packet['記録SHA256'] = 意味指紋(packet)
            return _符号化(packet).decode('utf-8')
        finally:
            self._ロック.release()

    @classmethod
    def 復元(cls, text, *, 期待セッションID=None):
        """入力行為を純粋な限定能力で再生し、返答・状態も再構築する。旧所有権は再利用しない。"""
        packet = JSONを厳格に読む(text)
        fields = {'版', '契約', 'セッションID', '最大発話', '履歴', '状態印', '記録SHA256'}
        if (type(packet) is not dict or set(packet) != fields or packet['版'] != 改善会話版
                or _符号化(packet['契約']) != _符号化(契約版)
                or type(packet['履歴']) is not list or len(packet['履歴']) > 128):
            raise ValueError('保存会話の欄・版・上限不正。別版を自動移行しない')
        seal = packet.pop('記録SHA256')
        if seal != 意味指紋(packet):
            raise ValueError('保存会話のハッシュ不一致')
        if 期待セッションID is not None and packet['セッションID'] != 期待セッションID:
            raise ValueError('別セッションの保存会話')
        instance = cls(packet['セッションID'], 最大発話=packet['最大発話'])
        for event in packet['履歴']:
            if type(event) is not dict or set(event) != {'原文', '状態', '返答印', '前ハッシュ', 'ハッシュ'}:
                raise ValueError('保存発話の欄不正')
            before = len(instance._履歴)
            instance.応答(event['原文'])
            if len(instance._履歴) != before + 1 or _符号化(instance._履歴[-1]) != _符号化(event):
                raise ValueError('保存発話と純粋再実行の不一致')
        if 意味指紋(instance._意味状態()) != packet['状態印']:
            raise ValueError('会話状態の再構築不一致')
        return instance
