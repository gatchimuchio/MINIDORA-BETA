//! Versioned description of the shared Native and CLI execution contract.

use crate::{Engine, search::SearchProvider};
use minidora_domain::{Lifecycle, ResourceBudget, Result, SCHEMA_VERSION};
use serde::Serialize;

pub const NATIVE_API_VERSION: &str = "minidora-native/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityPermission {
    ExactHttpsHost { host: &'static str },
    ExactLocalWritePath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct EngineCapability {
    pub id: &'static str,
    pub scope: &'static str,
    pub permission: Option<CapabilityPermission>,
    pub limits: &'static str,
}

/// Implemented routes and their boundaries, independent of a caller's grants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EngineCapabilities {
    pub package: &'static str,
    pub version: &'static str,
    pub api_version: &'static str,
    pub schema_version: &'static str,
    pub target_os: &'static str,
    pub target_arch: &'static str,
    pub default_budget: ResourceBudget,
    pub capabilities: Vec<EngineCapability>,
    pub known_limits: &'static [&'static str],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EngineCaseStatus {
    pub engine: EngineCapabilities,
    pub case_id: String,
    pub case_revision: Option<u64>,
    /// State of the saved case revision, even before a result is available.
    pub case_lifecycle: Option<Lifecycle>,
    pub result: Option<EngineResultStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EngineResultStatus {
    pub id: String,
    pub lifecycle: Lifecycle,
    pub reason: Option<String>,
    /// Identity observed in this result's adopted plan, when present.
    pub model_revision: Option<String>,
    pub model_artifact_ref: Option<String>,
}

impl Engine {
    /// Describe the engine without opening a store or issuing an effect.
    pub fn capabilities() -> EngineCapabilities {
        EngineCapabilities {
            package: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
            api_version: NATIVE_API_VERSION,
            schema_version: SCHEMA_VERSION,
            target_os: std::env::consts::OS,
            target_arch: std::env::consts::ARCH,
            default_budget: ResourceBudget::development(),
            capabilities: vec![
                EngineCapability {
                    id: "japanese-dialogue",
                    scope: "日本語の挨拶、ミニドラ自身についての質問、関係の質問、比較、説明、追問、訂正を同じCoreで処理します。",
                    permission: None,
                    limits: "入力形式は実装済みの文型に限られ、不明な依頼は保留します。",
                },
                EngineCapability {
                    id: "relation-memory",
                    scope: "利用者ごとの前提・規則・条件を保持し、別ターンや再起動後の推論と説明に使います。",
                    permission: None,
                    limits: "根拠、適用範囲、時点、否定を区別します。利用者の前提は外部で確認された事実にはなりません。",
                },
                EngineCapability {
                    id: "exact-mathematics",
                    scope: "有理数、対応単位、計算手順、統計、確率、有限の代数演算を処理します。",
                    permission: None,
                    limits: "演算ごとの文型、次元、項数と実行予算に従います。",
                },
                EngineCapability {
                    id: "structured-documents",
                    scope: "CSV・JSON・表の集計、比較と変換、本文の編集、貼り付けた文章の抽出要約を処理します。",
                    permission: None,
                    limits: "表と文書は対応形式に限られます。抽出要約の意味範囲や網羅性は保証しません。",
                },
                EngineCapability {
                    id: "bounded-code",
                    scope: "構造化仕様や文中の戻り式からPython関数を生成し、貼り付けた関数の説明、対応文法の読取と有限評価を行います。",
                    permission: None,
                    limits: "生成は明示されたアルゴリズムに、評価は単一関数の対応文法に限られます。ホストのPythonは実行しません。",
                },
                EngineCapability {
                    id: "web-grounded-answer",
                    scope: "対応する日本語の依頼から候補を検索し、取得した本文の記述を出典・時点・ハッシュ付きで回答します。明示された標高の換算と、保存済み本文からの根拠付き抜粋も扱います。",
                    permission: Some(CapabilityPermission::ExactHttpsHost {
                        host: SearchProvider::Wikipedia.host(),
                    }),
                    limits: "Wikipedia日本語版の対応本文と短い話題に限られます。抜粋は記事冒頭で読み取れた範囲が対象です。本文不足や通信・許可の失敗時は保留します。",
                },
                EngineCapability {
                    id: "local-file-delivery",
                    scope: "元Goalが要求したUTF-8成果を、明示許可されたパスへ保存して読み戻します。取消や不明な受領は同じ作用IDで照合します。",
                    permission: Some(CapabilityPermission::ExactLocalWritePath),
                    limits: "保存先は信頼できるローカル領域に限ります。異なる既存内容を上書きせず、不明な書込結果は確認します。",
                },
            ],
            known_limits: &[
                "外部LLM、任意コード実行、任意のWebページ・PDF・画像の解釈は提供しません。",
                "永続状態には対応するローカルSQLite環境が必要です。旧Rust状態の移行は明示した対応版に限られます。",
                "対象OSとCPUはこのバイナリのビルド対象です。プラットフォームの保証は表しません。",
                "非ニューラル模型の実版と成果物は採用された各結果のContentPlanに記録します。",
            ],
        }
    }

    /// Describe the latest saved state and any result in an actor-owned case.
    pub fn case_status(&self, actor: &str, case_id: &str) -> Result<EngineCaseStatus> {
        let case = self.latest(actor, case_id)?;
        let case_revision = case.as_ref().map(|case| case.revision);
        let case_lifecycle = case.as_ref().map(|case| case.lifecycle);
        let result = case.and_then(|case| case.result).map(|result| {
            let (model_revision, model_artifact_ref) = result
                .content_plan
                .map(|plan| (plan.model_revision, plan.model_artifact_ref))
                .unwrap_or_default();
            EngineResultStatus {
                id: result.id,
                lifecycle: result.lifecycle,
                reason: result.reason,
                model_revision: (!model_revision.is_empty()).then_some(model_revision),
                model_artifact_ref: (!model_artifact_ref.is_empty()).then_some(model_artifact_ref),
            }
        });
        Ok(EngineCaseStatus {
            engine: Self::capabilities(),
            case_id: case_id.into(),
            case_revision,
            case_lifecycle,
            result,
        })
    }
}
