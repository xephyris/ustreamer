use std::env;
use std::path::{Path, PathBuf};

#[cfg(feature = "rk_hw_accel")]
fn main() {
    println!("cargo:rustc-check-cfg=cfg(mpp_accel)");
    println!("cargo:rustc-check-cfg=cfg(rga_converter)");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    if Path::new("/dev/mpp_service").exists() {
        println!("cargo:rustc-cfg=mpp_accel");
        println!("cargo:rustc-link-lib=rockchip_mpp"); 
        println!("cargo:rustc-link-search=native=mpp/inc"); 
        let bindings_mpp = bindgen::Builder::default()
            .header("mpp/wrapper.h")
            .clang_arg("-Impp/inc")
            .clang_arg("-Impp/osal/inc")
            .blocklist_item("FP_NAN")
            .blocklist_item("FP_INFINITE")
            .blocklist_item("FP_ZERO")
            .blocklist_item("FP_SUBNORMAL")
            .blocklist_item("FP_NORMAL")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("Unable to generate bindings");

       
        let bindings_dir = out_path.join("mpp");
        std::fs::create_dir_all(&bindings_dir).expect("Failed to create output directory");

        bindings_mpp
            .write_to_file(out_path.join("mpp/bindings.rs"))
            .expect("Couldn't write bindings!");
    }
    if Path::new("/dev/rga").exists() {
        println!("cargo:rustc-cfg=rga_converter");
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-search=native=rga");
        println!("cargo:rustc-link-lib=static=rga");
        println!("cargo:rustc-link-search=native=rga/include");
        let bindings_rga = bindgen::Builder::default()
            .header("rga/wrapper.h")
            .clang_arg("-Irga/include")
            .clang_arg("-Irga/im2d_api")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("Unable to generate bindings");

        let bindings_dir = out_path.join("rga");
        std::fs::create_dir_all(&bindings_dir).expect("Failed to create output directory"); 

        bindings_rga
            .write_to_file(out_path.join("rga/bindings.rs"))
            .expect("Couldn't write bindings!");
    }
}

#[cfg(not(feature = "rk_hw_accel"))]
fn main() {}