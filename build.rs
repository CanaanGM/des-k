// Compiles res/des-k.rc into the exe, so it carries des-k's icon.
fn main() {
    embed_resource::compile("res/des-k.rc", embed_resource::NONE).manifest_required().unwrap();
}
