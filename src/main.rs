mod config;
mod pc;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crates_io_api::{AsyncClient, Crate, CratesQuery};
use eyre::{bail, ensure, eyre, WrapErr};
use reqwest::header::HeaderValue;
use serde::Serialize;
use serde_json::Value;
use tokio_stream::StreamExt;
use tracing::{event, Level};
use url::Url;

use config::Cfg;
use pc::PoliteClient;

static USER_AGENT: &str = concat!(
    "squatter-spotter ",
    env!("CARGO_PKG_VERSION"),
    "; dx2718@gmail.com"
);
static RATE_LIMIT: Duration = Duration::from_millis(1000);
static API_BASE: &str = "https://crates.io/api/v1/crates";
static UA_HEADER: HeaderValue = HeaderValue::from_static(USER_AGENT);
static TOKEI_ARGS: &[&str] = &["-C", "-t", "Rust", "-o", "json"];

#[derive(Clone, Debug)]
struct CrateId {
    name: String,
    version: String,
}

impl From<Crate> for CrateId {
    fn from(c: Crate) -> Self {
        CrateId {
            name: c.name,
            version: c.max_version,
        }
    }
}

async fn get_user_crate_list(client: &AsyncClient, uid: u64) -> eyre::Result<Vec<CrateId>> {
    let q = CratesQuery::builder().user_id(uid).build();
    let mut resp = client.crates_stream(q);
    let mut v: Vec<CrateId> = Vec::new();

    while let Some(c) = resp.next().await {
        if let Ok(c) = c {
            v.push(c.into());
        }
    }

    Ok(v)
}

fn clear_dir<P: AsRef<Path>>(dir_path: P) -> eyre::Result<()> {
    let p = dir_path.as_ref();

    for ent in std::fs::read_dir(p)
        .wrap_err_with(|| format!("unable to list directory to clear: {}", p.display()))?
    {
        let ent = ent.wrap_err("error reading directory entry to delete")?;
        let ent_type = ent.file_type().wrap_err_with(|| {
            format!(
                "unable to determine directory entry {} file type",
                ent.path().display()
            )
        })?;

        if ent_type.is_dir() {
            std::fs::remove_dir_all(ent.path())
                .wrap_err_with(|| format!("error removing directory {}", ent.path().display()))?;
        } else {
            std::fs::remove_file(ent.path())
                .wrap_err_with(|| format!("error removing file {}", ent.path().display()))?;
        }
    }

    Ok(())
}

async fn get_crate_loc(cfg: &Cfg, client: &mut PoliteClient, info: &CrateId) -> eyre::Result<u64> {
    let uri = {
        let mut uri = Url::parse(API_BASE).unwrap();
        uri.path_segments_mut().map_err(|_| eyre!("API_BASE not so base"))?
            .extend(&[info.name.as_str(), info.version.as_str(), "download"]);
        uri
    };
    event!(Level::INFO, "    downloading crate: {}", &uri);

    let body_bytes = client.get_body_with(&[
        info.name.as_str(),
        info.version.as_str(),
        "download",
    ]).await.wrap_err("error downloading crate")?;

    let crate_tar = (&cfg.work_dir).join("download");
    std::fs::write(&crate_tar, &body_bytes).wrap_err("unable to write crate file")?;
    let status = Command::new(&cfg.untar_exec)
        .args(&["-xf".as_ref(), crate_tar.as_os_str(), "-C".as_ref(), cfg.work_dir.as_os_str()])
        .status()
        .wrap_err("unable to untar download")?;
    if !status.success() {
        event!(Level::WARN, "untar command exited with status: {}", &status);
        clear_dir(&cfg.work_dir).wrap_err("unable to clear working dir")?;
        return Ok(0);
    };

    let tokei_output = Command::new(&cfg.tokei_exec)
        .args(TOKEI_ARGS)
        .arg(&cfg.work_dir)
        .output()
        .wrap_err("error getting tokei output")?;
    let status = tokei_output.status;
    ensure!(
        status.success(),
        format!("tokei exited with status: {}", &status)
    );
    let tokei_json: Value = serde_json::from_slice(&tokei_output.stdout)
        .wrap_err("unable to deserialize tokei output")?;
    let lines = tokei_json
        .pointer("/Rust/code")
        .map(|p| p.as_u64())
        .flatten()
        .ok_or(eyre!(
            "unable to extract lines of Rust code from tokei output"
        ))?;

    clear_dir(&cfg.work_dir).wrap_err("unable to clear working dir")?;

    Ok(lines)
}

#[derive(Debug, Serialize)]
struct CrateInfo {
    name: String,
    version: String,
    uid: u64,
    loc: u64,
}

async fn get_uid_crate_info(
    cfg: &Cfg,
    api_client: &AsyncClient,
    dl_client: &mut PoliteClient,
    uid: u64,
) -> eyre::Result<Vec<CrateInfo>> {
    let id_list = get_user_crate_list(api_client, uid).await?;
    let mut v: Vec<CrateInfo> = Vec::with_capacity(id_list.len());

    for id in id_list.into_iter() {
        let loc = match get_crate_loc(cfg, dl_client, &id).await {
            Ok(loc) => loc,
            Err(e) => {
                event!(Level::WARN, "unable to get crate info: {:#}", &e);
                continue;
            },
        };
        let info = CrateInfo {
            name: id.name,
            version: id.version,
            uid,
            loc,
        };
        v.push(info);
    }

    Ok(v)
}

fn write_crate_info_output(cfg: &Cfg, info: Vec<CrateInfo>) -> eyre::Result<()> {
    let f = std::fs::OpenOptions::new()
        .append(true)
        .open(&cfg.output_file)
        .wrap_err("unable to open output file for appending")?;

    let mut wtr = csv::WriterBuilder::new().has_headers(false).from_writer(f);

    for i in info.iter() {
        wtr.serialize(i)
            .wrap_err_with(|| format!("unable to serialize crate info to output file: {:?}", i))?;
    }
    wtr.flush().wrap_err("unable to flush output writer")?;

    Ok(())
}

fn start_logging() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> eyre::Result<()> {
    start_logging();
    
    let args: Vec<String> = std::env::args().collect();
    let cfg_file = args
        .get(1)
        .ok_or(eyre!("must supply config file on command line"))?;
    let cfg = Cfg::load(&cfg_file)?;

    let starting_user_id: u64 = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("0")
        .parse()
        .wrap_err("unable to parse starting UID argument")?;

    let mut rdr =
        csv::Reader::from_path(&cfg.user_file).wrap_err("unable to open user file for reading")?;
    let mut users = rdr.records();
    let mut cur_uid: u64 = 0;
    if cur_uid < starting_user_id {
        event!(Level::INFO, "skipping to UID {}", &starting_user_id);
    }
    while cur_uid < starting_user_id {
        if let Some(rec) = users.next() {
            let rec = rec.wrap_err("error reading from user file")?;
            let uid: u64 = rec
                .get(3)
                .ok_or_else(|| eyre!(format!("malformed user record: {:?}", &rec)))?
                .parse()
                .wrap_err("unable to parse user id as u64")?;
            cur_uid = uid;
        } else {
            bail!("users file exhausted before starting uid encountered");
        }
    }

    let api_client =
        AsyncClient::new(USER_AGENT, RATE_LIMIT).wrap_err("unable to build API client")?;
    let mut dl_client = PoliteClient::new(API_BASE, RATE_LIMIT).await
        .wrap_err("unable to build download client")?;
   
    loop {
        event!(Level::INFO, "fetching crate info for UID {}", cur_uid);

        let infoz = get_uid_crate_info(&cfg, &api_client, &mut dl_client, cur_uid)
            .await
            .wrap_err_with(|| format!("unable to get crate info for user id {}", cur_uid))?;
        write_crate_info_output(&cfg, infoz)?;

        if let Some(rec) = users.next() {
            let rec = rec.wrap_err("error reading from user file")?;
            let uid: u64 = rec
                .get(3)
                .ok_or_else(|| eyre!(format!("malformed user record: {:?}", &rec)))?
                .parse()
                .wrap_err("unable to parse user id as u64")?;
            cur_uid = uid;
        } else {
            break;
        }
    }

    Ok(())
}
