#![deny(warnings)]
#![deny(future_incompatible)]
#![deny(nonstandard_style)]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![deny(rust_2021_compatibility)]
#![deny(unused)]

use std::{fs::{read_dir, File}, collections::HashMap, io::{Read, Write}, path::{Path, PathBuf}};
use async_std::{fs::create_dir_all, net::TcpStream};
use chrono::{Utc, TimeZone, DateTime};
use http_types::{Method, Request, Url};
use resol_vbus::{Language, Specification, SpecificationFile, RecordingReader};
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
impl IntoError for chrono::ParseError {}
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

                    sync_for_datecode(host, datecode).await?;
                }
            }
        }
    }

    convert(host, spec)?;

    Ok(())
}

async fn sync_for_datecode(host: &str, datecode: &str) -> Result<()> {
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

    // debug!(?needs_download);

    if needs_download {
        let url = format!("http://{}/log/{}_packets.vbus", host, datecode);
        let url = Url::parse(&url)?;

        let req = Request::new(Method::Get, url);
        let mut res = async_h1::connect(stream.clone(), req).await?;

        if !res.status().is_success() {
            return Err(format!("Unable to download log file dated {}", datecode).into());
        }

        let body = res.body_bytes().await?;

        async_std::fs::write(&vbus_filename, &body).await?;
    } else {
        debug!("Skipping download for file dated {}", datecode);
    };

    Ok(())
}

fn parse_datecode<Tz: TimeZone>(datecode_str: &str, tz: &Tz) -> Result<DateTime<Tz>> {
    let datecode = datecode_str.parse::<u32>()?;
    let year = (datecode / 10000) as i32;
    let month = (datecode / 100) % 100;
    let day = datecode % 100;
    let dt = tz.ymd(year, month, day).and_hms(0, 0, 0);
    Ok(dt)
}

fn convert(host: &str, spec: &Specification) -> Result<()> {
    let mut all_vbus_filenames = Vec::new();
    let mut vbus_file_modified_by_rel_filename = HashMap::new();
    let mut csv_file_modified_by_rel_filename = HashMap::new();

    for entry in read_dir(host)? {
        let entry = entry?;

        if !entry.file_type()?.is_file() {
            // nop
        } else {
            let filename = entry.file_name().to_string_lossy().to_string();
            if !filename [0..8].chars().all(|c| char::is_digit(c, 10)) {
                // nop
            } else if (filename.len() == 13) && filename.ends_with(".vbus") {
                all_vbus_filenames.push(filename.clone());
                vbus_file_modified_by_rel_filename.insert(filename, entry.metadata()?.modified()?);
            } else if (filename.len() == 12) && filename.ends_with(".csv") {
                csv_file_modified_by_rel_filename.insert(filename, entry.metadata()?.modified()?);
            }
        }
    }

    all_vbus_filenames.sort();

    let tz = chrono_tz::Europe::Berlin;

    let mut local_to_utc_datecodes = HashMap::new();

    for vbus_filename in &all_vbus_filenames {
        let datecode_str_utc = vbus_filename [0..8].to_string();

        let start_of_day_utc = parse_datecode(&datecode_str_utc, &Utc)?;
        let end_of_day_utc = start_of_day_utc.date().and_hms(23, 59, 59);

        let start_of_day_local = start_of_day_utc.with_timezone(&tz);
        let end_of_day_local = end_of_day_utc.with_timezone(&tz);

        let start_of_day_local_datecode = start_of_day_local.format("%Y%m%d").to_string();
        let end_of_day_local_datecode = end_of_day_local.format("%Y%m%d").to_string();

        if !local_to_utc_datecodes.contains_key(&start_of_day_local_datecode) {
            local_to_utc_datecodes.insert(start_of_day_local_datecode.clone(), Vec::new());
        }
        local_to_utc_datecodes.get_mut(&start_of_day_local_datecode).unwrap().push(datecode_str_utc.clone());

        if !local_to_utc_datecodes.contains_key(&end_of_day_local_datecode) {
            local_to_utc_datecodes.insert(end_of_day_local_datecode.clone(), Vec::new());
        }
        local_to_utc_datecodes.get_mut(&end_of_day_local_datecode).unwrap().push(datecode_str_utc.clone());
    }

    for (csv_datecode, mut vbus_datecodes) in local_to_utc_datecodes {
        let rel_csv_filename = format!("{}.csv", &csv_datecode);
        let csv_filename = format!("{}/{}", host, &rel_csv_filename);
        let csv_filename = Path::new(&csv_filename);

        vbus_datecodes.sort();

        let csv_modified = csv_file_modified_by_rel_filename.get(&rel_csv_filename);

        let mut vbus_filenames = Vec::new();
        let mut needs_conversion = csv_modified.is_none();
        for vbus_datecode in &vbus_datecodes {
            let rel_vbus_filename = format!("{}.vbus", &vbus_datecode);
            if let Some(vbus_modified) = vbus_file_modified_by_rel_filename.get(&rel_vbus_filename) {
                let vbus_filename = format!("{}/{}", host, rel_vbus_filename);
                let vbus_filename = PathBuf::from(&vbus_filename);

                vbus_filenames.push(vbus_filename);

                if !needs_conversion {
                    if *vbus_modified > *csv_modified.unwrap() {
                        needs_conversion = true;
                    }
                }
            }
        }

        if needs_conversion {
            debug!("Converting {:?} into {:?}...", &vbus_filenames, &csv_filename);

            let start_of_day_local = parse_datecode(&csv_datecode, &tz)?;
            let end_of_day_local = start_of_day_local.date().and_hms(23, 59, 59);

            let start_of_day_utc = start_of_day_local.with_timezone(&Utc);
            let end_of_day_utc = end_of_day_local.with_timezone(&Utc);

            let mut vbus_bytes = Vec::new();
            for vbus_filename in &vbus_filenames {
                let mut vbus_file = File::open(vbus_filename)?;
                vbus_file.read_to_end(&mut vbus_bytes)?;
            }

            let mut rr = RecordingReader::new(vbus_bytes.as_slice());
            rr.set_min_max_timestamps(Some(start_of_day_utc.clone()), Some(end_of_day_utc.clone()));

            let topo_data_set = rr.read_topology_data_set()?;

            let mut output_buffer = Vec::new();
            let output = &mut output_buffer;

            write!(output, "Datum")?;

            for field in spec.fields_in_data_set(&topo_data_set) {
                let name = &field.field_spec().name;
                let unit_text = field.field_spec().unit_text.trim();
                if unit_text.len() > 0 {
                    write!(output, "\t{} [{}]", name, unit_text)?;
                } else {
                    write!(output, "\t{}", name)?;
                }
            }

            write!(output, "\n")?;

            let mut rr = RecordingReader::new(vbus_bytes.as_slice());
            rr.set_min_max_timestamps(Some(start_of_day_utc), Some(end_of_day_utc));

            let mut contains_data_lines = false;
            while let Some(rr_data_set) = rr.read_data_set()? {
                let mut data_set = topo_data_set.clone();
                data_set.timestamp = rr_data_set.timestamp;
                data_set.add_data_set(rr_data_set);

                let local_now = data_set.timestamp.with_timezone(&tz);

                write!(output, "{}", local_now.format("%d.%m.%Y %H:%M:%S"))?;

                for field in spec.fields_in_data_set(&data_set) {
                    write!(output, "\t{}", field.fmt_raw_value(false))?;
                }

                write!(output, "\n")?;

                contains_data_lines = true;
            }

            if contains_data_lines {
                std::fs::write(csv_filename, output_buffer)?;
            } else {
                debug!("    Skipping because CSV would be empty");
            }
        }
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
