#![allow(clippy::expect_fun_call)]
use std::io;

use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    routing::{get, post},
    Router,
};
use futures::TryStreamExt;
use tokio::{fs::File, io::BufWriter, net::TcpListener};
use tokio_util::io::StreamReader;
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    uploads_directory: String,
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

    let app = Router::new()
        .route("/", get(|| async { "Ok" }))
        .route(
            "/*file_path",
            post(upload).get_service(ServeDir::new(&uploads_directory)),
        )
        .with_state(AppState { uploads_directory });

    let listener = TcpListener::bind(&address)
        .await
        .expect(&format!("failed to listen on {address}"));
    tracing::info!("file server started on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap()
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
        let body_with_io_error = stream.map_err(|err| io::Error::other(err);
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

fn path_is_valid(path: &str) -> bool {
    let path = std::path::Path::new(path);
    let mut components = path.components().peekable();

    components.all(|component| matches!(component, std::path::Component::Normal(_)))
}
