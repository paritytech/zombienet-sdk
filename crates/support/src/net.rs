use std::{io::Cursor, str::FromStr, time::Duration};

use reqwest::{Method, Request, StatusCode, Url};
use tracing::trace;

use crate::constants::THIS_IS_A_BUG;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub async fn download_file(url: String, dest: String) -> Result<()> {
    let response = reqwest::get(url).await?;
    let mut file = std::fs::File::create(dest)?;
    let mut content = Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut file)?;
    Ok(())
}

pub async fn wait_ws_ready(url: &str) -> Result<()> {
    let mut parsed = Url::from_str(url)?;
    parsed
        .set_scheme("http")
        .map_err(|_| anyhow::anyhow!("Can not set the scheme, {}", THIS_IS_A_BUG))?;

    let http_client = reqwest::Client::new();
    loop {
        let req = Request::new(Method::OPTIONS, parsed.clone());
        let res = http_client.execute(req).await;
        match res {
            Ok(res) => {
                if res.status() == StatusCode::OK {
                    // ready to go!
                    break;
                }

                trace!("http_client status: {}, continuing...", res.status());
            },
            Err(e) => {
                if !skip_err_while_waiting(&e) {
                    return Err(e.into());
                }

                trace!("http_client err: {}, continuing... ", e.to_string());
            },
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}

pub fn skip_err_while_waiting(e: &reqwest::Error) -> bool {
    // if the error is connecting/request could be the case that the node
    // is not listening yet, so we keep waiting
    // Skipped errs like:
    // 'tcp connect error: Connection refused (os error 61)'
    // 'operation was canceled: connection closed before message completed'
    // 'connection error: Connection reset by peer (os error 54)'
    e.is_connect() || e.is_request()
}
