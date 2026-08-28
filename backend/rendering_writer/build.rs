fn main() {
    println!("cargo:rerun-if-changed=c/save_clean.c");
    cc::Build::new()
        .file("c/save_clean.c")
        .compile("rendering_writer_clean");
}
