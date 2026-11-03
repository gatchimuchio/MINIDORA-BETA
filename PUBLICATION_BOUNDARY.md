# MINIDORA — Public Release Boundary

## Purpose

This public repository exposes executable implementations, tests, external evaluation evidence and local public interfaces. It does **not** disclose private internal theory, complete abstract definitions, exhaustive conceptual decompositions or a complete private-theory-to-source mapping.

## Permitted public material

- Executable code and its public-facing API/input/output contracts
- Local algorithms and failure conditions
- Tests and observed evaluation results, with scope
- Compatibility and replaceable module interfaces
- Public-data provenance needed for reproducibility
- English explanations of observed public behavior

## Excluded private material

- Direct links, fixed reference commits, or inventories pointing to unpublished internal theory
- Complete private abstract definitions or summaries allowing their unique reconstruction
- Complete crosswalks between private concepts and source files, types or development stages
- Large transformed teaching/semantic datasets capable of revealing unpublished meaning rules
- Historical explanations sufficient to reconstruct private definition changes
- Complete minimal decompositions of the unpublished internal theory

## Documentation rule

Explain **inputs, outputs, guarantees, non-guarantees, public behavior and observable failure modes**. Do not present internal theoretical explanations, complete private concept structures, or upper-level source references. Historical names in implementation filenames or compatible APIs are **identifiers**, not authorization to reconstruct private internal theory.

English translations must respect the same boundary as Japanese source documents.

## Distribution rule

Do not publish large derived semantic-output collections, large paired input/output ZIP archives, or other derivatives that could reconstruct unpublished internal meaning coordinates. Audit release assets separately from the source Git tree **before** creating a public Release.

## Git history

Removing a file from the default branch does not remove it from prior Git commits, tags, releases, forks, mirrors or downloaded archives. Where genuine historical removal is required, ordinary file deletion is insufficient; history rewrite, release-asset removal and mirror review are distinct operations requiring authorization.

## Automated audit

The original repository documents `python tools/公開境界監査.py` as a CI audit for reintroduced forbidden references and archives. **Do not assume this script is included in or automatically executed by the isolated beta checkout.** Check availability and execution evidence before claiming the audit passed.
