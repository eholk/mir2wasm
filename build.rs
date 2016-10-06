extern crate cmake;

use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use std::thread;

/// Build from https://wasm-stat.us that we are known to work with.
const WASM_BUILD: &'static str = "11705";

fn main() {
    let cmake = thread::spawn(|| {
        if !Path::new("binaryen/.git").exists() {
            Command::new("git").args(&["submodule", "update", "--init"])
                .status().expect("error updating submodules");
        }
        cmake::Config::new("binaryen")
            .define("BUILD_STATIC_LIB", "ON")
            .build()
    });

    let toolchain = thread::spawn(|| {
        if env::var("HOST").unwrap().contains("linux") {
            update_wasm_toolchain();
        }
    });

    let dst = cmake.join().unwrap();
    let _ = toolchain.join();

    println!("cargo:rustc-link-search=native={}/build/lib", dst.display());
    println!("cargo:rustc-link-lib=static=binaryen");
    println!("cargo:rustc-link-lib=static=passes");
    println!("cargo:rustc-link-lib=static=support");
    println!("cargo:rustc-link-lib=static=emscripten-optimizer");
    println!("cargo:rustc-link-lib=static=asmjs");

    print_deps(Path::new("binaryen"));
}

fn print_deps(path: &Path) {
    for e in path.read_dir().unwrap().filter_map(|e| e.ok()) {
        let file_type = e.file_type().unwrap();
        if file_type.is_dir() {
            print_deps(&e.path());
        } else {
            println!("cargo:rerun-if-changed={}", e.path().display());
        }
    }
}

/// Downloads the wasm toolchain from https://wasm-stat.us/ if necessary.
fn update_wasm_toolchain() {
    const WASM_INSTALL_VER : &'static str = ".wasm-install-ver";

    // Check if the right version is already in .wasm-install-ver
    if let Ok(mut file) = File::open(WASM_INSTALL_VER) {
        let mut contents = String::new();
        if let Ok(_) = file.read_to_string(&mut contents) {
            if WASM_BUILD == contents.trim() {
                return;
            }
        }
    }

    // If we got here, we need to update.
    const TMP_FILE : &'static str = ".wasm-install.tbz2";

    let url = format!("https://storage.googleapis.com/wasm-llvm/builds/git/wasm-binaries-{}.tbz2",
                      WASM_BUILD);
    let url = url.as_str();
    Command::new("wget").args(&[url, "-O", TMP_FILE]).status()
        .and_then(|_| {
            Command::new("tar").args(&["xjf", TMP_FILE]).status()
        })
        .and_then(|_| {
            File::create(WASM_INSTALL_VER)
        })
        .and_then(|mut file| {
            writeln!(file, "{}", WASM_BUILD)
        })
        .expect("error downloading wasm toolchain");
}
