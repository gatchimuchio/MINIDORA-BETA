"""目的・入力役割・作用条件を結ぶ契約。計画器に具体的工程列を埋め込まない。"""
from __future__ import annotations
from .役割計画 import 役割作用
from .会話意味 import 意味目的
from .実行回復 import 回復規則


def 会話作用群():
    def 役割(name,kind,params): return (name,意味目的(kind,params))
    def table(p): return p['形式'] in ('JSON','CSV')
    def 情報源(p): return {'資料':p['資料'],'形式':p['形式']}
    def quantity_settings(p,mode):
        return {k:p[k] for k in ('資料','属性','単位','行条件')}|{'方式':mode}
    def search_settings(p,focused):
        query=p['主題']+' '+p['属性']
        if focused: query+=' '+p['単位']+' 数値'
        return {'検索語':query,'必要語':[p['主題'],p['属性']],
                '最低資料数':1,'最大検索回数':1,'最大候補数':3,'最大取得数':3,'最大資料数':2}
    rules=(
        役割作用('比較回答化','会話回答構成','比較回答',
            lambda p:(役割('比較','数量比較',{k:p[k] for k in ('左','右','時点差')}),),
            lambda p:{'詳細':p['詳細']},lambda p:True),
        役割作用('左右数量比較','数量比較','数量比較',
            lambda p:(役割('左','数量',p['左']),役割('右','数量',p['右'])),
            lambda p:{'時点差':p['時点差']},lambda p:True,
            不成立条件=('異単位','異属性','異条件','無断の時点差')),
        役割作用('直下数量選択','表数量解釈','数量',
            lambda p:(役割('文書','構造文書',情報源(p)),),
            lambda p:quantity_settings(p,'直下'),table,費用=1,
            不成立条件=('対象行曖昧','単位未確定','未解釈注記','構造未到達'),
            回復=(回復規則('構造未到達'),)),
        役割作用('入れ子数量選択','表数量解釈','数量',
            lambda p:(役割('文書','構造文書',情報源(p)),),
            lambda p:quantity_settings(p,'入れ子'),lambda p:p['形式']=='JSON',費用=2,
            不成立条件=('対象行曖昧','単位未確定','未解釈注記')),
        役割作用('構造文書読取','文書読取','構造文書',
            lambda p:(役割('原文','原資料',{'資料':p['資料']}),),
            lambda p:{'形式':p['形式']},table),
        役割作用('取得回答化','会話回答構成','取得回答',
            lambda p:(役割('記載','取得数量',{k:p[k] for k in ('主題','属性','単位')}),),
            lambda p:{'詳細':p['詳細']},lambda p:True),
        役割作用('取得記載数量','数値記載解釈','取得数量',
            lambda p:(役割('参照','取得資料',p),),
            lambda p:{'対象':p['主題'],'属性':p['属性'],'単位':p['単位']},lambda p:True,
            不成立条件=('記載不足','記載矛盾','未知の条件')),
        役割作用('取得成立採用','取得報告採用','取得資料',
            lambda p:(役割('報告','取得報告',p),),lambda p:{},lambda p:True,
            回復=(回復規則('取得不足','入力役割','報告',('主題取得',)),)),
        役割作用('主題取得','会話取得報告','取得報告',lambda p:(),
            lambda p:search_settings(p,False),lambda p:True,費用=2,外部読取=True),
        役割作用('焦点取得','会話取得報告','取得報告',lambda p:(),
            lambda p:search_settings(p,True),lambda p:True,費用=3,外部読取=True),
    )
    return rules
