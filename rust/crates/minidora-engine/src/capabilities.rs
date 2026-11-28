//! Versioned description of the shared Native and CLI execution contract.

use crate::{Engine, news_source::NewsPublisher, search::SearchProvider};
use minidora_domain::{Lifecycle, ResourceBudget, Result, SCHEMA_VERSION};
use serde::Serialize;

pub const NATIVE_API_VERSION: &str = "minidora-native/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityPermission {
    ExactHttpsHost { host: &'static str },
    ExactLocalWritePath,
    ExplicitLocalWave,
    LocalWasmExecution,
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
        const METADATA_SEARCH_LIMITS: &str = "候補のメタデータのみを返し、本文取得や事実確認は行いません。許可・予算・通信条件を満たさない場合は保留します。";
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
                    scope: "日常の体験や観察については、短い日本語・英語の発話を同じ会話で扱い、指定した言語で応答します。挨拶、話題の追問、ミニドラ自身についての質問、関係の質問、比較、説明、訂正も同じCoreで処理します。同じ会話で確定した話題は、名前を指定して戻れます。",
                    permission: None,
                    limits: "入力形式は実装済みの文型に限られ、不明な依頼は保留します。",
                },
                EngineCapability {
                    id: "japanese-message-draft",
                    scope: "指定した宛先と内容からメールやメッセージの文案を作ります。英語では依頼や招待への辞退も作成できます。箇条書きでは翻訳や計算とまとめて依頼でき、同じ会話で保存した文案・訳文は、箇条書きの項目番号を指定して変更できます。文案は日英どちらからでも、元の言語を保って文体や文数を変えられます。",
                    permission: None,
                    limits: "対応する目的・条件に限ります。文案を生成し、送信を求める入力は保留します。",
                },
                EngineCapability {
                    id: "japanese-short-fiction",
                    scope: "指定した登場人物、舞台、物語に入れる要素を使い、読者・文体・文数に合わせて短い日本語の創作を生成します。",
                    permission: None,
                    limits: "対応する構成と短い文数に限ります。生成文は創作と明示し、事実の根拠や利用者の記憶として登録しません。",
                },
                EngineCapability {
                    id: "offline-short-translation-ja-en",
                    scope: "対応する短い通知・記述を日英で翻訳します。別の話題を挟んでも、同じ会話の確定訳を日英どちらの依頼からも参照し、原文を保って文体を変更できます。",
                    permission: None,
                    limits: "対応する語彙と文型が対象です。日本語訳は敬体・常体、英訳は対応する助動詞などの短縮形を切り替えます。",
                },
                EngineCapability {
                    id: "option-decision",
                    scope: "数値と有無で示された候補を、必須条件と優先順で比較し、推奨・同順位・判断保留の根拠を返します。同じ会話の比較は、候補情報、必須条件、優先順の指定箇所だけを更新して見直せます。",
                    permission: None,
                    limits: "更新資料は候補名と項目が行で明示されたTXT・Markdown・文字PDFに対応します。値や単位を照合できない場合は保留します。",
                },
                EngineCapability {
                    id: "bounded-work-planning",
                    scope: "目的と工程、それぞれの所要時間・依存先、人数・期限から作業計画を作ります。人数だけ不足して計画が変わる場合は、分かっている条件を示し、同じ会話で人数の補足を受け付けます。同じ会話で確定した計画は、人数を補足して作った場合も、所要時間や期限を変更して見直せます。",
                    permission: None,
                    limits: "対応する個数・人数・入力形式の作業計画を扱います。工程の実行や予定の登録は行いません。",
                },
                EngineCapability {
                    id: "memory-sharing-settings",
                    scope: "記憶の設定を確認し、同じ利用者の別会話で保存した記憶の横断利用をオン・オフにできます。設定は再起動後も続きます。",
                    permission: None,
                    limits: "会話の履歴と保存記憶は残ります。この設定が管理するのは、保存記憶の新しい応答への利用範囲です。",
                },
                EngineCapability {
                    id: "case-branching",
                    scope: "確定済みの回答から独立した会話を作ります。分岐時点の履歴を読み、元の回答の要約や指定語句の置換を行えます。",
                    permission: None,
                    limits: "分岐元と先は同じ利用者です。分岐元の許可や保存作用は引き継ぎません。要約と置換の対象は固定された回答本文です。",
                },
                EngineCapability {
                    id: "purpose-profiles",
                    scope: "登録した用途ごとに指示、資料の保存版、道具の範囲を切り替えます。用途の資料は登録時の内容で要約または語句検索します。",
                    permission: None,
                    limits: "用途は利用者ごとに保存されます。道具の設定で今回の権限は増えません。同名の設定は上書きしません。",
                },
                EngineCapability {
                    id: "relation-memory",
                    scope: "利用者ごとの前提・規則・条件を保持し、別ターンや再起動後の推論と説明に使います。「訂正します。」に続けて新しい前提を書くと、特定できる元の前提を訂正できます。「仮にAはBだとすると、AはCですか」のような質問では、保存した根拠での結論と、その場だけの仮定を加えた結論を比べます。",
                    permission: None,
                    limits: "根拠、適用範囲、時点、否定を区別します。利用者の前提は外部で確認された事実にはなりません。",
                },
                EngineCapability {
                    id: "exact-mathematics",
                    scope: "有理数、対応単位、計算手順、統計、確率、有限の代数演算を処理します。明示された値の記述統計は日英で処理し、箇条書きの依頼では選択した統計量を同じ依頼の後続計算に使えます。有限分布の期待値・分散は日本語で返します。型付き計算記録を追問に使い、保存した計算結果を指定した倍率で計算し直せます。",
                    permission: None,
                    limits: "演算ごとの文型、次元、項数と実行予算に従います。確定値の長さ換算で換算先が不足すると、同じ会話で単位を補足できます。前の結果への倍率指定は、単位のない数値と mm・cm・m・km の長さに対応します。",
                },
                EngineCapability {
                    id: "structured-documents",
                    scope: "表の集計・比較・変換、CSVのSVG、TXT・Markdown・文字PDFの出典付き抜粋と検索、資料の項目で絞り込むCSVとExcelの合計を処理します。",
                    permission: None,
                    limits: "表と文書は対応形式に限られます。絞り込み済みCSVの統計で有効な列が複数あると、同じ会話で列を補足できます。抽出要約の意味範囲や網羅性は保証しません。",
                },
                EngineCapability {
                    id: "attached-document-field-review",
                    scope: "指定した項目について、添付文書の要点を整理し、二つの文書で確認した値を出典付きで比較します。同じ会話に保存した添付文書も利用できます。",
                    permission: None,
                    limits: "明示された項目と対応する文書が対象です。読み取れない箇所や複数の値がある項目は、値の違いと分けて表示します。",
                },
                EngineCapability {
                    id: "bounded-code",
                    scope: "構造化仕様、文中の戻り式、数値の条件分岐・回文判定・リストの件数や合計や抽出・順序付き重複除去を指定した日本語からPython関数を生成します。入力と期待値を指定して検証でき、貼り付けた対応コードでは整数比較の境界の誤りを試験から特定できる場合に差分を提案して再検証します。",
                    permission: None,
                    limits: "対応する単一関数を内蔵の有限評価器で検証します。引数名を使う条件では関数の名前と引数を指定してください。条件付きのリスト集計には整数を使います。回文判定は文字の大小や句読点を区別します。",
                },
                EngineCapability {
                    id: "local-wasm-execution",
                    scope: "発話ごとに許可を得たWebAssemblyを別プロセスで実行し、標準出力・標準エラー・終了状態と生成ファイルを記録します。添付CSVのグループ別の件数や整数合計に使うWebAssemblyも生成して実行できます。",
                    permission: Some(CapabilityPermission::LocalWasmExecution),
                    limits: "信頼するローカルNodeを事前指定します。ゲストは許可済みのWASI入出力と宣言済み生成物だけを扱い、実行時間と容量に上限があります。",
                },
                EngineCapability {
                    id: "web-grounded-answer",
                    scope: "対応する日本語の説明質問では保存知識の充足を確認し、必要時に許可済みWebを探索します。取得した本文の記述を出典・時点・ハッシュ付きで回答します。明示された標高の換算と、保存済み本文からの根拠付き抜粋も扱います。",
                    permission: Some(CapabilityPermission::ExactHttpsHost {
                        host: SearchProvider::Wikipedia.host(),
                    }),
                    limits: "Wikipedia日本語版の対応本文と短い話題に限られます。抜粋は記事冒頭で読み取れた範囲が対象です。本文不足や通信・許可の失敗時は保留します。",
                },
                EngineCapability {
                    id: "web-research-comparison",
                    scope: "指定した話題を日本語Wikipediaで検索し、取得した二つの本文から根拠付きで共通する語句と記述差を示します。別件として指定された計算は通信停止時も処理します。",
                    permission: Some(CapabilityPermission::ExactHttpsHost {
                        host: SearchProvider::Wikipedia.host(),
                    }),
                    limits: "取得候補は最大4件です。取得時刻は記録しますが、掲載・改訂日時や資料の独立性、引用の意味上の矛盾は確定しません。",
                },
                EngineCapability {
                    id: "current-news-nhk",
                    scope: "今日・今朝・昨日・直近24時間のニュースを、NHKの公開一覧で見つけ、許可先で取得した記事本文と掲載日時を確認して出典付きで返します。",
                    permission: Some(CapabilityPermission::ExactHttpsHost {
                        host: NewsPublisher::Nhk.host(),
                    }),
                    limits: "対象は公開一覧で発見できた対応記事です。時刻は日本時間を使用し、取得時刻とは区別します。通信設定は起動時の許可先に限ります。",
                },
                EngineCapability {
                    id: "current-news-sankei",
                    scope: "今日・今朝・昨日・直近24時間のニュースを、産経新聞の公開一覧で見つけ、許可先で取得した記事本文と掲載日時を確認して出典付きで返します。",
                    permission: Some(CapabilityPermission::ExactHttpsHost {
                        host: NewsPublisher::Sankei.host(),
                    }),
                    limits: "対象は公開一覧で発見できた対応記事です。時刻は日本時間を使用し、取得時刻とは区別します。通信設定は起動時の許可先に限ります。",
                },
                EngineCapability {
                    id: "local-file-delivery",
                    scope: "元Goalが要求したUTF-8成果を、明示許可されたパスへ保存して読み戻します。取消や不明な受領は同じ作用IDで照合します。",
                    permission: Some(CapabilityPermission::ExactLocalWritePath),
                    limits: "保存先は信頼できるローカル領域に限ります。異なる既存内容を上書きせず、不明な書込結果は確認します。",
                },
                EngineCapability {
                    id: "local-png-image",
                    scope: "配置・色・寸法を指定した図形のPNG生成と、添付PNG/JPEGの指定範囲の塗り替えを行い、明示許可したPNGへ保存します。",
                    permission: Some(CapabilityPermission::ExactLocalWritePath),
                    limits: "円・四角形・三角形を扱います。座標は画像の左上が原点です。画像編集は指定範囲外の画素を保持します。",
                },
                EngineCapability {
                    id: "display-png-image",
                    scope: "指定した図形の描画や添付画像の範囲編集を、保存先の指定なしでPNGとして表示します。",
                    permission: None,
                    limits: "図形は円・四角形・三角形です。範囲編集では指定した長方形の外の画素を保持します。",
                },
                EngineCapability {
                    id: "japanese-wav-input",
                    scope: "Windowsのチャットで明示したWAVの日本語音声を認識し、同じ会話の発話として扱います。",
                    permission: Some(CapabilityPermission::ExplicitLocalWave),
                    limits: "固定のJulius GMM資産と16 kHz・16 bit・モノラルのPCM WAVが必要です。最長10秒かつ依頼の入力上限内です。",
                },
                EngineCapability {
                    id: "japanese-wav-input-stream",
                    scope: "Windowsのローカルブラウザーで選んだ日本語WAVを認識し、同じ会話の発話として扱います。",
                    permission: None,
                    limits: "固定したJulius資産が必要です。16 kHz・16 bit・モノラルPCMの1発話で最長10秒、依頼の入力上限内です。",
                },
                EngineCapability {
                    id: "japanese-answer-wav",
                    scope: "「前の回答を音声ファイルにして」で、同じ会話の採用済み日本語回答をWAVに保存します。",
                    permission: Some(CapabilityPermission::ExactLocalWritePath),
                    limits: "対応する読み辞書と音声データの起動時指定が必要です。本文は最大300文字、音声は最大15秒で実行予算にも従います。",
                },
                EngineCapability {
                    id: "display-japanese-answer-wav",
                    scope: "同じ会話で採用済みの日本語回答を、保存先の指定なしで再生できるWAVにします。",
                    permission: None,
                    limits: "起動時に対応する音声資産が必要です。本文は最大300文字、音声は最大15秒で実行予算にも従います。",
                },
                EngineCapability {
                    id: "search-wikipedia-metadata",
                    scope: "「検索 wikipedia: 検索語」で、日本語Wikipediaの候補メタデータを検索します。",
                    permission: Some(CapabilityPermission::ExactHttpsHost {
                        host: SearchProvider::Wikipedia.host(),
                    }),
                    limits: METADATA_SEARCH_LIMITS,
                },
                EngineCapability {
                    id: "search-crossref-metadata",
                    scope: "「検索 crossref: 検索語」で、Crossrefの文献メタデータを検索します。",
                    permission: Some(CapabilityPermission::ExactHttpsHost {
                        host: SearchProvider::Crossref.host(),
                    }),
                    limits: METADATA_SEARCH_LIMITS,
                },
                EngineCapability {
                    id: "search-europepmc-metadata",
                    scope: "「検索 europepmc: 検索語」で、Europe PMCの文献メタデータを検索します。",
                    permission: Some(CapabilityPermission::ExactHttpsHost {
                        host: SearchProvider::EuropePmc.host(),
                    }),
                    limits: METADATA_SEARCH_LIMITS,
                },
            ],
            known_limits: &[
                "外部LLM、任意コード実行、任意のWebページ・PDF・画像の解釈は提供しません。",
                "永続状態には対応するローカルSQLite環境が必要です。旧Rust状態の移行は明示した対応版に限られます。",
                "対象OSとCPUはこのバイナリのビルド対象です。プラットフォームの保証は表しません。",
                "非ニューラル模型の実版と成果物は採用された各結果のContentPlanに記録します。",
                "能力の列挙は通信許可や到達の確認ではありません。対応するURLを直接読む場合は、指定URLと各リダイレクト先のHTTPSホストを個別に許可する必要があります。",
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
