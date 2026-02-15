pub mod cache;
pub mod manager;
pub mod protocol;
pub mod registry;

/// Generated gRPC types from OpenTofu plugin protocol.
#[allow(clippy::all)]
#[allow(non_camel_case_types)]
pub mod tfplugin5 {
    #![doc(hidden)]
    tonic::include_proto!("tfplugin5");
}

#[allow(clippy::all)]
#[allow(non_camel_case_types)]
pub mod tfplugin6 {
    #![doc(hidden)]
    tonic::include_proto!("tfplugin6");
}

/// Protocol version supported by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolVersion {
    V5,
    V6,
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolVersion::V5 => write!(f, "5"),
            ProtocolVersion::V6 => write!(f, "6"),
        }
    }
}
