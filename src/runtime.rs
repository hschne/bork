//! Per-process graphics-library discovery. Never installs packages or changes system configuration.
use anyhow::{Context, Result, bail, ensure};
use object::read::elf::{Dyn, ElfFile64};
use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs::{self, File},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};

const READY: &str = "_BORK_RUNTIME_READY";
const LIBRARIES: &[&str] = &[
    "libwayland-client.so.0",
    "libwayland-cursor.so.0",
    "libwayland-egl.so.1",
    "libxkbcommon.so.0",
    "libxkbcommon-x11.so.0",
    "libEGL.so.1",
    "libGL.so.1",
    "libGLX.so.0",
    "libOpenGL.so.0",
    "libX11.so.6",
    "libXcursor.so.1",
    "libXi.so.6",
    "libXrandr.so.2",
];

pub fn bootstrap() -> Result<()> {
    if env::var_os(READY).is_some() {
        return Ok(());
    }
    let automatic = match env::var("BORK_NIX_DISCOVERY").as_deref() {
        Ok("0") => false,
        Ok("1") => true,
        Ok("auto") | Err(_) => {
            Path::new("/etc/NIXOS").exists()
                || fs::read_to_string("/etc/os-release").is_ok_and(|text| {
                    text.lines()
                        .any(|line| line == "ID=nixos" || line == "ID=\"nixos\"")
                })
        }
        Ok(value) => bail!("BORK_NIX_DISCOVERY must be auto, 0, or 1, not {value:?}"),
    };
    let mut paths = Vec::new();
    if let Some(value) = env::var_os("BORK_LIBRARY_PATH") {
        for path in env::split_paths(&value).filter(|path| !path.as_os_str().is_empty()) {
            ensure!(
                path.is_absolute() && path.is_dir(),
                "BORK_LIBRARY_PATH must contain existing absolute directories: {}",
                path.display()
            );
            paths.push(path);
        }
    }
    if let Some(value) = env::var_os("LD_LIBRARY_PATH") {
        paths.extend(env::split_paths(&value).filter(|path| !path.as_os_str().is_empty()));
    }
    if automatic {
        if let Some(binary) = env::var_os("BORK_HYPRLAND_BINARY") {
            let binary = PathBuf::from(binary);
            ensure!(
                binary.is_absolute() && binary.is_file(),
                "BORK_HYPRLAND_BINARY must name an existing absolute executable path"
            );
        }
        if let Some(binary) = hyprland_binary() {
            match discover(&binary) {
                Ok(found) => paths.extend(found),
                Err(error) if env::var_os("BORK_HYPRLAND_BINARY").is_some() => {
                    return Err(error.context("Cannot inspect BORK_HYPRLAND_BINARY"));
                }
                Err(_) => (), // Explicit paths remain usable when no readable compositor ELF exists.
            }
        }
        if Path::new("/run/opengl-driver/lib").is_dir() {
            paths.push(PathBuf::from("/run/opengl-driver/lib"));
        }
    }
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    if paths.is_empty() {
        return Ok(());
    }
    let joined = env::join_paths(paths).context("Invalid graphics library search path")?;
    if env::var_os("LD_LIBRARY_PATH").as_ref() == Some(&joined) {
        return Ok(());
    }
    // glibc caches its search path at startup. Re-exec before creating any threads
    // or display state; Command::env changes only the replacement process, not globals.
    let error = Command::new(env::current_exe()?)
        .args(env::args_os().skip(1))
        .env("LD_LIBRARY_PATH", joined)
        .env(READY, "1")
        .exec();
    Err(error).context("Restarting Bork with graphics-library paths")
}

pub fn describe() {
    match env::var_os("LD_LIBRARY_PATH") {
        Some(paths) => {
            println!("Graphics library search paths:");
            for path in env::split_paths(&paths) {
                println!("  {}", path.display());
            }
        }
        None => println!("Graphics libraries: system loader defaults"),
    }
}

fn hyprland_binary() -> Option<PathBuf> {
    let configured = env::var_os("BORK_HYPRLAND_BINARY");
    let candidate = configured
        .map(PathBuf::from)
        .or_else(|| {
            // Prefer the package over /run/wrappers' capability launcher.
            Path::new("/run/current-system/sw/bin/Hyprland")
                .is_file()
                .then(|| PathBuf::from("/run/current-system/sw/bin/Hyprland"))
        })
        .or_else(|| {
            env::var_os("PATH").and_then(|paths| {
                env::split_paths(&paths)
                    .map(|path| path.join("Hyprland"))
                    .find(|path| path.is_file())
            })
        })?;
    let resolved = fs::canonicalize(candidate).ok()?;
    let wrapped = resolved
        .parent()?
        .join(format!(".{}-wrapped", resolved.file_name()?.to_str()?));
    Some(if wrapped.is_file() { wrapped } else { resolved })
}

fn elf_paths(path: &Path) -> Result<Vec<PathBuf>> {
    // Read ELF metadata on demand, including for large unstripped executables.
    let data = object::read::ReadCache::new(File::open(path)?);
    let elf = ElfFile64::<object::Endianness, _>::parse(&data)?;
    let endian = elf.endian();
    let sections = elf.elf_section_table();
    let Some((dynamic, index)) = sections.dynamic(endian, &data)? else {
        return Ok(Vec::new());
    };
    let strings = sections.strings(endian, &data, index)?;
    let mut paths = Vec::new();
    for entry in dynamic {
        if !matches!(
            entry.tag32(endian),
            Some(object::elf::DT_RUNPATH | object::elf::DT_RPATH)
        ) {
            continue;
        }
        let offset = entry
            .val32(endian)
            .context("Invalid ELF library-path offset")?;
        let value = strings
            .get(offset)
            .map_err(|()| anyhow::anyhow!("Invalid ELF library-path string"))?;
        let value = std::str::from_utf8(value)?;
        let origin = path
            .parent()
            .context("ELF has no parent directory")?
            .to_string_lossy();
        let value = value
            .replace("${ORIGIN}", &origin)
            .replace("$ORIGIN", &origin);
        paths.extend(
            env::split_paths(&OsString::from(value))
                .filter(|path| path.is_absolute() && path.is_dir()),
        );
    }
    Ok(paths)
}

fn discover(binary: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = elf_paths(binary)?;
    let mut visited = HashSet::new();
    let mut libraries = HashSet::new();
    let mut found = Vec::new();
    // Follow only graphics-library RUNPATHs, not every library in Hyprland's
    // closure. In particular, never inject a different libc into this executable.
    while let Some(directory) = pending.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        ensure!(
            visited.len() <= 256,
            "Graphics library discovery exceeded 256 directories"
        );
        let mut relevant = false;
        for name in LIBRARIES {
            let path = directory.join(name);
            if !path.is_file() {
                continue;
            }
            relevant = true;
            if libraries.insert(path.clone())
                && let Ok(paths) = elf_paths(&path)
            {
                pending.extend(paths);
            }
        }
        if relevant {
            found.push(directory);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_real_elf_and_rejects_non_elf() {
        assert!(elf_paths(&env::current_exe().unwrap()).is_ok());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-elf");
        fs::write(&path, "#!/bin/sh\n").unwrap();
        assert!(elf_paths(&path).is_err());
    }
}
