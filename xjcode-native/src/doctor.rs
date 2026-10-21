use crate::setup::Settings;
use anyhow::{Context, Result, bail, ensure};
use std::{ffi::OsString, path::Path};

pub const USAGE: &str = "用法：xjcode doctor [--probe] [-m|--model 交晓智授权模型名称]";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub probe: bool,
    pub model: Option<String>,
}

impl Options {
    pub fn parse(args: &[OsString]) -> Result<Self> {
        let mut options = Self::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--probe") => {
                    ensure!(!options.probe, "{USAGE}");
                    options.probe = true;
                }
                Some("-m" | "--model") => {
                    ensure!(options.model.is_none(), "{USAGE}");
                    let value = args
                        .next()
                        .and_then(|value| value.to_str())
                        .context(USAGE)?;
                    ensure!(
                        !value.trim().is_empty() && !value.starts_with('-'),
                        "{USAGE}"
                    );
                    options.model = Some(value.to_owned());
                }
                _ => bail!("{USAGE}"),
            }
        }
        Ok(options)
    }

    // Read the same home default used by the engine; selecting a model in the
    // TUI persists config.toml without rewriting the cached login settings.
    // An explicit model permits diagnosing the current session's override.
    // Never write settings or silently choose a different catalog model.
    pub fn model_name(&self, home: &Path, settings: &Settings) -> Result<String> {
        if let Some(model) = &self.model {
            return Ok(model.clone());
        }
        let text = match std::fs::read_to_string(home.join("config.toml")) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(settings.model.clone());
            }
            Err(error) => return Err(error).context("无法读取 XJCode config.toml"),
        };
        let config = text
            .parse::<toml_edit::DocumentMut>()
            .context("XJCode config.toml 格式错误")?;
        match config.get("model") {
            Some(value) => Ok(value
                .as_str()
                .filter(|name| !name.trim().is_empty())
                .context("config.toml 的 model 必须是非空字符串")?
                .to_owned()),
            None => Ok(settings.model.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings {
            account_id: "fixture-account".into(),
            workspace_id: "fixture-workspace".into(),
            workspace_name: "fixture".into(),
            model: "stale-login-model".into(),
            models: vec![],
        }
    }

    #[test]
    fn probe_uses_persisted_engine_model_without_rewriting_settings() {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join("config.toml");
        let original = "model = 'current-session-model'\n[model_providers.xjtu]\nname = '交晓智'\n";
        std::fs::write(&file, original).unwrap();
        let options = Options::parse(&["--probe".into()]).unwrap();
        let settings = settings();
        assert_eq!(
            options.model_name(home.path(), &settings).unwrap(),
            "current-session-model"
        );
        assert_eq!(settings.model, "stale-login-model");
        assert_eq!(std::fs::read_to_string(file).unwrap(), original);
    }

    #[test]
    fn explicit_probe_model_wins_and_bad_options_do_not_become_probes() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("config.toml"), "model = 'home-default'\n").unwrap();
        for flag in ["-m", "--model"] {
            let options =
                Options::parse(&["--probe".into(), flag.into(), "selected-model".into()]).unwrap();
            assert!(options.probe);
            assert_eq!(
                options.model_name(home.path(), &settings()).unwrap(),
                "selected-model"
            );
        }
        for args in [
            vec!["--model"],
            vec!["--model", "--probe"],
            vec!["--probe", "--probe"],
            vec!["--unknown"],
            vec!["-m", "a", "-m", "b"],
        ] {
            let args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
            assert!(Options::parse(&args).is_err());
        }
    }

    #[test]
    fn missing_home_model_uses_login_default_but_invalid_config_is_not_hidden() {
        let home = tempfile::tempdir().unwrap();
        let options = Options::default();
        assert_eq!(
            options.model_name(home.path(), &settings()).unwrap(),
            "stale-login-model"
        );
        for text in ["[broken", "model = 42", "model = ''"] {
            std::fs::write(home.path().join("config.toml"), text).unwrap();
            assert!(options.model_name(home.path(), &settings()).is_err());
        }
    }
}
