fn main() {
    embed_resource::compile("assets/wectangle.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
