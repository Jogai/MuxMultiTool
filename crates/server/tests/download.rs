//! `GET /files/{id}`: ids from `FilesService`, range requests, containment.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use axum::Router;
use axum::body::Body;
use http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use muxmultitool_core::files::Location;
use muxmultitool_server::{Config, app};
use tower::ServiceExt;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn router(root: &Path, desktop: bool) -> Router {
	app(&Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 0,
		media_roots: vec![root.to_path_buf()],
		desktop,
		..Config::default()
	})
}

fn id_of(relative: &str) -> String {
	Location::InRoot {
		root: 0,
		relative: relative.into(),
	}
	.id()
	.as_str()
	.to_owned()
}

async fn get(
	router: &Router,
	id: &str,
	range: Option<&str>,
) -> Result<(http::response::Parts, Vec<u8>)> {
	let mut request = Request::get(format!("/files/{id}")).header(header::HOST, "127.0.0.1");
	if let Some(range) = range {
		request = request.header(header::RANGE, range);
	}
	let response = router.clone().oneshot(request.body(Body::empty())?).await?;
	let (parts, body) = response.into_parts();
	Ok((parts, body.collect().await?.to_bytes().to_vec()))
}

fn setup() -> Result<(tempfile::TempDir, tempfile::TempDir)> {
	let root = tempfile::tempdir()?;
	let outside = tempfile::tempdir()?;
	std::fs::write(root.path().join("out put.gif"), b"0123456789")?;
	std::fs::write(outside.path().join("secret.txt"), b"secret")?;
	Ok((root, outside))
}

#[tokio::test]
async fn downloads_with_the_file_name() -> Result<()> {
	let (root, _outside) = setup()?;
	let router = router(root.path(), false);
	let (parts, body) = get(&router, &id_of("out put.gif"), None).await?;
	assert_eq!(parts.status, StatusCode::OK);
	assert_eq!(body, b"0123456789");
	assert_eq!(parts.headers[header::ACCEPT_RANGES], "bytes");
	let disposition = parts.headers[header::CONTENT_DISPOSITION].to_str()?;
	assert!(disposition.starts_with("attachment;"), "{disposition}");
	assert!(
		disposition.contains("filename=\"out put.gif\""),
		"{disposition}"
	);
	Ok(())
}

#[tokio::test]
async fn serves_ranges() -> Result<()> {
	let (root, _outside) = setup()?;
	let router = router(root.path(), false);
	let id = id_of("out put.gif");

	let (parts, body) = get(&router, &id, Some("bytes=2-5")).await?;
	assert_eq!(parts.status, StatusCode::PARTIAL_CONTENT);
	assert_eq!(body, b"2345");
	assert_eq!(parts.headers[header::CONTENT_RANGE], "bytes 2-5/10");
	assert!(parts.headers.contains_key(header::CONTENT_DISPOSITION));

	let (parts, body) = get(&router, &id, Some("bytes=7-")).await?;
	assert_eq!(parts.status, StatusCode::PARTIAL_CONTENT);
	assert_eq!(body, b"789");

	let (parts, _) = get(&router, &id, Some("bytes=50-60")).await?;
	assert_eq!(parts.status, StatusCode::RANGE_NOT_SATISFIABLE);
	Ok(())
}

#[tokio::test]
async fn refuses_files_outside_the_roots() -> Result<()> {
	let (root, outside) = setup()?;
	let router = router(root.path(), false);

	// A traversal in an id the server never made
	let (parts, _) = get(&router, &id_of("../x"), None).await?;
	assert!(parts.status.is_client_error(), "{}", parts.status);

	// An absolute path id
	let absolute = Location::Absolute(outside.path().join("secret.txt")).id();
	let (parts, body) = get(&router, absolute.as_str(), None).await?;
	assert_eq!(parts.status, StatusCode::FORBIDDEN);
	assert!(!String::from_utf8_lossy(&body).contains("secret"));

	// Also in desktop mode the route does not read outside the roots
	let desktop = self::router(root.path(), true);
	let (parts, _) = get(&desktop, absolute.as_str(), None).await?;
	assert_eq!(parts.status, StatusCode::FORBIDDEN);

	// Garbage
	let (parts, _) = get(&router, "not-an-id!", None).await?;
	assert_eq!(parts.status, StatusCode::BAD_REQUEST);
	Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_symlinks_leading_out() -> Result<()> {
	let (root, outside) = setup()?;
	std::os::unix::fs::symlink(
		outside.path().join("secret.txt"),
		root.path().join("link.txt"),
	)?;
	let router = router(root.path(), false);
	let (parts, _) = get(&router, &id_of("link.txt"), None).await?;
	assert_eq!(parts.status, StatusCode::FORBIDDEN);
	Ok(())
}

#[tokio::test]
async fn missing_files_and_folders_are_404() -> Result<()> {
	let (root, _outside) = setup()?;
	std::fs::create_dir(root.path().join("sub"))?;
	let router = router(root.path(), false);
	let (parts, _) = get(&router, &id_of("nope.gif"), None).await?;
	assert_eq!(parts.status, StatusCode::NOT_FOUND);
	let (parts, _) = get(&router, &id_of("sub"), None).await?;
	assert_eq!(parts.status, StatusCode::NOT_FOUND);
	Ok(())
}
