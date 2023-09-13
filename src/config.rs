use std::path::{Path, PathBuf};

use eyre::WrapErr;
use serde::Deserialize;

static HEADERS: &str = "crate,version,uid,loc\n";

#[derive(Debug, Deserialize)]
pub struct Cfg {
    pub work_dir: PathBuf,
    pub user_file: PathBuf,
    pub output_file: PathBuf,
    pub untar_exec: PathBuf,
    pub tokei_exec: PathBuf,
}

impl Cfg {
    pub fn load<P: AsRef<Path>>(filename: P) -> eyre::Result<Cfg> {
        let p = filename.as_ref();
        let bytes = std::fs::read(p)
            .wrap_err_with(|| format!("unable to read config file: {}", p.display()))?;
        let cfg: Cfg =
            serde_json::from_slice(&bytes).wrap_err("unable to deserialize config bytes")?;

        ensure_output_file(&cfg.output_file)?;

        Ok(cfg)
    }
}

fn ensure_output_file(p: &Path) -> eyre::Result<()> {
    if let Ok(_) = std::fs::File::open(p) {
        return Ok(());
    }

    std::fs::write(p, HEADERS.as_bytes()).wrap_err("unable to create output file")?;
    tracing::event!(tracing::Level::INFO, "wrote headers to fresh output file {}", p.display());
    Ok(())
}
