fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile OpenTofu provider gRPC protocol definitions.
    // tfplugin5: Used by older providers (protocol version 5.x)
    // tfplugin6: Used by newer providers (protocol version 6.x)
    let mut config = prost_build::Config::new();
    config.disable_comments(["."]);

    tonic_build::configure()
        .build_server(false) // We only need the client side
        .compile_protos_with_config(
            config,
            &["proto/tfplugin5.proto", "proto/tfplugin6.proto"],
            &["proto/"],
        )?;
    Ok(())
}
