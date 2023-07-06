use std::io::Cursor;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub async fn download_file(url: impl Into<String>, dest: impl Into<String>) -> Result<()> {
    let response = reqwest::get(url.into()).await?;
    let mut file = std::fs::File::create(dest.into())?;
    let mut content = Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut file)?;
    Ok(())
}
