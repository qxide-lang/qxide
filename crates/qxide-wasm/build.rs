fn main() {
    // The interpreter recurses per call and block; the CLI gives it a 64 MiB thread. wasm's default
    // shadow stack is 1 MiB, so reserve more so the call-depth limit, not a stack overflow, stops it.
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo::rustc-link-arg-cdylib=-zstack-size={}", 16 << 20);
    }
}
