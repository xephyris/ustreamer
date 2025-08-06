use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-link-lib=rockchip_mpp"); // Link to MPP
    println!("cargo:rustc-link-search=native=mpp/inc"); // Adjust path
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header("mpp/wrapper.h")
        .clang_arg("-Impp/inc")
        .clang_arg("-Impp/osal/inc")
        .blocklist_item("FP_NAN")
        .blocklist_item("FP_INFINITE")
        .blocklist_item("FP_ZERO")
        .blocklist_item("FP_SUBNORMAL")
        .blocklist_item("FP_NORMAL")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}