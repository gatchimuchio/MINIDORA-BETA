fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if matches!(
        std::env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("windows")
    ) && matches!(std::env::var("CARGO_CFG_TARGET_ENV").as_deref(), Ok("msvc"))
    {
        // Unoptimized Core and context/recovery paths can exhaust MSVC's
        // default stack during ordinary multi-turn use. Reserve room without
        // raising the initial commit.
        println!("cargo::rustc-link-arg-bin=minidora-rust=/STACK:4194304");
    }
}
