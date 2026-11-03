"""共通三操作を既存HDS通常循環へ接続する。別のループ・判定主体は作らない。"""
from __future__ import annotations
from dataclasses import replace
from .契約 import 関係要求,関係出力束,関係保証,構造要求,構造出力束
from .学習 import 関係学習状態,有効形成を取得,実行経験を形成
from .取得 import 関係を取得
from .変換 import 関係を変換
from .射影 import 関係結果を射影
from ..HDS実行主体 import HDS実行主体,HDS実行状態,HDS関数作用,HDS作用結果,HDS作用状態,HDS終端
from ..統合駆動_v2.政策 import HDS阻害,停止理由
from ..コア.値 import 署名

要求成果='関係駆動/要求'
取得成果='関係駆動/取得'
変換成果='関係駆動/変換'
出力成果='関係駆動/出力'
学習成果='関係駆動/学習更新'
取得済='関係駆動/取得済'
変換済='関係駆動/変換済'
回答済='関係駆動/回答成立'


def 三操作を構成(要求:関係要求,学習状態:関係学習状態,停止要求=None):
    additions=有効形成を取得(学習状態,要求.変換) if isinstance(要求,関係要求) else ()
    # クロージャは不変契約だけ。作用自身はCoreへ書き戻さず、結果として更新を提案する。
    def acquire(局所状態):
        request=局所状態.成果辞書()[要求成果]
        if request!=要求: raise ValueError('要求正本が途中で変わった')
        acquired=関係を取得(request,additions)
        return HDS作用結果(HDS作用状態.成立,追加状態=frozenset({取得済}),成果=((取得成果,acquired),))
    def convert(局所状態):
        values=局所状態.成果辞書()
        converted=関係を変換(要求,values[取得成果],停止要求=停止要求)
        return HDS作用結果(HDS作用状態.成立,追加状態=frozenset({変換済}),成果=((変換成果,converted),))
    def project(局所状態):
        conversion=局所状態.成果辞書()[変換成果]
        projected=関係結果を射影(要求,conversion,additions)
        learned=実行経験を形成(学習状態,要求,conversion,projected) if isinstance(要求,関係要求) else 学習状態
        outputs=((出力成果,projected),(学習成果,learned))
        if projected.状態=='成立':
            return HDS作用結果(HDS作用状態.成立,追加状態=frozenset({回答済}),成果=outputs,
                理由=('関係の導出証明と目的束縛を確認し、内容を射影',))
        reason={'競合':停止理由.証拠競合,'予算枯渇':停止理由.予算枯渇,'停止':停止理由.明示停止}.get(projected.状態,停止理由.観測不足)
        return HDS作用結果(HDS作用状態.保留,成果=outputs,停止要求=True,
            阻害=HDS阻害(reason,'関係駆動/射影',';'.join(projected.未充足)),理由=projected.未充足)
    return (
        HDS関数作用('関係駆動/取得',acquire,出力状態=(取得済,),読取成果=(要求成果,),
                    契約版='取得v1',意味入力署名=lambda s:署名((要求.署名,tuple(r.署名 for r in additions)))),
        HDS関数作用('関係駆動/変換',convert,入力状態=(取得済,),出力状態=(変換済,),読取成果=(取得成果,),
                    契約版='変換v1'),
        HDS関数作用('関係駆動/射影',project,入力状態=(変換済,),出力状態=(回答済,),読取成果=(変換成果,),
                    契約版='射影v1'),
    )


def 関係実行へ接続(中核,問合せ,要求):
    if not isinstance(要求,(関係要求,構造要求)) or 問合せ!=要求.目的: raise ValueError('原要求の目的と構造化契約が不一致')
    old=中核.関係学習状態
    actions=三操作を構成(要求,old,中核.停止要求)
    # 世界の前提は案件内。既存Coreの他案件の知識を暗黙で証拠へ足さない。
    initial=HDS実行状態(目的=(要求.ID,要求.目的),要求状態=frozenset({回答済}),
        成果=((要求成果,要求),),主体状態=(('HDS目的正本',(要求.ID,要求.版,要求.世界ID,要求.署名)),))
    局所結果=HDS実行主体(actions,最大作用回数=中核.最大作用回数,政策=中核.政策,
        観測器=中核.観測器,仮説雛型=中核.仮説雛型,検証器=中核.検証器,最終検証器=中核.最終検証器,
        関係規則=中核.関係規則,未来制約=中核.未来制約,作用供給器=中核.作用供給器,
        停止要求=中核.停止要求,適応記憶=中核._適応記憶).実行(initial)
    values=局所結果.状態.成果辞書(); proposal=values.get(学習成果); output=values.get(出力成果)
    # 最終検証に不合格な結果から成功形成を採用しない。観測された競合隔離だけは維持する。
    if isinstance(proposal,関係学習状態) and isinstance(output,(関係出力束,構造出力束)):
        if 局所結果.終端==HDS終端.採用 or (局所結果.終端==HDS終端.保留 and output.状態=='競合'):
            中核._関係学習状態=proposal
    return 局所結果


def 関係結果を読む(結果):
    """内容採用と外向き返却を分ける。未採用候補は確定回答として出力しない。"""
    values=結果.状態.成果辞書();request=values.get(要求成果)
    if not isinstance(request,(関係要求,構造要求)): raise TypeError('関係実行結果ではない')
    output=values.get(出力成果)
    if isinstance(output,(関係出力束,構造出力束)):
        if 結果.終端==HDS終端.採用 or output.状態!='成立': return output
        status='停止' if 結果.停止種別==停止理由.明示停止 else '予算枯渇' if 結果.停止種別==停止理由.予算枯渇 else '未観測'
        if isinstance(request,構造要求):
            return 構造出力束(request.ID,request.署名,status,None,('全体採否未成立',*結果.理由))
        return 関係出力束(request.ID,request.署名,status,(),未充足=('全体採否未成立',*結果.理由),照合数=output.照合数)
    status='予算枯渇' if 結果.停止種別==停止理由.予算枯渇 else '停止' if 結果.停止種別==停止理由.明示停止 else '未観測'
    if isinstance(request,構造要求):
        return 構造出力束(request.ID,request.署名,status,None,結果.理由 or ('構造出力へ未到達',))
    return 関係出力束(request.ID,request.署名,status,(),未充足=結果.理由 or ('関係出力へ未到達',))
