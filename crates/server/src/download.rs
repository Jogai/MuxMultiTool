//! `GET /files/{id}`: downloads a file by its `FilesService` id.
//!
//! In the container the outputs land in server-side folders, and the
//! browser fetches them by URL. The id goes through [`MediaRoots::path`], so
//! the containment check is the same one every other file access uses.
//! `tower-http`'s `ServeFile` does the byte work: `Range` requests (single
//! ranges, `206`/`416`), `Content-Length`, `Last-Modified` and the content
//! type; this module adds `Content-Disposition` with the file name.
//!
//! Desktop mode does not need the route: files open straight from disk.
//! Ids of the absolute-path form (native dialogs) are refused here, so the
//! route never reads outside the configured roots.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path as RoutePath, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use muxmultitool_core::files::{Error, FileId, Location};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use tracing::warn;

use crate::AppState;

/// The download route, to merge into the app router.
pub(crate) fn routes() -> Router<Arc<AppState>> {
	Router::new().route("/files/{id}", get(download))
}

async fn download(
	State(state): State<Arc<AppState>>,
	RoutePath(id): RoutePath<String>,
	request: Request,
) -> Response {
	let id = FileId::new(id);
	let resolved = {
		let state = Arc::clone(&state);
		tokio::task::spawn_blocking(move || {
			// Only ids under a configured root; absolute paths are desktop-only
			if !matches!(id.decode()?, Location::InRoot { .. }) {
				return Err(Error::PathsNotAccepted);
			}
			let path = state.files.path(&id)?;
			// A folder has an id too, but nothing to download
			path.is_file().then_some(path).ok_or(Error::NotFound)
		})
		.await
	};
	let path = match resolved {
		Ok(Ok(path)) => path,
		Ok(Err(error)) => return error_response(&error),
		Err(error) => {
			warn!(%error, "download task failed");
			return StatusCode::INTERNAL_SERVER_ERROR.into_response();
		}
	};
	let disposition = content_disposition(&path);
	let mut response = match ServeFile::new(&path).oneshot(request).await {
		Ok(response) => response.into_response(),
		Err(error) => match error {},
	};
	if response.status().is_success()
		&& let Ok(value) = HeaderValue::from_str(&disposition)
	{
		response
			.headers_mut()
			.insert(header::CONTENT_DISPOSITION, value);
	}
	response
}

/// The status for a files error. The body never names a server path.
fn error_response(error: &Error) -> Response {
	let status = match error {
		Error::InvalidId(_) | Error::NotAbsolute | Error::NotAFolder => StatusCode::BAD_REQUEST,
		Error::OutsideRoots | Error::PathsNotAccepted => StatusCode::FORBIDDEN,
		Error::NotFound => StatusCode::NOT_FOUND,
		_ => StatusCode::INTERNAL_SERVER_ERROR,
	};
	(status, error.to_string()).into_response()
}

/// Everything RFC 5987 does not let through as is: the `attr-char` set is
/// alphanumerics plus `! # $ & + - . ^ _ | ~` and the backtick; the rest is
/// percent-encoded.
const NOT_ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
	.remove(b'!')
	.remove(b'#')
	.remove(b'$')
	.remove(b'&')
	.remove(b'+')
	.remove(b'-')
	.remove(b'.')
	.remove(b'^')
	.remove(b'_')
	.remove(b'`')
	.remove(b'|')
	.remove(b'~');

/// `attachment` with the file name: an ASCII fallback plus the RFC 5987
/// `filename*` for the real name.
fn content_disposition(path: &Path) -> String {
	let name = path
		.file_name()
		.map_or_else(|| "download".into(), |n| n.to_string_lossy());
	let fallback: String = name
		.chars()
		.map(|c| {
			if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' && c != '%' {
				c
			} else {
				'_'
			}
		})
		.collect();
	let encoded = utf8_percent_encode(&name, NOT_ATTR_CHAR);
	format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn disposition_escapes_the_name() {
		let value = content_disposition(Path::new("/x/na\u{ef}ve \"a\".gif"));
		assert_eq!(
			value,
			"attachment; filename=\"na_ve _a_.gif\"; filename*=UTF-8''na%C3%AFve%20%22a%22.gif"
		);
		// attr-char stays, the rest is encoded: `*` and `'` would end the value
		let value = content_disposition(Path::new("a!#$&+-.^_`|~b*'c;d.mp4"));
		assert!(
			value.ends_with("filename*=UTF-8''a!#$&+-.^_`|~b%2A%27c%3Bd.mp4"),
			"{value}"
		);
	}
}
