# MINIDORA-BETA GitHub移植固定記録

- 元のソース正本: `gatchimuchio/NOTNN-LLM-MINIDORA`, `9307d7e2b1799750ecef1957d85128fcbe403d02`
- 移植元の配布物: `MINIDORA_Engine_Beta_0.6.0b1_20261103.zip`
- 移植元アーカイブ SHA256: `60e48b2a54c953166b9f8625a08bcd9573b13cee19fd69794e767f0aa921c7db`
- 配布物の範囲: 元公開ソース414ファイル全数 + 変更1ファイル + 新規入口2ファイル + 選定回帰試験 + 受入文書。
- GitHubの`main`にはソース・仕様・試験・保守文書を移植する。wheelバイナリは`dist/`に追跡せず、`python -m pip wheel --no-deps .`で再ビルド可能。
- 元の`RELEASE_MANIFEST.json`と`checksums.sha256`は**配布ZIPの固定記録**であり、`dist/`を除外したGitHubのチェックアウト全体を直接検証するものではない。
- このリポジトリのソース範囲は`checksums.repo.sha256`で検証する。追加のリポ体裁ファイルはGit commitで履歴管理する。
- GitHub側で追加したREADME英語版、変更履歴、変更規律、手動受入ワークフロー等は、β1エンジンの実装内容を変更しない。
- 2026-11-03、Linux/Python3.12で313件の関連試験を通過。全公開テストの合格や性能値の新規測定とは区別する。
