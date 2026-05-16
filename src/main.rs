use std::env::consts::OS;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;

use clap::Parser;
use ini::Ini;
use log::{debug, error};
use serde::Deserialize;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser, Debug)]
struct Args {
    /// Don't launch the game after applying the configuration
    #[arg(short, long, default_value_t = false)]
    no_launch: bool,
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

fn get_settings_ini_path(config: &Config) -> Option<Box<Path>> {
    if !config.settings_ini_path.is_empty() {
        Some(Path::new(&config.settings_ini_path).into())
    } else {
        None
    }
    .or_else(|| match OS {
        "windows" => {
            let mut pb = dirs::document_dir()?;
            pb.push("My Games");
            pb.push("Sid Meier's Railroads");
            pb.push("Settings.ini");
            Some(pb.into_boxed_path())
        }
        "linux" => panic!("TODO"),
        _ => None,
    })
}

fn get_railroads_exe_path(config: &Config) -> Option<Box<Path>> {
    const STEAM_APP_ID: u32 = 7600;

    if !config.railroads_exe_path.is_empty() {
        Some(Path::new(&config.railroads_exe_path).into())
    } else {
        None
    }
    .or_else(|| {
        let steam_dir = steamlocate::locate().ok()?;
        let (app, library) = steam_dir.find_app(STEAM_APP_ID).ok()??;
        let mut pb = library.resolve_app_dir(&app);
        pb.push("RailRoads.exe");
        Some(pb.into_boxed_path())
    })
}

fn change_settings_ini(config: &Config) -> Result<()> {
    let path = match get_settings_ini_path(config) {
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

fn change_railroads_exe(config: &Config) -> Result<()> {
    let path = match get_railroads_exe_path(config) {
        Some(p) => Ok(p),
        None => Err("Path not configured, and could not locate it automatically"),
    }?;
    debug!("RailRoads.exe path: {:?}", path);

    change_railroads_exe_laa(config, &path)
        .and_then(|()| change_railroads_exe_gamespy(config, &path))
}

fn change_railroads_exe_laa(config: &Config, path: &Box<Path>) -> Result<()> {
    // Locate the PE header, then set the LAA flag in the characteristics field.
    // https://github.com/pyinstaller/pyinstaller/issues/1288#issuecomment-109787370
    const PE_OFFSET_POINTER: u64 = 0x3c;
    const PE_CHARACTERISTICS_OFFSET: u64 = 22;
    const LAA_FLAG: u16 = 0x20;

    let mut exe = File::options().read(true).write(true).open(path)?;

    let mut buf = [0; 2];
    exe.read_exact(&mut buf)?;
    if buf != [b'M', b'Z'] {
        return Err(
            "RailRoads.exe does not look like an executable (invalid DOS magic header)".into(),
        );
    }

    let mut buf = [0; 4];
    exe.seek(SeekFrom::Start(PE_OFFSET_POINTER))?;
    exe.read_exact(&mut buf)?;
    let pe_offset = u32::from_le_bytes(buf);
    debug!("RailRoads.exe PE offset: 0x{:x?}", pe_offset);

    let mut buf = [0; 4];
    exe.seek(SeekFrom::Start(pe_offset as u64))?;
    exe.read_exact(&mut buf)?;
    if buf != [b'P', b'E', 0, 0] {
        return Err(
            "RailRoads.exe does not look like an executable (invalid PE magic header)".into(),
        );
    }

    let mut buf = [0; 2];
    exe.seek(SeekFrom::Start(
        pe_offset as u64 + PE_CHARACTERISTICS_OFFSET,
    ))?;
    exe.read_exact(&mut buf)?;
    let chars = u16::from_le_bytes(buf);

    let to_write = if config.laa_aware {
        chars | LAA_FLAG
    } else {
        chars & !LAA_FLAG
    };
    debug!(
        "Writing new characteristics to RailRoads.exe: 0x{:x?}",
        to_write
    );
    exe.seek(SeekFrom::Start(
        pe_offset as u64 + PE_CHARACTERISTICS_OFFSET,
    ))?;
    exe.write_all(&to_write.to_le_bytes())?;

    Ok(())
}

fn change_railroads_exe_gamespy(config: &Config, path: &Box<Path>) -> Result<()> {
    const OFFSETS: [u64; 9] = [
        0x618da2, 0x618eb4, 0x618f34, 0x618f61, 0x618f84, 0x618f98, 0x618fac, 0x619209, 0x619670,
    ];

    let server = &config.gamespy_server;
    if !server.is_ascii() {
        return Err("GameSpy server name must only contain ASCII characters".into());
    }
    if server.len() != 11 {
        return Err("GameSpy server name must be exactly 11 characters long".into());
    }

    let mut exe = File::options().read(true).write(true).open(path)?;
    for offset in OFFSETS {
        let mut buf = [0; 11];
        exe.seek(SeekFrom::Start(offset))?;
        exe.read_exact(&mut buf)?;
        match str::from_utf8(&buf) {
            Ok(s) => debug!("Setting RailRoads.exe GameSpy server at 0x{:x?}, currently: {}", offset, s),
            Err(_) => return Err("GameSpy server name is not where we expected it in the executable, so not setting it".into())
        }

        exe.seek(SeekFrom::Start(offset))?;
        exe.write_all(str::as_bytes(server))?;
    }

    Ok(())
}

fn launch_game(config: &Config) {
    match OS {
        "windows" => {
            if let Some(exe) = get_railroads_exe_path(&config) {
                debug!("Launching: {:?}", exe);
                let _ = Command::new(exe.as_os_str()).spawn();
            }
        }
        "linux" => panic!("TODO"),
        _ => {}
    }
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

    if let Err(err) = change_railroads_exe(&config) {
        error!("Error processing RailRoads.exe: {:?}", err);
    }

    if !args.no_launch {
        launch_game(&config);
    }
}
