use std::{env, fs, path::PathBuf, process::Command};

fn find_lean_include_dir() -> PathBuf {
    // 1. Try LEAN_SYSROOT env var
    if let Ok(sysroot) = env::var("LEAN_SYSROOT") {
        let inc = PathBuf::from(sysroot).join("include");
        if inc.exists() {
            return inc;
        }
    }
    // 2. Try `lean --print-prefix`
    if let Ok(output) = Command::new("lean").arg("--print-prefix").output()
        && output.status.success()
    {
        let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let inc = PathBuf::from(prefix).join("include");
        if inc.exists() {
            return inc;
        }
    }
    panic!(
        "Cannot find Lean include directory. \
     Set LEAN_SYSROOT or ensure `lean` is on PATH."
    );
}

/// The `lib` directory beside the `clang` bindgen takes include paths from
/// (e.g. `/usr/lib/llvm-21/lib`), if it holds a libclang. Otherwise clang-sys
/// loads the newest libclang it finds, and mixing two LLVM versions' headers
/// leaves `lean.h`'s C11 atomics undeclared.
fn libclang_beside_clang() -> Option<PathBuf> {
    let clang = clang_sys::support::Clang::find(None, &[])?
        .path
        .canonicalize()
        .ok()?;
    let lib = clang.parent()?.parent()?.join("lib");
    let mut entries = fs::read_dir(&lib).ok()?.flatten();
    entries
        .any(|entry| entry.file_name().to_string_lossy().starts_with("libclang"))
        .then_some(lib)
}

fn main() {
    let lean_include = find_lean_include_dir();
    let lean_h = lean_include.join("lean").join("lean.h");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let wrapper_c = out_dir.join("lean_static_fns.c");

    if env::var_os("LIBCLANG_PATH").is_none()
        && let Some(lib) = libclang_beside_clang()
    {
        // SAFETY: the build script is single-threaded.
        unsafe { env::set_var("LIBCLANG_PATH", lib) };
    }

    // Generate C wrappers for lean.h's static inline functions and
    // Rust bindings for all types and functions.
    bindgen::Builder::default()
        .header(lean_h.to_str().unwrap())
        .clang_arg(format!("-I{}", lean_include.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .wrap_static_fns(true)
        .wrap_static_fns_path(&wrapper_c)
        // lean_get_rc_mt_addr returns `_Atomic(int)*` which bindgen
        // cannot wrap. Types using `_Atomic` are made opaque.
        .blocklist_function("lean_get_rc_mt_addr")
        .opaque_type("lean_thunk_object")
        .opaque_type("lean_task_object")
        .generate()
        .expect("bindgen failed to process lean.h")
        .write_to_file(out_dir.join("lean.rs"))
        .expect("Couldn't write bindings");

    // Compile the generated C wrappers into a static library.
    cc::Build::new()
        .file(&wrapper_c)
        .include(&lean_include)
        .compile("lean_static_fns");

    println!("cargo:rerun-if-env-changed=LEAN_SYSROOT");
    println!("cargo:rerun-if-env-changed=LIBCLANG_PATH");
    println!("cargo:rerun-if-env-changed=CLANG_PATH");
    println!("cargo:rerun-if-changed={}", lean_h.display());
    println!("cargo:rerun-if-changed=build.rs");
}
