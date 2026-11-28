//! Bind deterministic image results to their verified output destination.
use crate::image_artifact;
use crate::input::verify_image_input;
use crate::output::{
    PNG_DISPLAY_RENDERER_VERSION, PNG_RENDERER_VERSION, matches_render_target, valid_png_path,
    validate_png_render,
};
use crate::store::Store;
use minidora_domain::{
    ContentPlan, Guarantee, ImageOperation, InputBundle, Intent, RenderObservation, Result,
    ResultRecord, canonical, digest, failure,
};
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const PREDICATE: &str = "IMAGE_ARTIFACT_V1";

#[derive(Serialize)]
struct ImageReport<'a> {
    kind: &'static str,
    renderer: &'static str,
    input_fingerprint: &'a str,
    operation: &'a ImageOperation,
    source_sha256: Option<String>,
    png_sha256: String,
    png_bytes: usize,
    width: u32,
    height: u32,
}

pub(crate) struct ImageOutcome {
    pub units: Vec<String>,
    pub dependencies: Vec<String>,
    pub png: Vec<u8>,
    report: Vec<u8>,
}

pub(crate) fn check_target(path: Option<&Path>) -> Result<()> {
    if path.is_none_or(valid_png_path) {
        return Ok(());
    }
    Err(failure(
        "IMAGE_PNG_TARGET_REQUIRED",
        "保存を許可したPNGの絶対パスを /output で選んでください。",
    ))
}

pub(crate) fn renderer_version(path: Option<&Path>) -> &'static str {
    if path.is_some() {
        PNG_RENDERER_VERSION
    } else {
        PNG_DISPLAY_RENDERER_VERSION
    }
}

fn mismatch() -> minidora_domain::Error {
    failure(
        "IMAGE_ARTIFACT_MISMATCH",
        "画像の依頼・参照画像・出力を照合できません。",
    )
}

fn evaluate(bundle: &InputBundle, started: Instant) -> Result<ImageOutcome> {
    let output_path = bundle.request.output_path.as_deref();
    check_target(output_path)?;
    verify_image_input(bundle)?;
    let Intent::ProduceImage { operation } = &bundle.meaning.intent else {
        return Err(mismatch());
    };
    let attachment = match (operation, bundle.request.attachments.as_slice()) {
        (ImageOperation::Draw { .. }, []) => None,
        (ImageOperation::Fill { .. }, [image]) => Some(image),
        _ => return Err(mismatch()),
    };
    let budget = &bundle.request.budget;
    let deadline = started
        .checked_add(Duration::from_millis(budget.deadline_ms))
        .ok_or_else(|| failure("DEADLINE", "画像処理の期限を確定できません。"))?;
    let png = image_artifact::render(operation, attachment, deadline)?;
    let (width, height) = image_artifact::inspect_png(&png)?;
    let source_sha256 = match attachment {
        Some(image) => Some(digest(&image.image_bytes()?.ok_or_else(mismatch)?)),
        None => None,
    };
    let detail = match operation {
        ImageOperation::Draw { shapes, .. } => {
            format!("指定の位置と色で図形{}個を描きました。", shapes.len())
        }
        ImageOperation::Fill { rectangle, .. } => format!(
            "左上 ({}, {})、幅{}、高さ{} px の範囲を塗り、範囲外の画素を保持しました。",
            rectangle.x, rectangle.y, rectangle.width, rectangle.height
        ),
    };
    let units = vec![format!("{} PNG {}×{} px。", detail, width, height)];
    let report = ImageReport {
        kind: "minidora.image-artifact.v1",
        renderer: renderer_version(output_path),
        input_fingerprint: &bundle.fingerprint,
        operation,
        source_sha256,
        png_sha256: digest(&png),
        png_bytes: png.len(),
        width,
        height,
    };
    let report_bytes = canonical(&report)?;
    if png
        .len()
        .saturating_add(report_bytes.len())
        .saturating_add(units[0].len())
        > budget.max_evidence_bytes
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "PNG画像と根拠が依頼の証拠予算を超えました。",
        ));
    }
    if Instant::now() >= deadline {
        return Err(failure("DEADLINE", "画像の確認中に処理期限へ達しました。"));
    }
    let dependencies = vec![report.png_sha256, digest(&report_bytes)];
    Ok(ImageOutcome {
        units,
        dependencies,
        png,
        report: report_bytes,
    })
}

pub(crate) fn produce(
    store: &Store,
    bundle: &InputBundle,
    started: Instant,
) -> Result<ImageOutcome> {
    let image = evaluate(bundle, started)?;
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let image_ref = store.put_bytes(actor, case, &image.png)?;
    let report_ref = store.put_bytes(actor, case, &image.report)?;
    if image.dependencies != [image_ref, report_ref] {
        return Err(mismatch());
    }
    Ok(image)
}

pub(crate) fn verify_delivery(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
    render: &RenderObservation,
) -> Result<()> {
    let image = evaluate(bundle, Instant::now())?;
    let [image_ref, report_ref] = result.dependencies.as_slice() else {
        return Err(mismatch());
    };
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let output_path = bundle.request.output_path.as_deref();
    if result.guarantee != Guarantee::Computed
        || plan.guarantee != result.guarantee
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || image.dependencies != result.dependencies
        || plan.required_units != image.units
        || result.text != image.units.join("\n")
        || result.text != plan.text
        || !matches_render_target(output_path, render.target.as_ref())
        || render.renderer_version != renderer_version(output_path)
        || render.bytes != image.png
        || !validate_png_render(plan, render)
        || store.get_bytes(actor, case, image_ref)? != image.png
        || store.get_bytes(actor, case, report_ref)? != image.report
    {
        return Err(mismatch());
    }
    Ok(())
}
