fn main() {
    // Explicit include dir so GNU windres (used when cross-compiling from
    // Linux) finds the icon and manifest the same way rc.exe does on MSVC.
    embed_resource::compile(
        "assets/wrecktangle.rc",
        embed_resource::ParamsIncludeDirs(["assets"]),
    )
    .manifest_required()
    .unwrap();

    // Build scripts are compiled for the host, so only the target env var can
    // tell a Windows build from a Linux `cargo test`.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        slint_build::compile_with_config(
            "ui/settings.slint",
            slint_build::CompilerConfiguration::new()
                .with_style("fluent".into())
                .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
        )
        .unwrap();
    }
}
