from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable


@dataclass(frozen=True, slots=True)
class 構文化還元対応:
    模型: str
    公開構造: str
    作用解釈: str
    MINIDORA還元: str
    実装状態: str
    観測深度: str
    備考: str = ""


_K3 = (
    構文化還元対応(
        "K3",
        "KDA recurrent state update",
        "履歴差を圧縮状態へ選択的に継続反映する",
        "K3機能.MemorySystem.selective_update / HDS作業状態",
        "既存",
        "D4",
    ),
    構文化還元対応(
        "K3",
        "3 KDA + 1 Gated MLA",
        "局所更新と大域再照合を別作用として交替する",
        "hds局所再照合 / hds能力経路_v2 / HDS大域再照合判断",
        "既存+GLMで一般化",
        "D4",
    ),
    構文化還元対応(
        "K3",
        "AttnRes depth checkpoints",
        '過去処理検査点を保持し後段から再選択する',
        '能力状態差循環 / HDS作業検査点',
        "既存",
        "D4",
    ),
    構文化還元対応(
        "K3",
        "shared experts + routed experts",
        "共通作用を残しつつ複数専門作用へ分岐する",
        "能力作用群 / 専門作用routing",
        "既存",
        "D4",
    ),
    構文化還元対応(
        "K3",
        "MOPD effort policy",
        "同一処理核で計算予算を切り替える",
        'HDS探索方針 / Distilled計算量PolicyController',
        "既存",
        "構造類似",
    ),
    構文化還元対応(
        "K3",
        "GRM / rubric verification",
        "候補生成と最終採否権限を分離する",
        "HDS判断主体 / J_hds",
        "既存",
        "構造類似",
    ),
)


_GLM = (
    構文化還元対応(
        "GLM-5.3-Flash",
        "KDA local state layers",
        "安価な局所状態更新を連続実行する",
        "HDS多時間尺度政策.局所更新回数",
        "追加",
        "D4+公開config",
    ),
    構文化還元対応(
        "GLM-5.3-Flash",
        "3 KDA : 1 DSA hybrid cycle",
        "局所更新と高価な大域検索を異なる時間尺度で実行する",
        "HDS多時間尺度政策 / HDS大域再照合判断",
        "追加",
        "D4+公開config",
    ),
    構文化還元対応(
        "GLM-5.2/5.3",
        "DSA top-k sparse retrieval",
        "粗い索引で候補参照を選び、正本証拠を正確に再読する",
        "hds参照計画.HDS参照索引 / HDS参照計画適用",
        "追加",
        "D4+公開実装",
    ),
    構文化還元対応(
        "GLM-5.3-Flash",
        "IndexPool 4-to-1",
        "正本証拠とは別に検索索引だけをbucket圧縮する",
        "HDS参照索引圧縮(bucket幅=4)",
        "追加",
        "D4+公開config",
        "圧縮索引を証拠として使用しない",
    ),
    構文化還元対応(
        "GLM-5.2/5.3",
        "IndexShare",
        "一度決めた参照選択を有限期間だけ再利用する",
        "HDS参照計画.利用上限/残存利用回数",
        "追加",
        "公開実装",
        "MINIDORAでは参照計画leaseとして射影",
    ),
    構文化還元対応(
        "GLM-5.3-Flash",
        "mHC hc_mult=4 + Sinkhorn constrained mixing",
        "複数状態laneを並行保持し制約付きで読取・書戻し混合する",
        "hds並列作業状態.HDS並列作業状態 / HDS制約混合行列",
        "追加",
        "D4+公開config",
        "laneの意味役割は固定しない",
    ),
    構文化還元対応(
        "GLM-5.x",
        "first dense layers before sparse MoE",
        "専門分岐前に全入力共通の処理を通す",
        "既存の共通作用→専門作用境界",
        "K3既存を再利用",
        "公開config",
    ),
    構文化還元対応(
        "GLM-5.x",
        "shared expert + top-k routed experts",
        "共通作用と専門作用を同時保持する",
        "既存の能力作用群 / 専門作用routing",
        "K3既存を再利用",
        "D4+公開config",
    ),
    構文化還元対応(
        "GLM-5.2/5.3",
        "reasoning effort levels",
        "要求に応じて探索・計算予算を変える",
        "HDS探索方針",
        "K3既存を再利用",
        "公開運用",
    ),
    構文化還元対応(
        "GLM-5.x",
        "MTP / speculative next-token path",
        "先行草案を作りprefix検証し不成立位置からrollbackする",
        "HDS先行草案検証",
        "追加・任意",
        "公開実装",
        "知能作用ではなく生成効率補助として扱う",
    ),
    構文化還元対応(
        "GLM-5.3-Flash",
        "native multimodal input path",
        "modality固有表象を中央処理の共通表象へ接続する",
        "HDS異種入力射影 / HDS共通入力表象",
        "追加・境界",
        "公開config",
        'parser自体は外部適合器責任',
    ),
    構文化還元対応(
        "GLM-5.2/5.3 post-training",
        "long-horizon environment feedback",
        "失敗理由に応じて次作用を変え、証拠状態変化時は参照計画を再構築する",
        "HDS阻害回復方針",
        "追加",
        "公開post-training記述からの作用射影",
        "実行系内部命令は推定でありGLMのliteral componentではない",
    ),
    構文化還元対応(
        "GLM-5.2→5.3",
        "same pretrained base, different post-training result",
        "静的topologyと運用政策を別責任として保持する",
        "HDS多時間尺度政策を状態/索引機構から分離",
        "設計境界へ還元",
        "公開モデル情報",
        "最終weightが同一という意味ではない",
    ),
)


構文化還元一覧: tuple[構文化還元対応, ...] = _K3 + _GLM


def 構文化還元検索(*, 模型: str | None = None, MINIDORA還元: str | None = None) -> tuple[構文化還元対応, ...]:
    rows: Iterable[構文化還元対応] = 構文化還元一覧
    if 模型 is not None:
        needle = str(模型).casefold()
        rows = tuple(row for row in rows if needle in row.模型.casefold())
    if MINIDORA還元 is not None:
        needle = str(MINIDORA還元).casefold()
        rows = tuple(row for row in rows if needle in row.MINIDORA還元.casefold())
    return tuple(rows)


__all__ = ["構文化還元対応", "構文化還元一覧", "構文化還元検索"]
