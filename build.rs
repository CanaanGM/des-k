// Compiles res/des-k.rc into the exe, so it carries des-k's icon and its version card.
fn main() {
    // env! bakes the version into this script, so a version bump rebuilds it and the card follows.
    let version = env!("CARGO_PKG_VERSION");
    let mut parts = version.split(['.', '-']);
    let mut next = || parts.next().unwrap_or("0").to_owned();
    let macros = [
        format!("VER_MAJOR={}", next()),
        format!("VER_MINOR={}", next()),
        format!("VER_PATCH={}", next()),
        format!("VER_TEXT=\"{version}\""),
    ];
    embed_resource::compile("res/des-k.rc", &macros).manifest_required().unwrap();
}
