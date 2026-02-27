fn main() {
    // Compile the C shim for IOKit HID access
    cc::Build::new()
        .file("src/sensor/iokit.c")
        .flag("-framework")
        .flag("IOKit")
        .flag("-framework")
        .flag("CoreFoundation")
        .compile("iokit_shim");

    // Link macOS frameworks
    println!("cargo:rustc-link-lib=framework=IOKit");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
}
