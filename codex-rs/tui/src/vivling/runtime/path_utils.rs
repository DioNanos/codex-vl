use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::*;

pub(crate) fn resolve_input_path(cwd: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

pub(crate) fn ensure_extension(path: PathBuf, ext: &str) -> PathBuf {
    if path.extension().and_then(|value| value.to_str()) == Some(ext) {
        path
    } else {
        path.with_extension(ext)
    }
}

pub(crate) fn read_zip_json<T: serde::de::DeserializeOwned>(
    archive: &mut ZipArchive<fs::File>,
    name: &str,
) -> io::Result<T> {
    let mut file = archive.by_name(name).map_err(io::Error::other)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    serde_json::from_str(&buf).map_err(io::Error::other)
}

pub(crate) fn roman_numeral(n: usize) -> String {
    match n {
        1 => "I".to_string(),
        2 => "II".to_string(),
        3 => "III".to_string(),
        4 => "IV".to_string(),
        5 => "V".to_string(),
        6 => "VI".to_string(),
        7 => "VII".to_string(),
        8 => "VIII".to_string(),
        9 => "IX".to_string(),
        10 => "X".to_string(),
        _ => n.to_string(),
    }
}
