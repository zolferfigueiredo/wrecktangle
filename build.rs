fn main() {
    // Explicit include dir so GNU windres (used when cross-compiling from
    // Linux) finds the icon and manifest the same way rc.exe does on MSVC.
    embed_resource::compile(
        "assets/wrecktangle.rc",
        embed_resource::ParamsIncludeDirs(["assets"]),
    )
    .manifest_required()
    .unwrap();
}
