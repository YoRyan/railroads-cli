use std::path::Path;

use clap::Parser;
use ini::Ini;
use log::{debug, error};
use serde::Deserialize;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser, Debug)]
struct Args {
    /// Don't launch the game after applying the configuration
    #[arg(short, long, default_value_t = false)]
    no_launch: bool
}

#[derive(Deserialize, Debug)]
struct Config {
    #[serde(default = "String::new")]
    settings_ini_path: String,
    #[serde(default = "default_true")]
    best_graphics: bool,
    #[serde(default = "default_true")]
    skip_opening: bool,
    #[serde(default = "default_true")]
    enable_editor: bool,
    #[serde(default = "String::new")]
    railroads_exe_path: String,
    #[serde(default = "default_true")]
    laa_aware: bool,
    #[serde(default = "default_openspy")]
    gamespy_server: String,
    #[serde(default = "default_true")]
    warn_on_delete: bool,
    #[serde(default = "String::new")]
    usermaps_from: String,
    #[serde(default = "String::new")]
    customassets_from: String,
}

fn default_true() -> bool {
    true
}

fn default_openspy() -> String {
    String::from("openspy.net")
}

fn change_settings_ini(config: &Config) -> Result<()> {
    let path = if !config.settings_ini_path.is_empty() {
        Some(Path::new(&config.settings_ini_path).into())
    } else {
        None
    }
    .or_else(|| match std::env::consts::OS {
        "windows" => dirs::document_dir()
            .map(|mut pb| {
                pb.push("My Games");
                pb.push("Sid Meier's Railroads");
                pb.push("Settings.ini");
                pb
            })
            .map(|pb| pb.into_boxed_path()),
        "linux" => panic!("TODO"),
        _ => None,
    });

    let path = match path {
        Some(p) => Ok(p),
        None => Err("Path not configured, and could not locate it automatically"),
    }?;
    debug!("Settings.ini path: {:?}", path);

    let mut i = Ini::load_from_file_opt(
        &path,
        ini::ParseOption {
            enabled_quote: false,
            // The game writes EditorPath with single backslashes, so they
            // shouldn't be escaped.
            enabled_escape: false,
            enabled_indented_mutiline_value: false,
            enabled_preserve_key_leading_whitespace: false,
        },
    )?;
    let mut user_settings = i.with_section(Some("User Settings"));

    if config.best_graphics {
        user_settings.set("FSAA", "8");
        user_settings.set("TextureLevel", "2");
        user_settings.set("ClutterDensity", "3");
        user_settings.set("ShadowQuality", "2");
        user_settings.set("TerrainShaderLevel", "2");
    }

    user_settings.set(
        "SkipOpeningMovies",
        if config.skip_opening { "1" } else { "0" },
    );

    user_settings.set(
        "EditorEnabled",
        if config.enable_editor { "1" } else { "0" },
    );
    if config.enable_editor {
        user_settings.set("EditorWarningViewed", "1");
    }

    i.write_to_file_opt(
        &path,
        ini::WriteOption {
            // As above, don't escape the backslashes.
            escape_policy: ini::EscapePolicy::Nothing,
            line_separator: ini::LineSeparator::CRLF,
            kv_separator: " = ",
        },
    )?;

    Ok(())
}

fn main() {
    colog::init();

    let args = Args::parse();

    let settings = config::Config::builder()
        .add_source(config::File::with_name("railroadscli").required(false))
        .add_source(config::Environment::with_prefix("RR"))
        .build()
        .unwrap();

    let config = settings.try_deserialize::<Config>().unwrap();
    debug!("Loaded config: {:?}", config);

    // Errors halt processing within each function, but we can skip to the next
    // step instead of aborting the whole program.

    if let Err(err) = change_settings_ini(&config) {
        error!("Error processing Settings.ini file: {:?}", err);
    }
}
