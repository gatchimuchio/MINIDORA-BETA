use minidora_domain::{Lifecycle, Proposition, canonical, digest, identity};
fn main() {
    let record = Proposition {
        subject: "ミニドラ".into(),
        predicate: "is".into(),
        object: "決定論的".into(),
        negative: true,
        scope: "仮定2026".into(),
    };
    let bytes = canonical(&record).expect("exact record");
    println!(
        "{}",
        serde_json::json!({"record":record,"canonical":String::from_utf8(bytes.clone()).unwrap(),"sha256":digest(&bytes),"identity":identity(&record).unwrap(),"early_commit":Lifecycle::Running.permits(Lifecycle::Commit),"adopted_commit":Lifecycle::Adopted.permits(Lifecycle::Commit)})
    );
}
