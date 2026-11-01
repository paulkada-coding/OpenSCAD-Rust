//! Builds the vendored Manifold kernel and links it statically.
//!
//! Sources live in `vendor/` so the build needs no network access:
//! - Manifold v3.5.4 (commit ce50d78021d64507f89e8c9fc2c2e51018117857), the
//!   subset needed for the library and its C bindings: `CMakeLists.txt`,
//!   `LICENSE`, `AUTHORS`, `cmake/`, `include/`, `src/`, `bindings/c/`.
//! - Clipper2 at commit 46f639177fe418f9689e8ddb74f08a870c71f5b4 (the pin in
//!   Manifold's `cmake/manifoldDeps.cmake`): `LICENSE` and `CPP/` without
//!   tests, examples and utilities.
//!
//! To upgrade, replace both trees from the new upstream tags and update the
//! versions above (and in the crate docs).
//!
//! Manifold and Clipper2 are built with CMake (Release, static, single
//! threaded: `MANIFOLD_PAR=OFF`). `shim/shim.cpp`, an exception-safe layer
//! over Manifold's C API, is compiled with `cc`. Everything uses the dynamic
//! release CRT (`/MD`), like Rust on MSVC.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifold_src = manifest.join("vendor").join("manifold");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed=vendor");

    // The kernel is C++: building it for the browser needs a C++ toolchain
    // for WebAssembly, not set up yet. Fail with a clear message instead of
    // CMake's linker errors.
    if env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32") {
        panic!(
            "the Manifold kernel is not available for WebAssembly yet, so `manifold` and \
             `mesher` (and the crates using them) cannot be built for wasm32; see docs/web.md"
        );
    }

    // Clipper2's CMakeLists writes `clipper.version.h` into its source tree,
    // which would touch vendor/ and retrigger this script on every build:
    // configure a private copy instead.
    let clipper_src = out_dir.join("clipper2-src");
    sync_dir(&manifest.join("vendor").join("clipper2"), &clipper_src)
        .expect("failed to copy vendor/clipper2 into OUT_DIR");

    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");

    // --- Manifold + Clipper2 (CMake) ---
    let mut cfg = cmake::Config::new(&manifold_src);
    cfg.profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("MANIFOLD_CROSS_SECTION", "ON")
        .define("MANIFOLD_CBIND", "ON")
        .define("MANIFOLD_PAR", "OFF")
        .define("MANIFOLD_TEST", "OFF")
        .define("MANIFOLD_PYBIND", "OFF")
        .define("MANIFOLD_JSBIND", "OFF")
        .define("MANIFOLD_DEBUG", "OFF")
        // Never download anything: Clipper2 comes from vendor/.
        .define("MANIFOLD_DOWNLOADS", "OFF")
        .define("MANIFOLD_USE_BUILTIN_CLIPPER2", "ON")
        // FetchContent reads the upper-case name, Manifold's own "is it
        // provided?" check the mixed-case one.
        .define("FETCHCONTENT_SOURCE_DIR_CLIPPER2", &clipper_src)
        .define("FETCHCONTENT_SOURCE_DIR_Clipper2", &clipper_src)
        .define("FETCHCONTENT_FULLY_DISCONNECTED", "ON")
        .define("CLIPPER2_USINGZ", "OFF");
    if msvc {
        // Explicit flags: otherwise the cmake crate replaces the Release
        // flags with cc's (no /O2, no /DNDEBUG) and silences warnings.
        // /EHs /EHc-: C++ exceptions may propagate through `extern "C"`
        // functions (manifoldc), so that the shim can catch them.
        // /MP: compile the files of each target in parallel.
        let common = "/DWIN32 /D_WINDOWS /nologo /Brepro /MP /W3";
        cfg.define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreadedDLL")
            .define("CMAKE_C_FLAGS", common)
            .define("CMAKE_CXX_FLAGS", format!("{common} /GR /EHs /EHc-"))
            .define("CMAKE_C_FLAGS_RELEASE", "/O2 /Ob2 /DNDEBUG")
            .define("CMAKE_CXX_FLAGS_RELEASE", "/O2 /Ob2 /DNDEBUG");
    }
    let dst = cfg.build();

    // --- Exception-safe shim (cc) ---
    let mut shim = cc::Build::new();
    shim.cpp(true)
        .std("c++17")
        .file(manifest.join("shim").join("shim.cpp"))
        .include(manifold_src.join("bindings").join("c").join("include"))
        // The configured copy, which has the generated `clipper.version.h`.
        .include(clipper_src.join("CPP").join("Clipper2Lib").join("include"))
        .warnings(true);
    if msvc {
        shim.flag("/EHs").flag("/EHc-").static_crt(false);
    }
    shim.compile("manifold_shim");

    // --- Link (dependents after dependencies' users) ---
    for lib in ["manifoldc", "manifold", "Clipper2"] {
        let dir = find_lib(&dst.join("lib"), lib)
            .or_else(|| find_lib(&dst, lib))
            .unwrap_or_else(|| panic!("static library '{lib}' not found under {}", dst.display()));
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-lib=static={lib}");
    }
    if !msvc {
        let target_vendor = env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
        let cxx = if target_vendor == "apple" {
            "c++"
        } else {
            "stdc++"
        };
        println!("cargo:rustc-link-lib={cxx}");
    }
}

/// Mirrors `src` into `dst`, rewriting only files whose content differs so
/// that unchanged files keep their timestamps (no needless C++ rebuilds).
fn sync_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            sync_dir(&from, &to)?;
        } else {
            let data = std::fs::read(&from)?;
            if std::fs::read(&to).ok().as_deref() != Some(data.as_slice()) {
                std::fs::write(&to, data)?;
            }
        }
    }
    Ok(())
}

/// Directory containing `name.lib` (MSVC) or `libname.a`, searched
/// recursively under `root`.
fn find_lib(root: &Path, name: &str) -> Option<PathBuf> {
    let candidates = [format!("{name}.lib"), format!("lib{name}.a")];
    let entries = std::fs::read_dir(root).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| candidates.iter().any(|c| c == f))
        {
            return Some(root.to_path_buf());
        }
    }
    subdirs.into_iter().find_map(|dir| find_lib(&dir, name))
}
