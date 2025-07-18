use subxt::{backend::rpc::RpcClient, OnlineClient};

#[async_trait::async_trait]
pub trait ClientFromUrl: Sized {
    async fn from_secure_url(url: &str) -> Result<Self, subxt::Error>;
    async fn from_insecure_url(url: &str) -> Result<Self, subxt::Error>;
}

#[async_trait::async_trait]
impl<Config: subxt::Config + Send + Sync> ClientFromUrl for OnlineClient<Config> {
    async fn from_secure_url(url: &str) -> Result<Self, subxt::Error> {
        Self::from_url(url).await
    }

    async fn from_insecure_url(url: &str) -> Result<Self, subxt::Error> {
        Self::from_insecure_url(url).await
    }
}

#[async_trait::async_trait]
impl ClientFromUrl for RpcClient {
    async fn from_secure_url(url: &str) -> Result<Self, subxt::Error> {
        Self::from_url(url).await.map_err(subxt::Error::from)
    }

    async fn from_insecure_url(url: &str) -> Result<Self, subxt::Error> {
        Self::from_insecure_url(url)
            .await
            .map_err(subxt::Error::from)
    }
}

pub async fn get_client_from_url<T: ClientFromUrl + Send>(url: &str) -> Result<T, subxt::Error> {
    if subxt::utils::url_is_secure(url)? {
        T::from_secure_url(url).await
    } else {
        T::from_insecure_url(url).await
    }
}
