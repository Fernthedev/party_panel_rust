use std::{env, path::PathBuf};

use qpm_cli::{
    models::package::{PackageConfigExtensions, SharedPackageConfigExtensions},
    package::models::{dependency::SharedPackageConfig, package::PackageConfig},
    repository,
    resolver::dependency,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    prost_build::compile_protos(&["src/items.proto", "src/packets.proto"], &["src/"])?;

    let manifest_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // change if qpm.shared.json modified
    println!(
        "cargo:rerun-if-changed={}",
        manifest_path.join("qpm.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_path.join("qpm.shared.json").display()
    );

    let include_dir = manifest_path.join("extern").join("includes");
    let lib_path = manifest_path.join("extern").join("libs");

    let package = PackageConfig::read(&manifest_path)?;
    let mut repo = repository::useful_default_new(false)?;
    let (shared_package, resolved_deps) =
        SharedPackageConfig::resolve_from_package(package, &repo)?;

    dependency::restore(&manifest_path, &shared_package, &resolved_deps, &mut repo)?;

    shared_package.write(&manifest_path)?;

    // cbindgen::Builder::new()
    //   .with_crate(&manifest_path)
    //   .generate()
    //   .expect("Unable to generate bindings")
    //   .write_to_file("include/bindings.h");

    // // The bindgen::Builder is the main entry point
    // // to bindgen, and lets you build up options for
    // // the resulting bindings.
    // let bindings = bindgen::Builder::default()
    //     // The input header we would like to generate
    //     // bindings for.
    //     .header("wrapper.h")
    //     // Tell cargo to invalidate the built crate whenever any of the
    //     // included header files changed.
    //     .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
    //     // Finish the builder and generate the bindings.
    //     .generate()
    //     // Unwrap the Result and panic on failure.
    //     .expect("Unable to generate bindings");

    // // Write the bindings to the $OUT_DIR/bindings.rs file.
    // let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    // bindings
    //     .write_to_file(out_path.join("quest_compat.rs"))
    //     .expect("Couldn't write bindings!");

    println!("cargo:rustc-link-search={}", lib_path.display());
    println!("cargo:rustc-link-lib=songcore");

    cc::Build::new()
        .cpp(true) // Switch to C++ library compilation.
        .file("src/quest_compat.cpp")
        .cpp_link_stdlib("c++_static") // use libstdc++
        .flag_if_supported("-std=gnu++20")
        .flag_if_supported("-frtti")
        .flag_if_supported("-fexceptions")
        .flag_if_supported("-fdeclspec")
        .flag_if_supported("-Wno-invalid-offsetof")
        .flag("-DUNITY_2021")
        .flag("-DUNITY_2022")
        .flag("-DHAS_CODEGEN")
        .flag("-DNEED_UNSAFE_CSHARP")
        .flag("-DQUEST")
        .flag("-DFMT_HEADER_ONLY")
        // system include
        .flag(format!(
            "-isystem{}",
            include_dir // fmt/fmt/include
                .join("fmt")
                .join("fmt")
                .join("include")
                .display()
        ))
        .flag(format!(
            "-isystem{}",
            include_dir // libil2cpp/il2cpp/libil2cpp
                .join("libil2cpp")
                .join("il2cpp")
                .join("libil2cpp")
                .display()
        ))
        .flag(format!(
            "-isystem{}",
            include_dir // baselib include
                .join("libil2cpp")
                .join("il2cpp")
                .join("external")
                .join("baselib")
                .join("Include")
                .display()
        ))
        .flag(format!(
            "-isystem{}",
            include_dir // baselib android include
                .join("libil2cpp")
                .join("il2cpp")
                .join("external")
                .join("baselib")
                .join("Platforms")
                .join("Android")
                .join("Include")
                .display()
        ))
        .include(include_dir.join("bs-cordl").join("include"))
        .include(include_dir)
        .compile("quest_compat");
    Ok(())
}
