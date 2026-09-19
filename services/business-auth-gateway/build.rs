fn main() {
    // SQLx embeds the migrations; additions must invalidate incremental builds too.
    println!("cargo:rerun-if-changed=migrations");
}
