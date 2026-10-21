fn main() {
    // refinery's procedural macro embeds the directory at compile time. Track additions,
    // not only SQL files already present in the previous expansion.
    println!("cargo:rerun-if-changed=migrations");
}
