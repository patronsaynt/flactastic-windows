use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let taglib_src = manifest.join("../../third_party/taglib");
    println!("cargo:rerun-if-changed=csrc");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", taglib_src.join("taglib").display());
    println!("cargo:rerun-if-changed={}", taglib_src.join("bindings").display());

    // Always Release: Rust links the release CRT (/MD) even in debug builds,
    // and mixing /MDd TagLib objects with /MD ones fails to link on MSVC.
    let dst = cmake::Config::new(&taglib_src)
        .profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("BUILD_BINDINGS", "ON")
        .define("BUILD_TESTING", "OFF")
        .define("BUILD_EXAMPLES", "OFF")
        .define("WITH_ZLIB", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5")
        .build();

    let include = dst.join("include");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("csrc/fl_taglib.cpp")
        .include(&include)
        .include("csrc")
        .define("TAGLIB_STATIC", None)
        .warnings(false)
        .compile("fl_taglib");

    println!("cargo:rustc-link-search=native={}", dst.join("lib").display());
    println!("cargo:rustc-link-lib=static=tag_c");
    println!("cargo:rustc-link-lib=static=tag");
}
