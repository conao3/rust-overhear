use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=APRIL_LIB_DIR");

    if env::var("CARGO_FEATURE_APRIL").is_err() {
        return;
    }

    // libaprilasr.so は nixpkgs の livecaptions の出力に同梱されている。
    // april-asr 単体のパッケージは nixpkgs に無いため、これを直接リンクする。
    // flake の devShell が APRIL_LIB_DIR を注入する。
    if let Ok(dir) = env::var("APRIL_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
        // 実行時にも解決できるよう rpath を埋める。
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }
    println!("cargo:rustc-link-lib=dylib=aprilasr");
}
