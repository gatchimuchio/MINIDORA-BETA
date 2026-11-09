#!/usr/bin/env python3
"""Reproduce public-file identity and availability; never infer capability acceptance."""
from __future__ import annotations

import argparse
import ast
import csv
import hashlib
import json
import subprocess
from collections import Counter
from pathlib import Path

BASELINE = '1c546e77007afa0a9eeb3a44f07da99d84c383eb'
UPSTREAM = '9307d7e2b1799750ecef1957d85128fcbe403d02'
ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding='utf-8', newline='') as f:
        return list(csv.DictReader(f, delimiter='\t'))


def write_tsv(name: str, rows: list[dict[str, object]]) -> None:
    with (HERE / name).open('w', encoding='utf-8', newline='') as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0]), delimiter='\t', lineterminator='\n')
        writer.writeheader()
        writer.writerows(rows)


def identity(root: Path | None, path: str, expected: str) -> dict[str, str | int | None]:
    if path in ('NOT_UPSTREAM', 'NOT_INCLUDED'):
        return {'availability': 'NOT_APPLICABLE', 'git_blob': None, 'sha256': None, 'bytes': None,
                'identity_status': 'NOT_APPLICABLE'}
    p = root / path if root is not None else None
    if p is None or not p.is_file():
        return {'availability': 'MISSING', 'git_blob': None, 'sha256': None, 'bytes': None,
                'identity_status': 'UNVERIFIED'}
    data = p.read_bytes()
    blob = hashlib.sha1(f'blob {len(data)}\0'.encode() + data).hexdigest()
    return {'availability': 'PRESENT', 'git_blob': blob, 'sha256': hashlib.sha256(data).hexdigest(),
            'bytes': len(data), 'identity_status': 'MATCH' if blob == expected else 'MISMATCH'}


def symbols(path: Path) -> str:
    if path.suffix != '.py' or not path.is_file():
        return ''
    try:
        module = ast.parse(path.read_text(encoding='utf-8-sig'))
    except (SyntaxError, UnicodeError):
        return 'PARSE_FAILED'
    names = [n.name for n in module.body if isinstance(n, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef))]
    return json.dumps(names, ensure_ascii=False, separators=(',', ':'))


def generate(upstream: Path | None) -> None:
    if upstream is not None and git(upstream, 'rev-parse', 'HEAD') != UPSTREAM:
        raise SystemExit('Upstream checkout must be pinned to the exact extraction commit')
    source_rows = []
    for row in read_tsv(ROOT / 'docs/redesign/04_SOURCE_CENSUS.tsv'):
        result: dict[str, object] = dict(row)
        result['source_commit'] = BASELINE
        result['upstream_commit'] = UPSTREAM
        for label, repo, path_key, blob_key in [
            ('beta', ROOT, 'beta_path', 'beta_blob_sha'),
            ('upstream', upstream, 'origin_path', 'origin_blob_sha'),
        ]:
            result.update({label + '_' + k: v for k, v in identity(repo, row[path_key], row[blob_key]).items()})
        result['declared_top_level_symbols'] = symbols(ROOT / row['beta_path'])
        result['source_meaning'] = 'NOT_REVIEWED_BY_IDENTITY_INVENTORY'
        result['actual_entry_route'] = 'UNMAPPED'
        result['rust_path'] = 'UNMAPPED'
        result['acceptance_status'] = 'UNVERIFIED'
        source_rows.append(result)
    write_tsv('source_identity.tsv', source_rows)

    test_rows = []
    for row in read_tsv(ROOT / 'docs/redesign/09_TEST_CENSUS.tsv'):
        result = dict(row)
        result['source_commit'] = BASELINE
        result['upstream_commit'] = UPSTREAM
        for label, repo, path_key, blob_key in [
            ('beta', ROOT, 'beta_test_path', 'beta_blob_sha'),
            ('upstream', upstream, 'origin_test_path', 'origin_blob_sha'),
        ]:
            result.update({label + '_' + k: v for k, v in identity(repo, row[path_key], row[blob_key]).items()})
        result['rust_applicability'] = 'UNREVIEWED'
        result['rust_equivalent_test'] = 'UNMAPPED'
        result['execution_result'] = 'NOT_RUN_BY_IDENTITY_INVENTORY'
        test_rows.append(result)
    write_tsv('test_identity.tsv', test_rows)

    requirement_rows = read_tsv(ROOT / 'docs/redesign/03_REQUIREMENT_TRACE.tsv')
    for row in requirement_rows:
        row['rust_implementation_path'] = 'UNMAPPED'
        row['rust_observation_ref'] = 'UNOBSERVED'
        row['inventory_scope'] = 'BASELINE_REQUIREMENTS_ONLY'
    write_tsv('requirements.tsv', requirement_rows)

    references = [ROOT / 'AGENTS.md', ROOT / 'PUBLICATION_BOUNDARY.md']
    references += sorted((ROOT / 'docs/redesign').glob('*'))
    references += [ROOT / p for p in (
        'SOURCE_LOCK.md', 'REPOSITORY_LOCK.md', 'BETA_READINESS.md',
        'CURRENT_CANONICAL.md', 'RELEASE_MANIFEST.json', 'DOCS_REVISION.md')]
    if upstream is not None:
        references += [upstream / p for p in (
            'CURRENT_CANONICAL.md', '正本系統.md', '現行正本.md', '開発正本履歴.md',
            '評価/評価契約_v3.md', '評価/GPQA評価二系統契約_v1.json')]
    docs = []
    for p in references:
        if not p.is_file():
            continue
        base = ROOT if p.is_relative_to(ROOT) else upstream
        assert base is not None
        data = p.read_bytes()
        docs.append({'repository': 'beta' if base == ROOT else 'upstream',
                     'source_commit': BASELINE if base == ROOT else UPSTREAM,
                     'path': str(p.relative_to(base)), 'sha256': hashlib.sha256(data).hexdigest(),
                     'bytes': len(data), 'semantic_review': 'NOT_ESTABLISHED_BY_HASH'})
    write_tsv('reference_documents.tsv', docs)

    def counts(rows: list[dict[str, object]], key: str) -> dict[str, int]:
        return dict(sorted(Counter(str(r[key]) for r in rows).items()))

    summary = {
        'schema': 'MINIDORA_RUST_PUBLIC_IDENTITY_AUDIT_V1',
        'observed_date_jst': '2026-11-09',
        'baseline_commit': BASELINE,
        'observed_beta_head': git(ROOT, 'rev-parse', 'HEAD'),
        'upstream_commit': UPSTREAM,
        'upstream_available': upstream is not None,
        'source_population': len(source_rows),
        'upstream_source_population': sum(r['origin_path'] != 'NOT_UPSTREAM' for r in source_rows),
        'beta_source_identity': counts(source_rows, 'beta_identity_status'),
        'upstream_source_identity': counts(source_rows, 'upstream_identity_status'),
        'test_population_union': len(test_rows),
        'upstream_test_population': sum(r['origin_test_path'] != 'NOT_UPSTREAM' for r in test_rows),
        'beta_test_identity': counts(test_rows, 'beta_identity_status'),
        'upstream_test_identity': counts(test_rows, 'upstream_identity_status'),
        'test_presence_in_baseline_census': counts(test_rows, 'path_presence'),
        'requirements': len(requirement_rows),
        'requirement_ids_unique': len({r['capability_id'] for r in requirement_rows}) == len(requirement_rows),
        'requirement_baseline_acceptance': counts(requirement_rows, 'acceptance_status'),
        'test_results': 'NOT_RUN_BY_IDENTITY_INVENTORY',
        'semantic_route_review': 'NOT_ESTABLISHED_BY_FILE_PRESENCE_AST_OR_HASH',
        'complete_rust_acceptance': False,
    }
    (HERE / 'population.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    if any(r['beta_identity_status'] == 'MISMATCH' or r['upstream_identity_status'] == 'MISMATCH'
           for r in source_rows + test_rows):
        raise SystemExit('Identity mismatch: inspect audit output before accepting baseline provenance')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--upstream', type=Path)
    generate(parser.parse_args().upstream)
