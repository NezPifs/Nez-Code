#[cfg(nez_bazel)]
pub use code_mode_proto::nez::code_mode::v1::*;

#[cfg(not(nez_bazel))]
tonic::include_proto!("nez.code_mode.v1");

pub const MAX_IDENTIFIER_BYTES: usize = 256;
