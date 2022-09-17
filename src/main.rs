use std::{
    error,
    fmt::Display,
    fs::{self, File},
    io::{self, stdout, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
};

use human_bytes::human_bytes;
use inquire::{CustomType, MultiSelect};
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use reqwest::StatusCode;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub meta: Meta,
    pub data: Vec<Data>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Data {
    pub id: i64,
    pub channel_name: Channel,
    pub url: String,
    pub extension: Extension,
    pub width: i64,
    pub height: i64,
    pub filesize: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
    pub total: i64,
    pub offset: i64,
    pub count: i64,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, EnumIter)]
pub enum Channel {
    #[serde(rename = "media")]
    Media,
    #[serde(rename = "nsfw-general")]
    NsfwGeneral,
    #[serde(rename = "furry")]
    Furry,
    #[serde(rename = "futa")]
    Futa,
    #[serde(rename = "yaoi")]
    Yaoi,
    #[serde(rename = "yuri")]
    Yuri,
    #[serde(rename = "traps")]
    Traps,
    #[serde(rename = "irl-3d")]
    Irl3D,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Extension {
    #[serde(rename = "jpg")]
    Jpg,
    #[serde(rename = "jpeg")]
    Jpeg,
    #[serde(rename = "png")]
    Png,
    #[serde(rename = "webp")]
    Webp,
    #[serde(rename = "gif")]
    Gif,
}

impl Display for Extension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Extension::Jpg => write!(f, "jpg"),
            Extension::Jpeg => write!(f, "jpeg"),
            Extension::Png => write!(f, "png"),
            Extension::Webp => write!(f, "webp"),
            Extension::Gif => write!(f, "gif"),
        }
    }
}

impl Display for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ID: {}, Size: {}, Resolution: [{}x{}]",
            &self.id,
            human_bytes(self.filesize as f64),
            &self.width,
            &self.height
        )
    }
}

impl Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Channel::Media => write!(f, "media"),
            Channel::NsfwGeneral => write!(f, "nsfw-general"),
            Channel::Furry => write!(f, "furry"),
            Channel::Futa => write!(f, "futa"),
            Channel::Yaoi => write!(f, "yaoi"),
            Channel::Yuri => write!(f, "yuri"),
            Channel::Traps => write!(f, "traps"),
            Channel::Irl3D => write!(f, "irl-3d"),
        }
    }
}

struct Settings {
    max_pages: u64,
    path: PathBuf,
    channels: Vec<Channel>,
}

const BASE_URL: &str = "https://community-uploads.highwinds-cdn.com/api/v9/community_uploads";

fn main() -> Result<(), Box<dyn error::Error>> {
    let installed = Arc::new(AtomicU64::new(0));
    let errored = Arc::new(AtomicU64::new(0));

    let max_pages: u64 = CustomType::new("Max pages: ").prompt().unwrap_or_default();
    let path = FileDialog::new()
        .pick_folder()
        .expect("Exiting because operation was canceled.");
    let channels =
        MultiSelect::new("Select image channels:", Channel::iter().collect()).prompt()?;

    let settings = Settings {
        max_pages,
        path,
        channels,
    };

    let mut query: Vec<(String, String)> = Vec::new();
    for channel in settings.channels {
        fs::create_dir_all(&settings.path.join(channel.to_string()));
        query.push(("channel_name__in[]".to_owned(), channel.to_string()));
    }

    let data = (0..settings.max_pages)
        .into_par_iter()
        .map(|e| {
            reqwest::blocking::Client::new()
                .get(BASE_URL)
                .query(&query)
                .query(&[("__offset", e * 96)])
                .send()
                .ok()
                .and_then(|e| e.json().ok())
                .map(|e: Response| e.data)
                .unwrap_or_default()
        })
        .flatten()
        .collect::<Vec<_>>();

    let max_count = settings.max_pages * 96;
    let thread = {
        let installed = installed.clone();
        thread::spawn(move || {
            let installed = installed.clone();
            let mut stdout = stdout();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let installed = installed.load(Ordering::Acquire);
                print!("\rDownloading {}/{}", installed, max_count);
                let _ = stdout.flush();
                if installed == max_count {
                    break;
                }
            }
        })
    };

    let _ = data
        .into_par_iter()
        .map(|ele| {
            let file_name = format!("{}.{}", ele.id, ele.extension);
            let path = settings
                .path
                .join(ele.channel_name.to_string())
                .join(file_name);

            installed.fetch_add(1, Ordering::SeqCst);
            if !path.exists() {
                if let Ok(mut response) = reqwest::blocking::get(&ele.url) {
                    if response.status() == StatusCode::OK {
                        if let Ok(mut file) = File::create(&path) {
                            let _ = io::copy(&mut response, &mut file);
                            drop(file);
                        } else {
                            errored.fetch_add(1, Ordering::SeqCst);
                        }
                    } else {
                        errored.fetch_add(1, Ordering::SeqCst);
                    }
                } else {
                    errored.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
        .collect::<Vec<_>>();

    let _ = thread.join();
    println!(
        "\nFinished. Downloaded {}/{}",
        (installed.load(Ordering::Acquire) - errored.load(Ordering::Acquire)),
        max_count
    );
    Ok(())
}
