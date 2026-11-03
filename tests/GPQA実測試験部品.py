"""実データセットを使わない実測器の契約試験用部品。"""
import time,os
from pathlib import Path
class 記憶実行器:
    def __init__(self):self.回数=0
    def __call__(self,番号,問題,選択肢,記録):
        self.回数+=1;記録('人工推論')
        if 問題=='停止':time.sleep(30)
        if 問題=='例外':raise RuntimeError('人工失敗')
        if 問題=='終了':os._exit(7)
        if 問題.startswith('子プロセス:'):
            import subprocess,sys
            p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)'])
            Path(問題.split(':',1)[1]).write_text(str(p.pid));time.sleep(30)
        return {'番号':番号,'予測ラベル':'A','初期予測ラベル':'A','回答済み':True,'初期回答済み':True,
                '終端':'COMMIT','問題束形成回数':1,'記憶回数':self.回数,'選択入力長':len(選択肢)}
class 偽装実行器(記憶実行器):
    def __call__(self,*a):
        r=super().__call__(*a);r['予測ラベル']='Z';return r
class 重複形成器(記憶実行器):
    def __call__(self,*a):
        r=super().__call__(*a);r['問題束形成回数']=2;return r
