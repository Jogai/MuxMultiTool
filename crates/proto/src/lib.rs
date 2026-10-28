//! MuxMultiTool RPC contract.
//!
//! The `.proto` files in this crate define the ConnectRPC services. `buf generate`
//! produces the Rust and TypeScript code from them; generated code isn't committed.

// The generated trees are mounted as modules. They don't exist until `buf generate` has run.
// `allow`: generated code doesn't follow the workspace lints, and the allow is not an
// `expect` because the set of lints it trips depends on the proto files. `rustfmt::skip`
// keeps `cargo fmt --check` off the generated files.
/// Protobuf message types (buffa).
#[allow(missing_docs, clippy::pedantic)]
#[rustfmt::skip]
#[path = "generated/buffa/mod.rs"]
pub mod proto;

/// Connect service traits and clients. Message types live in [`proto`].
#[allow(missing_docs, clippy::pedantic)]
#[rustfmt::skip]
#[path = "generated/connect/mod.rs"]
pub mod connect;

#[cfg(test)]
mod tests {
	use super::proto::muxmultitool::system::v1::{GetVersionResponse, HealthStatus};
	use buffa::Message;

	#[test]
	fn messages_round_trip() {
		let response = GetVersionResponse {
			version: "0.1.0".into(),
			..Default::default()
		};
		let decoded = GetVersionResponse::decode_from_slice(&response.encode_to_vec()).unwrap();
		assert_eq!(decoded.version, "0.1.0");
		assert_eq!(HealthStatus::HEALTH_STATUS_OK as i32, 1);
	}
}
