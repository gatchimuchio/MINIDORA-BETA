'対象数や値を資料として扱い、数量群の作用を既存計画・実行へ接続する。'
from __future__ import annotations
from .役割計画 import 役割作用
from .会話意味 import 意味目的
from .会話能力接続 import _入力
from .会話数量 import 会話処理不成立
from .数量集合 import 数量集合版, 数量群を処理
from .能力合成 import 登録能力
from .製品版.型 import 能力結果


class 数量集合モジュール:
    名前='数量集合';版=数量集合版;優先度=0
    def 判定(self,文脈): return 1.0
    def 実行(self,文脈):
        try:
            values,settings=_入力(文脈)
            return 数量群を処理(values,settings)
        except 会話処理不成立 as exc:
            return 能力結果(False,'',保留理由=str(exc))
        except (ValueError,TypeError,KeyError,AttributeError,RecursionError,OverflowError) as exc:
            return 能力結果(False,'',保留理由='会話失敗:入力不正:'+str(exc))
    def 登録(self): return 登録能力(self)


def 集合作用群():
    def group(p):
        required={'対象','操作','時点差','選別','除外資料','供給'}
        if not required<=set(p) or set(p)-required-{'詳細','形式','手順'} or p['供給'] not in ('資料','取得') or type(p['対象']) not in (list,tuple) or not 1<=len(p['対象'])<=8:
            raise ValueError('集合目的の対象役割・引数不正')
        return {k:p[k] for k in required}
    def 役割(p):
        group(p)
        return tuple((f'対象:{i}',意味目的('数量' if p['供給']=='資料' else '取得数量',value)) for i,value in enumerate(p['対象']))
    return (
        役割作用('集合回答化','会話回答構成','集合回答',
            lambda p:(('集合',意味目的('数量集合',group(p))),),
            lambda p:{'詳細':p['詳細'],'形式':p['形式'],'手順':p['手順']},lambda p:True),
        役割作用('数量集合処理','数量集合','数量集合',役割,
            lambda p:{k:p[k] for k in ('操作','時点差','選別','除外資料')},lambda p:True,
            不成立条件=('異属性','異単位','異条件','無断の時点差','対象重複','未解釈注記')),
    )
