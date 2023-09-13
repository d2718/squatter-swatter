/*!
A Polite Client that spaces out its requrests in time.
*/
use std::time::Duration;

use eyre::{eyre, WrapErr};
use reqwest::{Client, ClientBuilder};
use tokio::time::{sleep, Sleep};
use tracing::{event, Level};
use url::Url;

pub struct PoliteClient {
    uri_base: Url,
    client: Client,
    interval: Duration,
    sleep: Option<Sleep>,
}

impl PoliteClient {
    pub async fn new(uri_base: &str, interval: Duration) -> eyre::Result<PoliteClient> {
        let uri_base: Url = uri_base.parse()?;
        let client = ClientBuilder::new()
            .user_agent(super::UA_HEADER.clone())
            .use_rustls_tls()
            .build()?;

        let pc = PoliteClient {
            uri_base, client, interval,
            sleep: None,
        };

        Ok(pc)
    }

    pub async fn get_body_with(&mut self, suffix: &[&str]) -> eyre::Result<Vec<u8>> {
        let uri = {
            let mut uri = self.uri_base.clone();
            uri.path_segments_mut()
                .map_err(|_| eyre!("base URL can't be relative: {}", &self.uri_base))?
                .extend(suffix);
            uri
        };
        event!(Level::INFO, "getting {}", &uri);

        if let Some(s) = self.sleep.take() {
            s.await;
        }
        self.sleep = Some(sleep(self.interval));

        let resp = self.client.get(uri).send().await.wrap_err("error sending request")?;
        let body_bytes: Vec<u8> = resp.bytes().await.wrap_err("unable to read body of request")?.into();

        Ok(body_bytes)
    }
}