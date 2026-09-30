//! Builds vendored libsoxr as a shared library (LGPL-2.1: shipped as a
//! separate DLL/.so beside the app so it can be replaced), and copies it next
//! to the test/bin executables so they run from `target/`.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("../../third_party/soxr");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", src.join("src").display());

    let dst = cmake::Config::new(&src)
        .profile("Release")
        .define("BUILD_SHARED_LIBS", "ON")
        .define("BUILD_TESTS", "OFF")
        .define("BUILD_EXAMPLES", "OFF")
        .define("WITH_OPENMP", "OFF")
        .define("WITH_LSR_BINDINGS", "OFF")
        .define("WITH_DEV_TRACE", "OFF")
        .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5")
        .build();

    let lib = dst.join("lib");
    let bin = dst.join("bin");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=soxr");

    // target/<profile>/build/fl-audio-<hash>/out → target/<profile>
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let profile_dir = out.ancestors().nth(3).unwrap().to_path_buf();
    let runtime: Vec<PathBuf> = if cfg!(windows) {
        vec![bin.join("soxr.dll")]
    } else {
        std::fs::read_dir(&lib)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| is_shared_object(p)).collect())
            .unwrap_or_default()
    };
    for f in runtime {
        for d in [profile_dir.clone(), profile_dir.join("deps"), profile_dir.join("examples")] {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::copy(&f, d.join(f.file_name().unwrap()));
        }
    }
    println!("cargo:soxr_runtime_dir={}", if cfg!(windows) { bin.display() } else { lib.display() });
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    }
}

fn is_shared_object(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(".so") || n.ends_with(".dylib"))
}
