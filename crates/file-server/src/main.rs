#![allow(clippy::expect_fun_call)]
use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    routing::{get, post},
    Router,
};
use futures::TryStreamExt;
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
    net::TcpListener,
};
use tokio_util::io::StreamReader;
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    uploads_directory: String,
    /// Fetches started by `PUT` that haven't completed, or that failed and
    /// haven't been reported yet. A completed fetch is just the file.
    fetches: Arc<Mutex<HashMap<PathBuf, Fetch>>>,
    client: reqwest::Client,
}

enum Fetch {
    /// Bytes written so far.
    Running(Arc<AtomicU64>),
    Failed(String),
}

#[tokio::main]
async fn main() {
    let address =
        std::env::var("LISTENING_ADDRESS").expect("LISTENING_ADDRESS env variable isn't defined");
    let uploads_directory =
        std::env::var("UPLOADS_DIRECTORY").expect("UPLOADS_DIRECTORY env variable isn't defined");

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();

    tokio::fs::create_dir_all(&uploads_directory)
        .await
        .expect(&format!("failed to create '{uploads_directory}' directory"));

    let listener = TcpListener::bind(&address)
        .await
        .expect(&format!("failed to listen on {address}"));
    tracing::info!("file server started on {}", listener.local_addr().unwrap());
    axum::serve(listener, app(uploads_directory)).await.unwrap()
}

fn app(uploads_directory: String) -> Router {
    Router::new()
        .route("/", get(|| async { "Ok" }))
        .route(
            "/*file_path",
            post(upload)
                .put(fetch)
                .get_service(ServeDir::new(&uploads_directory)),
        )
        .with_state(AppState {
            uploads_directory,
            fetches: Arc::default(),
            client: reqwest::Client::new(),
        })
}

async fn upload(
    Path(file_path): Path<String>,
    State(state): State<AppState>,
    request: Request,
) -> Result<(), (StatusCode, String)> {
    if !path_is_valid(&file_path) {
        return Err((StatusCode::BAD_REQUEST, "Invalid path".to_owned()));
    }

    async {
        let path = std::path::Path::new(&state.uploads_directory).join(file_path);

        if let Some(parent_dir) = path.parent() {
            tokio::fs::create_dir_all(parent_dir).await?;
        }

        let stream = request.into_body().into_data_stream();
        let body_with_io_error = stream.map_err(io::Error::other);
        let body_reader = StreamReader::new(body_with_io_error);
        futures::pin_mut!(body_reader);

        let mut file = BufWriter::new(File::create(&path).await?);
        tokio::io::copy(&mut body_reader, &mut file).await?;

        tracing::info!("created file '{}'", path.to_string_lossy());

        Ok::<_, io::Error>(())
    }
    .await
    .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

/// Fetch the URL in the request body into `file_path`, so a large file never
/// has to pass through whoever runs zombienet.
///
/// Idempotent, so the same request both starts the fetch and polls it:
/// - `200` once the file is there, also straight away if it already was;
/// - `202` while the fetch runs, with the bytes written so far;
/// - `502` once, if the fetch failed (the next `PUT` starts it again).
///
/// The file only appears, under its name, when it is complete: a fetch writes
/// to `<file_path>.part` and renames it at the end.
async fn fetch(
    Path(file_path): Path<String>,
    State(state): State<AppState>,
    body: String,
) -> (StatusCode, String) {
    if !path_is_valid(&file_path) {
        return (StatusCode::BAD_REQUEST, "Invalid path".to_owned());
    }
    let url = match reqwest::Url::parse(body.trim()) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => return (StatusCode::BAD_REQUEST, "Invalid URL".to_owned()),
    };
    let path = std::path::Path::new(&state.uploads_directory).join(&file_path);

    let mut fetches = state.fetches.lock().expect("fetches lock poisoned");
    // Checked before the map: a fetch renames its file into place before it
    // leaves the map, so a completed one is never mistaken for a missing one.
    if path.exists() {
        return (StatusCode::OK, "done".to_owned());
    }
    match fetches.get(&path) {
        Some(Fetch::Running(written)) => (
            StatusCode::ACCEPTED,
            format!("{} bytes", written.load(Ordering::Relaxed)),
        ),
        Some(Fetch::Failed(_)) => {
            let Some(Fetch::Failed(message)) = fetches.remove(&path) else {
                unreachable!("just matched")
            };
            (StatusCode::BAD_GATEWAY, message)
        },
        None => {
            let written = Arc::new(AtomicU64::new(0));
            fetches.insert(path.clone(), Fetch::Running(written.clone()));
            tracing::info!("fetching '{}' into '{file_path}'", shown(&url));
            tokio::spawn(fetch_in_background(state.clone(), url, path, written));
            (StatusCode::ACCEPTED, "0 bytes".to_owned())
        },
    }
}

async fn fetch_in_background(
    state: AppState,
    url: reqwest::Url,
    path: PathBuf,
    written: Arc<AtomicU64>,
) {
    let part = PathBuf::from(format!("{}.part", path.display()));
    let outcome = download(&state.client, &url, &part, &path, &written).await;

    let mut fetches = state.fetches.lock().expect("fetches lock poisoned");
    match outcome {
        Ok(()) => {
            tracing::info!("fetched '{}' into '{}'", shown(&url), path.display());
            fetches.remove(&path);
        },
        Err(message) => {
            tracing::warn!("fetching '{}' failed: {message}", shown(&url));
            let _ = std::fs::remove_file(&part);
            fetches.insert(path, Fetch::Failed(message));
        },
    }
}

async fn download(
    client: &reqwest::Client,
    url: &reqwest::Url,
    part: &std::path::Path,
    path: &std::path::Path,
    written: &AtomicU64,
) -> Result<(), String> {
    // `without_url`: the URL may be signed, and its query is the signature.
    let fail = |err: reqwest::Error| err.without_url().to_string();

    let res = client
        .get(url.clone())
        .send()
        .await
        .and_then(|res| res.error_for_status())
        .map_err(fail)?;

    if let Some(parent_dir) = part.parent() {
        tokio::fs::create_dir_all(parent_dir)
            .await
            .map_err(|err| err.to_string())?;
    }
    let mut file = BufWriter::new(File::create(part).await.map_err(|err| err.to_string())?);
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.try_next().await.map_err(fail)? {
        file.write_all(&chunk)
            .await
            .map_err(|err| err.to_string())?;
        written.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    file.flush().await.map_err(|err| err.to_string())?;
    drop(file);

    tokio::fs::rename(part, path)
        .await
        .map_err(|err| err.to_string())
}

/// `url` without its query or fragment, for logs.
fn shown(url: &reqwest::Url) -> String {
    let mut shown = url.clone();
    shown.set_query(None);
    shown.set_fragment(None);
    shown.to_string()
}

fn path_is_valid(path: &str) -> bool {
    let path = std::path::Path::new(path);
    let mut components = path.components().peekable();

    components.all(|component| matches!(component, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use tower::ServiceExt;

    use super::*;

    /// A server publishing `/snap.tgz`, and `/missing` answering 404.
    async fn source() -> String {
        let app = Router::new().route(
            "/snap.tgz",
            get(|| async { vec![7u8; 3 * 1024 * 1024 + 5] }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    async fn put(app: &Router, path: &str, url: &str) -> (StatusCode, String) {
        let res = app
            .clone()
            .oneshot(Request::put(path).body(Body::from(url.to_owned())).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// `PUT` until the fetch is no longer running.
    async fn settle(app: &Router, path: &str, url: &str) -> (StatusCode, String) {
        for _ in 0..500 {
            let answer = put(app, path, url).await;
            if answer.0 != StatusCode::ACCEPTED {
                return answer;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the fetch never settled");
    }

    async fn get_bytes(app: &Router, path: &str) -> (StatusCode, Vec<u8>) {
        let res = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, body.to_vec())
    }

    #[tokio::test]
    async fn a_put_fetches_the_url_and_then_serves_it() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path().to_string_lossy().into_owned());
        let url = format!("{}/snap.tgz?sig=secret&x=1", source().await);

        let (status, body) = put(&app, "/abc.tgz", &url).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(settle(&app, "/abc.tgz", &url).await.0, StatusCode::OK);

        let (status, bytes) = get_bytes(&app, "/abc.tgz").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, vec![7u8; 3 * 1024 * 1024 + 5]);
        assert!(!dir.path().join("abc.tgz.part").exists());
    }

    #[tokio::test]
    async fn a_file_already_there_is_not_fetched_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("abc.tgz"), b"cached").unwrap();
        let app = app(dir.path().to_string_lossy().into_owned());

        // Nothing listens there: a fetch would fail.
        let (status, _) = put(&app, "/abc.tgz", "http://127.0.0.1:9/snap.tgz").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(get_bytes(&app, "/abc.tgz").await.1, b"cached");
    }

    #[tokio::test]
    async fn a_failed_fetch_is_reported_once_without_the_signature_and_serves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path().to_string_lossy().into_owned());
        let url = format!("{}/missing?sig=secret", source().await);

        let (status, message) = settle(&app, "/abc.tgz", &url).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(message.contains("404"), "{message}");
        assert!(!message.contains("secret"), "{message}");
        assert_eq!(get_bytes(&app, "/abc.tgz").await.0, StatusCode::NOT_FOUND);

        // Reported once: the next PUT tries again.
        assert_eq!(put(&app, "/abc.tgz", &url).await.0, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn a_bad_path_or_url_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path().to_string_lossy().into_owned());

        assert_eq!(
            put(&app, "/../abc.tgz", "http://example.com/a").await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            put(&app, "/abc.tgz", "not a url").await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            put(&app, "/abc.tgz", "file:///etc/passwd").await.0,
            StatusCode::BAD_REQUEST
        );
    }
}
