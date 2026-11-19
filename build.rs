// rust-embed bakes frontend/dist into release builds but cannot tell Cargo to
// re-embed when it changes, hence the rerun directive. The folder is created
// (empty) only because Cargo treats a missing rerun path as changed on every
// build, which would recompile the crate each time until a frontend is built.
fn main() {
    std::fs::create_dir_all("frontend/dist").expect("create frontend/dist");
    println!("cargo::rerun-if-changed=frontend/dist");
}
