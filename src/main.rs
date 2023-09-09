use std::time::Duration;

use crates_io_api::{AsyncClient, Crate, CratesQuery};
use eyre::{Report, WrapErr};
use tokio_stream::StreamExt;

static USER_AGENT: &str = "squatter-swatter 0.1.0; dx2718@gmail.com";
static RATE_LIMIT: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug)]
struct CrateInfo {
    name: String,
    version: String,
}

impl From<Crate> for CrateInfo {
    fn from(c: Crate) -> Self {
        CrateInfo {
            name: c.name,
            version: c.max_version,
        }
    }
}

async fn get_user_crate_list(client: &Client, uid: usize) -> eyre::Result<Vec<CrateInfo>> {
    let q = CratesQuery::builder()
        .user_id(uid)
        .build();
    let mut resp = client.crates_stream(q);
    let mut v: Vec<CrateInfo> = Vec::new();

    while let Some(c) = resp.next().await {
        if let Ok(c) = c {
            v.push(c.into());
        }
    }

    Ok(v)
}

async fn get_crate_loc(info: &CrateInfo) -> eyre::Result<usize> {
    Ok(0)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> eyre::Result<()> {
    let client = AsyncClient::new(USER_AGENT, RATE_LIMIT)?;
    let q = CratesQuery::builder()
        .user_id(3618)
        .build();
    let mut resp = client.crates_stream(q);

    while let Some(c) = resp.next().await {
        println!("{:#?}", &c);
    }

    Ok(())
}
