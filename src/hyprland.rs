//! Direct, bounded Hyprland request socket transport. No hyprctl subprocess.
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{
    env,
    io::{Read, Write},
    net::Shutdown,
    os::unix::{ffi::OsStrExt, net::UnixStream},
    path::PathBuf,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Hyprland {
    pub socket: PathBuf,
}

impl Hyprland {
    pub fn from_env() -> Result<Self> {
        let runtime = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
        let signature =
            env::var("HYPRLAND_INSTANCE_SIGNATURE").context("Not in a Hyprland session")?;
        ensure!(
            !signature.is_empty() && !signature.contains('/') && !signature.contains(".."),
            "Invalid Hyprland instance signature"
        );
        Ok(Self {
            socket: PathBuf::from(runtime)
                .join("hypr")
                .join(signature)
                .join(".socket.sock"),
        })
    }

    /// Keep the control socket well below Unix's 108-byte address limit, even
    /// with Hyprland's long instance signatures. FNV-1a is stable across upgrades.
    pub fn runtime_directory(&self) -> Result<PathBuf> {
        let instance = self.socket.parent().context("Missing instance directory")?;
        let hypr = instance
            .parent()
            .context("Missing Hyprland runtime directory")?;
        ensure!(
            hypr.file_name().is_some_and(|name| name == "hypr"),
            "Unexpected Hyprland socket location"
        );
        let root = hypr.parent().context("Missing XDG runtime directory")?;
        let hash = self
            .socket
            .as_os_str()
            .as_bytes()
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        Ok(root.join(format!("signal-loss-{hash:016x}")))
    }

    pub fn locked(&self) -> Result<bool> {
        self.json("locked")?["locked"]
            .as_bool()
            .context("Hyprland locked query unavailable")
    }

    pub fn request(&self, request: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket)
            .with_context(|| format!("Connecting to {}", self.socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(request.as_bytes())?;
        stream.shutdown(Shutdown::Write)?;
        let mut bytes = Vec::new();
        stream.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 1024 * 1024,
            "Hyprland response exceeds 1 MiB"
        );
        Ok(String::from_utf8(bytes)?)
    }

    pub fn json(&self, command: &str) -> Result<Value> {
        let response = self.request(&format!("j/{command}"))?;
        serde_json::from_str(&response)
            .with_context(|| format!("Invalid Hyprland JSON for {command}: {response}"))
    }

    pub fn get(&self, key: &str) -> Result<String> {
        let value = self.json(&format!("getoption {key}"))?;
        if let Some(s) = value["str"].as_str() {
            return Ok(s.to_owned());
        }
        if let Some(b) = value["bool"].as_bool() {
            return Ok(if b { "1" } else { "0" }.into());
        }
        if let Some(n) = value["int"].as_i64() {
            return Ok(n.to_string());
        }
        bail!("Unsupported or absent Hyprland option {key}: {value}")
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        ensure!(
            !value.contains(['\n', '\r', '\0']) && !key.contains(['\n', '\r', '\0']),
            "Invalid IPC value"
        );
        let mut response = self.request(&format!("/keyword {key} {value}"))?;
        if response.starts_with("keyword can't work with non-legacy parsers") {
            // A definite refusal is safe to adapt; ambiguous transport failures are
            // never resent. This uses the compositor's built-in Lua config API,
            // not an external interpreter or plugin.
            ensure!(
                [
                    "decoration:screen_shader",
                    "debug:damage_tracking",
                    "debug:vfr"
                ]
                .contains(&key),
                "Unsupported Lua setting {key}"
            );
            let option = self.json(&format!("getoption {key}"))?;
            let literal = if option["str"].is_string() {
                serde_json::to_string(value)?
            } else if option["bool"].is_boolean() {
                (value.parse::<i64>()? != 0).to_string()
            } else {
                value.parse::<i64>()?.to_string()
            };
            let key = serde_json::to_string(&key.replace(':', "."))?;
            response = self.request(&format!("/eval hl.config({{[{key}]={literal}}})"))?;
        }
        ensure!(response.trim() == "ok", "Setting {key} failed: {response}");
        Ok(())
    }

    pub fn check(&self) -> Result<Value> {
        let version = self.json("version")?;
        // These exact option names are the integration contract for the researched 0.56.2 target.
        for key in [
            "decoration:screen_shader",
            "debug:damage_tracking",
            "debug:vfr",
        ] {
            self.get(key)?;
        }
        Ok(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_socket_is_short_stable_and_instance_specific() {
        let first = Hyprland {
            socket: PathBuf::from(format!(
                "/run/user/1000/hypr/{}_1234567890_1234567890/.socket.sock",
                "a".repeat(40)
            )),
        };
        let mut second = first.clone();
        second.socket = PathBuf::from(format!(
            "/run/user/1000/hypr/{}_1234567890_9876543210/.socket.sock",
            "a".repeat(40)
        ));
        let path = first.runtime_directory().unwrap();
        assert_eq!(path, first.runtime_directory().unwrap());
        assert!(path.join("control.sock").as_os_str().as_bytes().len() < 108);
        assert_ne!(path, second.runtime_directory().unwrap());
    }
}
