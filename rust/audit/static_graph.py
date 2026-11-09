#!/usr/bin/env python3
"""Index lexical imports/call syntax without claiming runtime reachability."""
from __future__ import annotations
import argparse
import ast
import csv
import json
from pathlib import Path
from collections import Counter

ROOT=Path(__file__).resolve().parents[2]
OUT=Path(__file__).resolve().parent


def tsv(path):
    return list(csv.DictReader(path.open(encoding='utf-8'), delimiter='\t'))


def write(name, rows):
    with (OUT/name).open('w',encoding='utf-8',newline='') as f:
        w=csv.DictWriter(f,fieldnames=list(rows[0]),delimiter='\t',lineterminator='\n')
        w.writeheader();w.writerows(rows)


def main(upstream):
    files={row['beta_path']:ROOT/row['beta_path'] for row in tsv(OUT/'source_identity.tsv')}
    for row in tsv(OUT/'test_identity.tsv'):
        if row['beta_test_path']!='NOT_INCLUDED':
            files[row['beta_test_path']]=ROOT/row['beta_test_path']
        elif upstream:
            files[row['origin_test_path']]=upstream/row['origin_test_path']
    imports=[]; definitions=[]; result=Counter()
    for path,p in sorted(files.items()):
        if p.suffix!='.py':result['non_python']+=1;continue
        if not p.is_file():result['missing']+=1;continue
        try:tree=ast.parse(p.read_text(encoding='utf-8-sig'))
        except (SyntaxError,UnicodeError):result['parse_failed']+=1;continue
        result['parsed']+=1
        for node in ast.walk(tree):
            if isinstance(node,ast.Import):
                for a in node.names:
                    imports.append({'path':path,'line':node.lineno,'module':a.name,'imported_name':'MODULE','alias':a.asname or '', 'kind':'STATIC_IMPORT_SYNTAX','runtime_reachability':'UNVERIFIED'})
            elif isinstance(node,ast.ImportFrom):
                for a in node.names:
                    imports.append({'path':path,'line':node.lineno,'module':'.'*node.level+(node.module or ''),'imported_name':a.name,'alias':a.asname or '', 'kind':'STATIC_IMPORT_FROM_SYNTAX','runtime_reachability':'UNVERIFIED'})
            elif isinstance(node,ast.Call) and isinstance(node.func,ast.Name) and node.func.id in ('_読込','import_module') and node.args and isinstance(node.args[0],ast.Constant) and isinstance(node.args[0].value,str):
                imports.append({'path':path,'line':node.lineno,'module':node.args[0].value,'imported_name':'DYNAMIC_LITERAL','alias':'','kind':'DYNAMIC_IMPORT_LITERAL_SYNTAX','runtime_reachability':'UNVERIFIED'})
        def walk(items,prefix=''):
            for node in items:
                if isinstance(node,(ast.ClassDef,ast.FunctionDef,ast.AsyncFunctionDef)):
                    name=prefix+node.name
                    calls=sorted({ast.unparse(n.func) for n in ast.walk(node) if isinstance(n,ast.Call)})
                    definitions.append({'path':path,'symbol':name,'kind':type(node).__name__,'start_line':node.lineno,'end_line':node.end_lineno,'lexical_call_syntax':json.dumps(calls,ensure_ascii=False,separators=(',',':')),'assert_nodes':sum(isinstance(n,ast.Assert) for n in ast.walk(node)),'raise_nodes':sum(isinstance(n,ast.Raise) for n in ast.walk(node)),'semantic_review':'NOT_ESTABLISHED_BY_STATIC_AST','runtime_reachability':'UNVERIFIED'})
                    walk(node.body,name+'.')
        walk(tree.body)
    write('static_imports.tsv',imports)
    write('static_callables.tsv',definitions)
    summary={'schema':'MINIDORA_PUBLIC_STATIC_AST_INDEX_V1','file_counts':dict(result),'import_syntax_rows':len(imports),'callable_rows':len(definitions),'warning':'Lexical references may be conditional, shadowed, dynamically replaced, or unreachable. This index is not a call graph proof, semantic audit or acceptance result. It contains no evaluated input/output dataset.'}
    (OUT/'static_summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(summary,ensure_ascii=False,indent=2))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--upstream',type=Path);main(p.parse_args().upstream)
