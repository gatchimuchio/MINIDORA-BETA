from __future__ import annotations

from dataclasses import dataclass, field
from typing import Protocol, Sequence
from collections import Counter
from math import log1p, sqrt
import re
import json

from .意味字句 import 意味語
from .言語構造 import 言語関係構造, 意味列, 言語関係抽出, 問い候補関係形成, 問題前提関係抽出
from .能力作用則 import 証拠状態合計寄与, 証拠状態照合, _端点意味同一, _述語対応, _比較成立域

LLM成立規定リポジトリ = "https://github.com/gatchimuchio/LLM-Constitutive-Specification"
LLM成立規定参照コミット = "306ff834e5ac7e7e958b513db723a24619c8895a"
LLM成立規定版 = "2026-08-27-成立規定-3"
LLM成立意味区別 = ("独立対象","文脈依存関係","関係再利用","言語対応","局所対応")
LLM構成再現区別 = (
    "状態分離・保持・更新",
    "意味・関係同一性追跡",
    "未確定差の共存",
    "寄与調整と確定分離",
    "構成連鎖・再作用・再結合",
    "再作用閉包・終端成立差",
    "形成済み関係の保持と作用機構との分離",
)

@dataclass(frozen=True, slots=True)
class 言語状態:
    内容: str
    言語体系: str = "自然言語:ja"
    識別子: str = ""
    関係構造: tuple[言語関係構造,...] = ()
    表層再解析可: bool = True
    証拠利用可: bool = True
    def __post_init__(self):
        if not isinstance(self.表層再解析可,bool) or not isinstance(self.証拠利用可,bool):
            raise TypeError("言語状態の証拠境界フラグはboolである必要がある")
        if not self.言語体系.strip(): raise ValueError("言語体系は空にできない")
        if not isinstance(self.内容,str): raise TypeError("言語状態.内容は文字列である必要がある")

@dataclass(frozen=True, slots=True)
class 内部言語状態:
    表層: str
    言語体系: str
    意味語集合: frozenset[str]
    識別子: str = ""
    意味語列: tuple[str,...] = ()
    関係構造: tuple[言語関係構造,...] = ()
    表層再解析可: bool = True
    証拠利用可: bool = True
    @property
    def 構造署名(self):
        return (self.言語体系,self.意味語列,tuple(x.署名 for x in self.関係構造),
                self.表層再解析可,self.証拠利用可)

@dataclass(frozen=True, slots=True)
class 文脈付き言語状態:
    現在: 内部言語状態
    履歴: tuple[内部言語状態,...] = ()
    条件: tuple[str,...] = ()
    参照状態: tuple[内部言語状態,...] = ()
    @property
    def 意味語集合(self):
        out=set(self.現在.意味語集合)
        for s in self.履歴: out.update(s.意味語集合)
        for c in self.条件: out.update(意味語(c))
        return frozenset(out)

@dataclass(frozen=True, slots=True)
class 成立候補:
    候補ID: str
    状態: 言語状態
    def __post_init__(self):
        if not self.候補ID.strip(): raise ValueError("候補IDは空にできない")

@dataclass(frozen=True, slots=True)
class 関係寄与:
    関係名: str
    差: int
    根拠: tuple[str,...] = ()

@dataclass(frozen=True, slots=True)
class 成立差:
    候補ID: str
    差: int
    寄与: tuple[関係寄与,...] = ()

@dataclass(frozen=True, slots=True)
class 模型検査点:
    段階: str
    候補差: tuple[tuple[str,int],...]
    未確定候補ID: tuple[str,...] = ()
    再利用元: tuple[str,...] = ()

@dataclass(frozen=True, slots=True)
class 模型統計:
    寄与状態生成数: int = 0
    寄与状態再利用数: int = 0
    検査点再活性数: int = 0
    大域再照合数: int = 0
    候補横断更新数: int = 0
    再作用回数: int = 0
    終端遍歴数: int = 0

@dataclass(frozen=True, slots=True)
class 模型結果:
    文脈: 文脈付き言語状態
    候補差: tuple[成立差,...]
    最有力候補ID: str|None
    同率候補ID: tuple[str,...] = ()
    検査点: tuple[模型検査点,...] = ()
    統計: 模型統計 = 模型統計()
    参照最有力候補ID: str|None = None
    参照同率候補ID: tuple[str,...] = ()
    def 候補辞書(self): return {x.候補ID:x.差 for x in self.候補差}
    def 参照候補辞書(self):
        out={x.候補ID:0 for x in self.候補差}
        for row in self.候補差:
            out[row.候補ID]=sum(c.差 for c in row.寄与 if c.関係名.startswith(("参照関係寄与","候補共同参照","候補共同再照合")))
        return out

class 言語対応:
    def 内部化(self, 状態:言語状態)->内部言語状態:
        parsed=言語関係抽出(状態.内容,状態.言語体系) if 状態.表層再解析可 and 状態.証拠利用可 else ()
        rels=[]; seen=set()
        for item in (*parsed,*(状態.関係構造 if 状態.証拠利用可 else ())):
            if item.署名 not in seen:
                seen.add(item.署名); rels.append(item)
        return 内部言語状態(状態.内容,状態.言語体系,意味語(状態.内容),状態.識別子,意味列(状態.内容),tuple(rels),状態.表層再解析可,状態.証拠利用可)
    def 文脈化(self,現在,履歴:Sequence[言語状態]=(),条件:Sequence[str]=(),参照状態:Sequence[言語状態]=()):
        for 状態 in (*履歴,*参照状態):
            if 状態.言語体系!=現在.言語体系: raise ValueError("同一の文脈評価内で言語体系を無言混在させない")
        # 同一の意味・関係構造を持つDataを、出典IDが違うだけで複数票へ増幅しない。
        references=[]; seen_参照=set()
        for 状態 in 参照状態:
            internal=self.内部化(状態)
            signature=internal.構造署名
            if signature in seen_参照: continue
            seen_参照.add(signature); references.append(internal)
        return 文脈付き言語状態(self.内部化(現在),tuple(self.内部化(x) for x in 履歴),tuple(str(x) for x in 条件),tuple(references))

class 模型関係(Protocol):
    名称:str
    def 評価(self,文脈:文脈付き言語状態,候補:内部言語状態)->関係寄与|None:...

@dataclass(frozen=True, slots=True)
class 関係規則:
    名称:str
    文脈必須:frozenset[str]=field(default_factory=frozenset)
    候補必須:frozenset[str]=field(default_factory=frozenset)
    文脈禁止:frozenset[str]=field(default_factory=frozenset)
    候補禁止:frozenset[str]=field(default_factory=frozenset)
    差:int=1
    根拠:tuple[str,...]=()
    def 評価(self,文脈,候補):
        文脈=文脈.意味語集合; cand=候補.意味語集合
        if not self.文脈必須.issubset(文脈) or not self.候補必須.issubset(cand): return None
        if self.文脈禁止.intersection(文脈) or self.候補禁止.intersection(cand): return None
        if not (self.文脈必須 or self.候補必須 or self.文脈禁止 or self.候補禁止): return None
        return 関係寄与(self.名称,int(self.差),self.根拠)

@dataclass(frozen=True, slots=True)
class 意味連続関係:
    名称:str="意味連続"; 関係語重み:int=2
    def 評価(self,文脈,候補):
        shared=文脈.意味語集合.intersection(候補.意味語集合)
        if not shared:return None
        r=sum(1 for t in shared if t.startswith("rel:")); d=(len(shared)-r)+r*self.関係語重み
        return 関係寄与(self.名称,d,tuple(f"共有:{t}" for t in sorted(shared))) if d>0 else None

@dataclass(frozen=True, slots=True)
class 順序連続関係:
    名称:str="順序連続"
    @staticmethod
    def _二連(seq): return frozenset(zip(seq,seq[1:]))
    def 評価(self,文脈,候補):
        shared=self._二連(文脈.現在.意味語列).intersection(self._二連(候補.意味語列))
        return 関係寄与(self.名称,len(shared),tuple(f"順序:{a}>{b}" for a,b in sorted(shared))) if shared else None

def _端点一致(a,b): return bool(a and b and a.intersection(b))
def _関係同型(a,b,*,肯否無視=False,条件無視=True):
    if a.種別!=b.種別:return False
    if not _端点一致(a.始点,b.始点) or not _端点一致(a.終点,b.終点):return False
    if not 肯否無視 and a.肯定!=b.肯定:return False
    if not 条件無視 and a.条件!=b.条件:return False
    return True

@dataclass(frozen=True, slots=True)
class 有向関係整合:
    名称:str="有向関係整合"; 一致差:int=4; 逆向差:int=-2
    def 評価(self,文脈,候補):
        score=0; ev=[]
        for base in 文脈.現在.関係構造:
            for item in 候補.関係構造:
                if base.種別!=item.種別: continue
                if _端点一致(base.始点,item.始点) and _端点一致(base.終点,item.終点):score+=self.一致差;ev.append(f"有向一致:{base.種別}")
                elif _端点一致(base.始点,item.終点) and _端点一致(base.終点,item.始点):score+=self.逆向差;ev.append(f"逆向:{base.種別}")
        return 関係寄与(self.名称,score,tuple(ev)) if score else None

@dataclass(frozen=True, slots=True)
class 肯否整合関係:
    名称:str="肯否整合"; 一致差:int=2; 不一致差:int=-3
    def 評価(self,文脈,候補):
        score=0;ev=[]
        for base in 文脈.現在.関係構造:
            for item in 候補.関係構造:
                if not _関係同型(base,item,肯否無視=True):continue
                if base.肯定==item.肯定:score+=self.一致差;ev.append(f"肯否一致:{base.種別}")
                else:score+=self.不一致差;ev.append(f"肯否不一致:{base.種別}")
        return 関係寄与(self.名称,score,tuple(ev)) if score else None

@dataclass(frozen=True, slots=True)
class 履歴近接関係:
    名称:str="履歴近接"; 最大参照履歴:int=8
    def 評価(self,文脈,候補):
        score=0;ev=[]
        for distance,状態 in enumerate(reversed(文脈.履歴[-self.最大参照履歴:]),1):
            shared=状態.意味語集合.intersection(候補.意味語集合)
            if shared:
                w=max(1,4-distance);score+=len(shared)*w;ev.append(f"履歴距離{distance}:{len(shared)}")
        return 関係寄与(self.名称,score,tuple(ev)) if score else None

@dataclass(frozen=True, slots=True)
class 条件結合関係:
    名称:str="条件結合"; 一致差:int=3; 不一致差:int=-1
    def 評価(self,文脈,候補):
        score=0;ev=[]
        for base in 文脈.現在.関係構造:
            if not base.条件:continue
            for item in 候補.関係構造:
                if not _関係同型(base,item,条件無視=True):continue
                if base.条件==item.条件:score+=self.一致差;ev.append(f"条件一致:{base.種別}")
                else:score+=self.不一致差;ev.append(f"条件不一致:{base.種別}")
        return 関係寄与(self.名称,score,tuple(ev)) if score else None

def _照合対象関係(文脈, 候補):
    # Compilerが構造を供給した場合はそれを保持する。原文の空所束縛は原子的候補に限る。
    if 候補.関係構造:
        return 候補.関係構造
    return 問い候補関係形成(文脈.現在.表層,候補.表層,文脈.現在.言語体系)


def _照合証拠関係(文脈):
    return tuple(item for ref in 文脈.参照状態 if ref.証拠利用可 for item in ref.関係構造) + 問題前提関係抽出(
        文脈.現在.表層,文脈.現在.言語体系)


@dataclass(frozen=True, slots=True)
class 参照関係寄与作用:
    名称: str = "参照関係寄与"

    def 評価(self, 文脈, 候補):
        targets = _照合対象関係(文脈,候補)
        relations = _照合証拠関係(文脈)
        if not targets:
            return None
        状態 = 証拠状態照合(targets,relations)
        diagnostics = (f"関係被覆:支持={状態.支持};反証={状態.反証};未観測={状態.未観測};矛盾={状態.矛盾}",)
        if 状態.矛盾:
            return 関係寄与(self.名称,0,(*diagnostics,"参照矛盾:支持と反証が併存"))
        # 連言は全節支持で成立、一節の明示反証で反証。未観測は反証ではない。
        score = 2 if 状態.完全支持 else -2 if 状態.反証 else 0
        if "選択意図=反転" in 文脈.条件:
            score = -score
        sources = tuple(sorted(set(ref.識別子 or "anonymous" for ref in 文脈.参照状態 if ref.証拠利用可)))
        return 関係寄与(self.名称,score,(*diagnostics,"参照再結合:"+json.dumps(sources,ensure_ascii=False)))


def _順序識別特徴(terms,sequence):
    pairs={"順序組:"+a+"\x1f"+b for a,b in zip(sequence,sequence[1:]) if a!=b}
    return frozenset(terms)|frozenset(pairs)

def _列挙参照解決(文脈,候補群):
    """候補中の列挙記号を、同じ問いに宣言された対象に束縛する。"""
    text=文脈.現在.表層
    if re.search(r"\b(?:how many|number of)\b|何個|いくつ|何件",text,re.I):
        return 候補群
    matches=list(re.finditer(r"(?m)^\s*\(?([A-Za-z]|[1-9][0-9]?)\s*[.)]\s+",text))
    if len(matches)<2:
        return 候補群
    labels=[m.group(1).casefold() for m in matches]
    if len(set(labels))!=len(labels):
        return 候補群
    mapping={}
    for i,m in enumerate(matches):
        end=matches[i+1].start() if i+1<len(matches) else len(text)
        body=text[m.end():end].strip()
        body=re.split(r"\n\s*\n",body,maxsplit=1)[0].strip()
        if not body:
            return 候補群
        mapping[labels[i]]=body
    結果=[]
    for cid,状態 in 候補群:
        raw=状態.表層.strip()
        if re.fullmatch(r"[A-Za-z0-9,;\s()]+",raw) is None:
            結果.append((cid,状態));continue
        parts=[p.casefold() for p in re.findall(r"[A-Za-z]+|[0-9]+",raw)]
        parts=[p for p in parts if p not in {'and'}]
        if not parts or any(p not in mapping for p in parts):
            結果.append((cid,状態));continue
        expanded=" ".join(mapping[p] for p in parts)
        if not (状態.証拠利用可 and 状態.表層再解析可):
            結果.append((cid,状態));continue
        結果.append((cid,内部言語状態(expanded,状態.言語体系,意味語(expanded),状態.識別子,
                         意味列(expanded),言語関係抽出(expanded,状態.言語体系),True,True)))
    return tuple(結果)

@dataclass(frozen=True, slots=True)
class 候補共同参照作用:
    名称:str="候補共同参照"
    def _evaluate(self,文脈,候補群,*,name):
        '問いと局所資料の同時対応を、全候補間の対照で評価する。\n\n        出現重みは一requestの参照だけから計算する。正解・学習済み重み・分野辞書は使わない。\n        '
        if not 候補群:
            return {}
        候補群=_列挙参照解決(文脈,候補群)
        relations=tuple(x for ref in 文脈.参照状態 if ref.証拠利用可 for x in ref.関係構造)+問題前提関係抽出(文脈.現在.表層,文脈.現在.言語体系)
        targets={cid:状態.関係構造 or 問い候補関係形成(文脈.現在.表層,状態.表層,文脈.現在.言語体系) for cid,状態 in 候補群}
        matched={cid:証拠状態照合(targets[cid],relations) for cid,状態 in 候補群}
        reverse="選択意図=反転" in 文脈.条件
        complete={cid for cid,st in matched.items() if not st.矛盾 and (bool(st.反証) if reverse else st.完全支持)}
        if complete:
            return {cid:関係寄与(name,1,("一般関係再結合:全体成立",)) for cid in sorted(complete)}
        if any(st.矛盾 for st in matched.values()):
            return {}
        if reverse:
            # 反転選択は世界関係の「無観測=反証」へ変換しない。
            # ただし他候補がすべて明示支持され、未支持が一候補だけなら、
            # 候補集合上の相対例外差としてその一候補を選べる。
            supported={cid for cid,st in matched.items() if st.完全支持}
            残差=[cid for cid,st in matched.items()
                      if not (st.支持 or st.反証 or st.矛盾)]
            if len(残差)==1 and len(supported)==len(候補群)-1:
                cid=残差[0]
                return {cid:関係寄与(name,1,("選択意図反転:他候補全体支持による相対例外差",))}
            return {}
        blocked={cid for cid,st in matched.items() if st.反証}
        for cid,状態 in 候補群:
            bound=問い候補関係形成(文脈.現在.表層,状態.表層,文脈.現在.言語体系)
            closed_bound=any(t.種別!="開放述語" for t in bound)
            related=False
            for target in targets[cid]:
                for observed in relations:
                    same_comparison_domain=target.種別 in _比較成立域 and observed.種別 in _比較成立域
                    if not (_述語対応(target,observed) or same_comparison_domain):
                        continue
                    if any(_端点意味同一(a,b) for a in (target.始点,target.終点)
                           for b in (observed.始点,observed.終点)):
                        related=True;break
                if related:break
            if closed_bound or related:
                blocked.add(cid)
        if not 文脈.参照状態:
            return {}
        usable_refs=tuple(ref for ref in 文脈.参照状態 if ref.証拠利用可 and ref.表層再解析可)
        sigs={cid:_順序識別特徴(状態.意味語集合,状態.意味語列) for cid,状態 in 候補群}
        common=frozenset.intersection(*sigs.values())
        distinct={cid:values-common for cid,values in sigs.items()}
        union=frozenset.union(*sigs.values())
        question_features=_順序識別特徴(文脈.現在.意味語集合,文脈.現在.意味語列)
        # 問い自身に意味anchorが無い場合、候補共通語を問い由来anchorへ偽装しない。
        anchors=((question_features-union)|common) if question_features else frozenset()
        if not anchors and question_features:
            anchors=文脈.現在.意味語集合
        frequency=Counter(字句 for ref in usable_refs for 字句 in _順序識別特徴(ref.意味語集合,ref.意味語列))
        n=len(usable_refs)
        def weight(字句):
            return log1p(n/(1+frequency[字句]))
        def mass(字句):
            return sum(weight(t) for t in sorted(字句))
        anchor_mass=mass(anchors)
        選択肢_mass={cid:mass(字句) for cid,字句 in distinct.items()}
        score={cid:0.0 for cid in sigs}
        証拠={cid:[] for cid in sigs}
        reverse=any(str(x).casefold()=="選択意図=反転" for x in 文脈.条件)
        for ref in 文脈.参照状態:
            if not (ref.証拠利用可 and ref.表層再解析可):
                continue
            sentences=[x.strip() for x in re.split(r"(?<![0-9])[.!?。！？]\s+|[\r\n]+",ref.表層) if x.strip()]
            windows=list(sentences)
            sentence_字句=[意味語(x) for x in sentences]
            for i in range(len(sentences)-1):
                if sentence_字句[i]&sentence_字句[i+1]:
                    windows.append(" ".join(sentences[i:i+2]))
            if not windows:
                windows=[ref.表層]
            local={cid:0.0 for cid in sigs}
            for text in windows:
                字句=_順序識別特徴(意味語(text),意味列(text))
                # 問い側に意味anchorが無い制御的入力では、候補差とDataの局所対応だけを使う。
                # これはData内容の差を保持するための縮退であり、候補IDや正解情報は使わない。
                anchor=mass(anchors&字句)/anchor_mass if anchor_mass else 1.0
                if anchor<=0:
                    continue
                for cid in sigs:
                    if cid in blocked or 選択肢_mass[cid]<=0:
                        continue
                    overlap=mass(distinct[cid]&字句)/選択肢_mass[cid]
                    value=sqrt(anchor*overlap)
                    local[cid]=max(local[cid],value)
            if anchor_mass <= 0:
                # 問いanchor無しの縮退では、各Data内で候補が一意に識別できた時だけ1観測とする。
                # 同一Dataは文脈化時に重複除去されるため、異なるDataだけが別観測として残る。
                maximum=max(local.values(),default=0.0)
                top=[cid for cid,value in local.items() if value==maximum and value>0]
                if maximum>0 and len(top)==1:
                    cid=top[0]
                    score[cid]+=1.0
                    証拠[cid].append(f"局所対応:{ref.識別子 or 'anonymous'}:1")
                continue
            shared=min(local.values())
            for cid,value in local.items():
                delta=value-shared
                if delta>0:
                    # 意味anchor有りでは、多数の弱い資料が強い局所支持を票数で押し流すのを防ぐ。
                    if delta>score[cid]:
                        score[cid]=delta
                        証拠[cid]=[f"最大局所対応:{ref.識別子 or 'anonymous'}:{delta:.9f}"]
                    elif delta==score[cid]:
                        証拠[cid].append(f"最大局所対応:{ref.識別子 or 'anonymous'}:{delta:.9f}")
        if reverse:
            # 無観測を反証へ読み替えない。例外選択は明示関係寄与に委ねる。
            return {}
        return {cid:関係寄与(name,round(1000000*value),tuple(証拠[cid]))
                for cid,value in score.items() if round(1000000*value)>0}
    def 評価群(self,文脈,候補群): return self._evaluate(文脈,候補群,name=self.名称)
    def 再評価群(self,文脈,候補群,round_index:int): return self._evaluate(文脈,候補群,name=f"候補共同再照合:{round_index}")

def _不成立入力の留保結果(文脈,internal):
    '基底模型核と状態差循環模型核で、同じ入力境界を共有する。'
    if 文脈.現在.証拠利用可 and all(状態.証拠利用可 for _,状態 in internal):
        return None
    ids=tuple(cid for cid,_ in internal)
    differences=tuple(成立差(cid,0,(関係寄与("入力境界未成立",0,('INCOMPLETE_INPUT_状態',)),)) for cid in ids)
    return 模型結果(文脈,differences,None,ids,(),模型統計(),None,ids)


def _コア寄与同一性(item):
    name = "候補共同参照" if item.関係名.startswith("候補共同再照合:") else item.関係名
    return (name, item.差, tuple(sorted(set(item.根拠))))

@dataclass
class _作業状態:
    contributions:dict[str,list[関係寄与]]
    checkpoints:list[模型検査点]
    seen_active:set[tuple[str,...]]
    created:int=0; reused:int=0; reactivated:int=0; global_reconcile:int=0; cross_updates:int=0; rounds:int=0
    def add(self,cid,item):
        key=_コア寄与同一性(item)
        if any(_コア寄与同一性(x)==key for x in self.contributions[cid]):
            return False
        self.contributions[cid].append(item);self.created+=1;return True
    def scores(self):return {cid:sum(x.差 for x in rows) for cid,rows in self.contributions.items()}
    def 検査点(self,stage,active=(),reuse=()):
        scores=self.scores(); self.checkpoints.append(模型検査点(stage,tuple(sorted(scores.items())),tuple(active),tuple(reuse)))

class MINIDORA模型核:
    def __init__(self,関係群:Sequence[模型関係]=(),*,言語対応_:言語対応|None=None,能力作用群:Sequence[object]=(),形成済み関係群:Sequence[模型関係]=(),最大再作用回数:int=2):
        self.言語対応=言語対応_ or 言語対応();self._関係群=list(関係群);self._能力作用群=list(能力作用群);self._形成済み関係群=list(形成済み関係群);self.最大再作用回数=max(0,int(最大再作用回数))
    @property
    def 関係群(self):return tuple(self._関係群)
    @property
    def 能力作用群(self):return tuple(self._能力作用群)
    @property
    def 形成済み関係群(self):return tuple(self._形成済み関係群)
    def 関係登録(self,関係):
        if not getattr(関係,"名称",""):raise ValueError("模型関係には名称が必要")
        self._関係群.append(関係)
    def 形成済み関係登録(self,関係):
        if not getattr(関係,"名称",""):raise ValueError("形成済み関係には名称が必要")
        self._形成済み関係群.append(関係)
    def 文脈化(self,現在,履歴=(),条件=(),参照状態=()):return self.言語対応.文脈化(現在,履歴,条件,参照状態)

    def 評価(self,文脈:文脈付き言語状態,候補群:Sequence[成立候補])->模型結果:
        if not 候補群:raise ValueError("成立差の評価には1候補以上が必要")
        ids=[x.候補ID for x in 候補群]
        if len(ids)!=len(set(ids)):raise ValueError("候補IDは評価内で一意である必要がある")
        internal=[]
        for c in 候補群:
            if c.状態.言語体系!=文脈.現在.言語体系:raise ValueError("候補と言語文脈の言語体系が一致しない")
            internal.append((c.候補ID,self.言語対応.内部化(c.状態)))
        # 未成立の問い/候補を取り除いて、一意候補を人工的に作ってはいけない。
        # Data本文の存在と、Compilerが後続利用を許した入力であることを区別する。
        incomplete=_不成立入力の留保結果(文脈,tuple(internal))
        if incomplete is not None:
            return incomplete
        work=_作業状態({cid:[] for cid in ids},[],set())

        # 1) 一般作用。ここでは確定せず寄与状態だけ作る。
        for cid,状態 in internal:
            for rel in self._関係群:
                item=rel.評価(文脈,状態)
                if item:work.add(cid,item)
        work.検査点("標準関係",ids)

        # 2) 形成済み関係は作用機構と分離した別段で再利用する。
        for cid,状態 in internal:
            for rel in self._形成済み関係群:
                item=rel.評価(文脈,状態)
                if item:work.add(cid,item)
        work.検査点("形成済み関係",ids)

        # 3) 参照/共同作用の初回作用。
        for 作用 in self._能力作用群:
            if hasattr(作用,"評価群"):
                結果=作用.評価群(文脈,tuple(internal))
                for cid,item in 結果.items():work.add(cid,item)
            else:
                for cid,状態 in internal:
                    item=作用.評価(文脈,状態)
                    if item:work.add(cid,item)
        work.検査点("一次能力作用",ids)

        # 4) 候補共同状態を変えながら、過去状態へ再作用する。active集合が同じなら重複反復しない。
        for round_index in range(1,self.最大再作用回数+1):
            scores=work.scores()
            ordered=sorted(ids,key=lambda cid:(-scores[cid],cid))
            active=tuple(ordered[:min(2,len(ordered))])
            if active in work.seen_active:break
            work.seen_active.add(active);work.reactivated+=1;work.global_reconcile+=1;work.rounds+=1
            active_rows=tuple(row for row in internal if row[0] in active)
            changed=0
            for 作用 in self._能力作用群:
                if not hasattr(作用,"再評価群"):continue
                結果=作用.再評価群(文脈,tuple(internal) if isinstance(作用,候補共同参照作用) else active_rows,round_index)
                for cid,item in 結果.items():
                    if work.add(cid,item):changed+=1;work.reused+=1
            work.cross_updates+=changed
            work.検査点(f"RECONCILE_{round_index}",active,reuse=("一次能力作用",))
            if not changed:break

        differences=tuple(成立差(cid,sum(x.差 for x in work.contributions[cid]),tuple(work.contributions[cid])) for cid in ids)
        maximum=max(x.差 for x in differences);top=tuple(x.候補ID for x in differences if x.差==maximum)
        # 正の成立差が一意に形成された場合だけ一般模型結果を確定する。
        winner=top[0] if maximum>0 and len(top)==1 else None

        ref_scores={row.候補ID:sum(c.差 for c in row.寄与 if c.関係名.startswith(("参照関係寄与","候補共同参照","候補共同再照合"))) for row in differences}
        ref_max=max(ref_scores.values(),default=0);ref_top=tuple(cid for cid,v in ref_scores.items() if v==ref_max)
        ref_winner=ref_top[0] if ref_max>0 and len(ref_top)==1 else None
        stats=模型統計(work.created,work.reused,work.reactivated,work.global_reconcile,work.cross_updates,work.rounds,len(work.checkpoints))
        return 模型結果(文脈,differences,winner,top if len(top)>1 else (),tuple(work.checkpoints),stats,ref_winner,ref_top if len(ref_top)>1 else ())

    def 評価言語状態(self,現在,候補群,*,履歴=(),条件=(),参照状態=()):
        return self.評価(self.文脈化(現在,履歴,条件,参照状態),候補群)

def 標準模型核()->MINIDORA模型核:
    return MINIDORA模型核((意味連続関係(),順序連続関係(),有向関係整合(),肯否整合関係(),履歴近接関係(),条件結合関係()),能力作用群=(参照関係寄与作用(),候補共同参照作用()))

__all__=["LLM成立規定リポジトリ","LLM成立規定参照コミット","LLM成立規定版","LLM成立意味区別","LLM構成再現区別","言語状態","内部言語状態","文脈付き言語状態","成立候補","関係寄与","成立差",'模型検査点',"模型統計","模型結果","言語対応","模型関係","関係規則","意味連続関係","順序連続関係","有向関係整合","肯否整合関係","履歴近接関係","条件結合関係","参照関係寄与作用","候補共同参照作用","MINIDORA模型核","標準模型核"]
