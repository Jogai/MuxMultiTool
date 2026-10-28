//! MuxMultiTool server binary.
//!
//! Runs the axum server as the container's main process. Every setting is a
//! flag with an `MMT_*` environment variable behind it, so a compose file
//! needs no command line. Logs go to stderr; `RUST_LOG` filters them.

use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use muxmultitool_core::ffmpeg::tools::{FFMPEG_ENV, FFPROBE_ENV};
use muxmultitool_server::{Config, DEFAULT_FRONTEND_DIR, DEFAULT_PORT, Server, VERSION};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

/// Self-hostable ffmpeg GUI: the server behind the web app.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
	/// IP address to listen on. Use 0.0.0.0 in a container.
	#[arg(long, env = "MMT_LISTEN", default_value = "127.0.0.1")]
	listen: IpAddr,

	/// Port to listen on.
	#[arg(long, env = "MMT_PORT", default_value_t = DEFAULT_PORT)]
	port: u16,

	/// Folder the app may read media from and write outputs to. Repeat the flag
	/// for several folders, or separate them with commas in the variable.
	#[arg(
		long = "media-root",
		env = "MMT_MEDIA_ROOTS",
		value_delimiter = ',',
		value_name = "DIR"
	)]
	media_roots: Vec<PathBuf>,

	/// Path of the ffmpeg executable. Found on PATH by default.
	#[arg(long, env = FFMPEG_ENV, value_name = "PATH")]
	ffmpeg: Option<PathBuf>,

	/// Path of the ffprobe executable. Found on PATH by default.
	#[arg(long, env = FFPROBE_ENV, value_name = "PATH")]
	ffprobe: Option<PathBuf>,

	/// Folder with the built frontend (the output of `vite build`).
	#[arg(long, env = "MMT_FRONTEND_DIR", default_value = DEFAULT_FRONTEND_DIR, value_name = "DIR")]
	frontend_dir: PathBuf,
}

impl From<Cli> for Config {
	fn from(cli: Cli) -> Self {
		let mut config = Self {
			address: cli.listen,
			port: cli.port,
			media_roots: cli.media_roots,
			frontend_dir: cli.frontend_dir,
			..Self::default()
		};
		if let Some(ffmpeg) = cli.ffmpeg {
			config.tools.ffmpeg = ffmpeg;
		}
		if let Some(ffprobe) = cli.ffprobe {
			config.tools.ffprobe = ffprobe;
		}
		config
	}
}

#[tokio::main]
async fn main() -> ExitCode {
	tracing_subscriber::fmt()
		.with_env_filter(
			EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
		)
		.with_writer(std::io::stderr)
		.init();

	let config = Config::from(Cli::parse());
	info!(
		version = VERSION,
		ffmpeg = %config.tools.ffmpeg.display(),
		ffprobe = %config.tools.ffprobe.display(),
		frontend_dir = %config.frontend_dir.display(),
		media_roots = ?config.media_roots,
		"starting"
	);

	let server = match Server::bind(&config).await {
		Ok(server) => server,
		Err(error) => {
			error!(addr = %config.socket_addr(), %error, "cannot bind");
			return ExitCode::FAILURE;
		}
	};
	match server.run(muxmultitool_server::shutdown_signal()).await {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			error!(%error, "server failed");
			ExitCode::FAILURE
		}
	}
}
