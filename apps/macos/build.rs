fn main() {
    println!("cargo:rustc-link-lib=framework=Carbon");
    println!("cargo:rustc-link-lib=framework=InputMethodKit");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-arg=-mmacosx-version-min=13.0");
}
