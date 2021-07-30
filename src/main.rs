use std::io::Write;
use async_std::{fs::create_dir_all, net::TcpStream};
use http_types::{Method, Request, Url};
use resol_vbus::{chrono::Duration, Language, RecordingReader, Specification, SpecificationFile};
use tracing::debug;
use tracing_subscriber::EnvFilter;

#[derive(Debug)]
struct Error(String);

impl From<String> for Error {
    fn from(other: String) -> Error {
        Error(other)
    }
}

impl From<&str> for Error {
    fn from(other: &str) -> Error {
        Error(other.to_string())
    }
}

trait IntoError: std::fmt::Debug {}

impl<T: IntoError> From<T> for Error {
    fn from(other: T) -> Error {
        Error(format!("{:?}", other))
    }
}

type Result<T> = std::result::Result<T, Error>;

impl IntoError for std::io::Error {}
impl IntoError for std::num::ParseIntError {}
impl IntoError for color_eyre::Report {}
impl IntoError for http_types::Error {}
impl IntoError for http_types::url::ParseError {}
impl IntoError for resol_vbus::Error {}

#[async_std::main]
async fn main() -> Result<()> {
    setup_debugging()?;

    let spec_file_bytes = include_bytes!("../vbus_specification.vsf");
    let spec_file = SpecificationFile::from_bytes(spec_file_bytes)?;
    let spec = Specification::from_file(spec_file, Language::De);

    for arg in std::env::args().skip(1) {
        sync_and_convert(&arg, &spec).await?;
    }

    Ok(())
}

async fn sync_and_convert(host: &str, spec: &Specification) -> Result<()> {
    debug!("Downloading log file index for {:?}", host);

    let addr = format!("{}:80", host);
    let stream = TcpStream::connect(&addr).await?;

    let url = format!("http://{}/log/", host);
    let url = Url::parse(&url)?;

    let req = Request::new(Method::Get, url);
    let mut res = async_h1::connect(stream.clone(), req).await?;

    if !res.status().is_success() {
        return Err("Unable to download log directory index".into());
    }

    let body = res.body_string().await?;

    // debug!(%body);

    create_dir_all(host).await?;

    for (idx, _) in body.match_indices("<a href=") {
        let start_idx = if &body [idx + 8..idx + 14] == "'/log/" {
            Some(idx + 14)
        } else if &body [idx + 8..idx + 9] == "\"" {
            Some(idx + 9)
        } else {
            None
        };

        if let Some(start_idx) = start_idx {
            let mid_idx = start_idx + 8;
            let end_idx = start_idx + 21;

            if end_idx <= body.len() {
                let suffix = &body [mid_idx..end_idx];
                if suffix == "_packets.vbus" {
                    let datecode = &body [start_idx..mid_idx];

                    sync_and_convert_for_datecode(host, spec, datecode).await?;
                }
            }
        }
    }

    Ok(())
}

async fn sync_and_convert_for_datecode(host: &str, spec: &Specification, datecode: &str) -> Result<()> {
    debug!("Fetching information about log file dated {}", datecode);

    let vbus_filename = format!("{}/{}.vbus", host, datecode);

    let addr = format!("{}:80", host);
    let stream = TcpStream::connect(&addr).await?;

    let url = format!("http://{}/log/{}_packets.vbus", host, datecode);
    let url = Url::parse(&url)?;

    let req = Request::new(Method::Head, url);
    let res = async_h1::connect(stream.clone(), req).await?;

    if !res.status().is_success() {
        return Err(format!("Unable to download log file dated {}", datecode).into());
    }

    // debug!(?res);

    let content_length = if let Some(content_length) = res.header("content-length") {
        content_length.as_str().parse::<u64>()?
    } else {
        return Err(format!("Unable to determine file size dated {}", datecode).into());
    };

    // debug!(?content_length);

    let file_size = if let Ok(metadata) = std::fs::metadata(&vbus_filename) {
        metadata.len()
    } else {
        0
    };

    let needs_download = file_size != content_length;

    debug!(?needs_download);

    let body = if needs_download {
        let url = format!("http://{}/log/{}_packets.vbus", host, datecode);
        let url = Url::parse(&url)?;

        let req = Request::new(Method::Get, url);
        let mut res = async_h1::connect(stream.clone(), req).await?;

        if !res.status().is_success() {
            return Err(format!("Unable to download log file dated {}", datecode).into());
        }

        let body = res.body_bytes().await?;

        async_std::fs::write(&vbus_filename, &body).await?;

        body
    } else {
        debug!("Skipping download for file dated {}", datecode);

        let vbus_bytes = async_std::fs::read(&vbus_filename).await?;

        vbus_bytes
    };

    let csv_filename = format!("{}/{}.csv", host, datecode);

    let needs_conversion = needs_download || std::fs::metadata(&csv_filename).is_err();

    debug!(?needs_conversion);

    if needs_conversion {
        let mut rr = RecordingReader::new(body.as_slice());

        let topology_data_set = rr.read_topology_data_set()?;

        let mut rr = RecordingReader::new(body.as_slice());

        let mut out = Vec::new();

        write!(out, "Timestamp")?;
        for field in spec.fields_in_data_set(&topology_data_set) {
            write!(out, "\t{}", field.field_spec().name)?;
        }
        writeln!(out, "")?;

        let mut cumultative_data_set = topology_data_set.clone();
        cumultative_data_set.clear_all_packets();

        let duration = Duration::minutes(15);

        while let Some(data_set) = rr.read_data_set()? {
            let timestamp = data_set.timestamp.clone();
            let min_timestamp = timestamp.clone() - duration;
            cumultative_data_set.clear_packets_older_than(min_timestamp);

            cumultative_data_set.add_data_set(data_set);

            write!(out, "{}", timestamp.to_rfc3339())?;
            for field in spec.fields_in_data_set(&cumultative_data_set) {
                write!(out, "\t{}", field.fmt_raw_value(false))?;
            }
            writeln!(out, "")?;
        }

        std::fs::write(&csv_filename, &out)?;
    } else {
        debug!("Skipping conversion for file dated {}", datecode);
    }

    Ok(())
}

fn setup_debugging() -> Result<()> {
    if std::env::var("RUST_BACKTRACE").is_err() {
        std::env::set_var("RUST_BACKTRACE", "1")
    }
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info")
    }
    tracing_subscriber::fmt::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    Ok(())
}
