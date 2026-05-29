use gpui::App;
use gpui_component::Theme;

pub(crate) const REQ_FONT_DEFAULT_FAMILY: &str = ".SystemUIFont";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FontConfig {
    pub family: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FontThemeOverrideDecision {
    NoConfiguredFamily,
    Apply(String),
    Unavailable(String),
}

#[derive(Debug, Default, serde::Deserialize)]
struct ReqFontConfigFile {
    #[serde(default)]
    font: ReqFontSection,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ReqFontSection {
    #[serde(default)]
    family: Option<String>,
}

pub(crate) fn req_font_generated_default_config() -> FontConfig {
    FontConfig {
        family: Some(REQ_FONT_DEFAULT_FAMILY.to_string()),
    }
}

fn req_font_trimmed_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn req_font_config_from_parsed(parsed: &ReqFontConfigFile) -> FontConfig {
    FontConfig {
        family: req_font_trimmed_non_empty(parsed.font.family.as_deref()),
    }
}

fn load_font_config_result(path: &std::path::Path) -> std::io::Result<FontConfig> {
    if path.exists() && !path.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("req-font config path is not a file path={}", path.display()),
        ));
    }

    if !path.is_file() {
        crate::log::trace_debug(format!(
            "req-font config missing path={} defaults family_override=false",
            path.display()
        ));
        return Ok(FontConfig::default());
    }

    let raw = std::fs::read_to_string(path)?;
    let parsed: ReqFontConfigFile = toml::from_str(&raw)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let resolved = req_font_config_from_parsed(&parsed);

    crate::log::trace_debug(format!(
        "req-font config loaded path={} family_override={} family={}",
        path.display(),
        resolved.family.is_some(),
        resolved.family.as_deref().unwrap_or("")
    ));
    Ok(resolved)
}

pub(crate) fn load_font_config(path: &std::path::Path) -> FontConfig {
    match load_font_config_result(path) {
        Ok(config) => config,
        Err(error) => {
            crate::log::trace_debug(format!(
                "req-font config fallback path={} error={} defaults family_override=false",
                path.display(),
                error
            ));
            FontConfig::default()
        }
    }
}

pub(crate) fn req_font_theme_override_decision(
    config: &FontConfig,
    available_font_names: &[String],
) -> FontThemeOverrideDecision {
    let Some(family) = config.family.as_deref() else {
        return FontThemeOverrideDecision::NoConfiguredFamily;
    };

    if available_font_names.iter().any(|name| name == family) {
        FontThemeOverrideDecision::Apply(family.to_string())
    } else {
        FontThemeOverrideDecision::Unavailable(family.to_string())
    }
}

pub(crate) fn apply_font_theme_overrides(
    config: &FontConfig,
    cx: &mut App,
) -> FontThemeOverrideDecision {
    let available_font_names = cx.text_system().all_font_names();
    let decision = req_font_theme_override_decision(config, available_font_names.as_slice());

    match &decision {
        FontThemeOverrideDecision::NoConfiguredFamily => {
            let theme = Theme::global(cx);
            crate::log::trace_debug(format!(
                "req-font theme override skipped reason=no_configured_family default_font_family={} default_mono_font_family={}",
                theme.font_family, theme.mono_font_family
            ));
        }
        FontThemeOverrideDecision::Apply(family) => {
            let theme = Theme::global_mut(cx);
            let previous_font_family = theme.font_family.clone();
            let previous_mono_font_family = theme.mono_font_family.clone();
            theme.font_family = family.as_str().into();
            theme.mono_font_family = family.as_str().into();
            crate::log::trace_debug(format!(
                "req-font theme override applied family={} previous_font_family={} previous_mono_font_family={} available_fonts={}",
                family,
                previous_font_family,
                previous_mono_font_family,
                available_font_names.len()
            ));
        }
        FontThemeOverrideDecision::Unavailable(family) => {
            let theme = Theme::global(cx);
            crate::log::trace_debug(format!(
                "req-font theme override fallback requested_family={} reason=unavailable default_font_family={} default_mono_font_family={} available_fonts={}",
                family,
                theme.font_family,
                theme.mono_font_family,
                available_font_names.len()
            ));
        }
    }

    decision
}

#[cfg(test)]
mod tests {
    use super::{
        FontConfig, FontThemeOverrideDecision, REQ_FONT_DEFAULT_FAMILY,
        req_font_generated_default_config, req_font_theme_override_decision,
    };
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn req_font_test_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "gpui_papyru2_req_font_{name}_{}_{}",
            std::process::id(),
            stamp
        ));
        path
    }

    #[test]
    fn font_test1_req_font_missing_section_uses_no_override() {
        let root = req_font_test_temp_root("font_test1");
        let config_path = root.join("conf").join(crate::app::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(
            config_path.as_path(),
            "[color]\nbackground = 0xf7f2ec\nforeground = 0x437085\n",
        )
        .expect("write color-only config");

        let resolved = super::load_font_config(config_path.as_path());
        assert_eq!(resolved, FontConfig::default());
    }

    #[test]
    fn font_test2_req_font_family_loads_and_trims() {
        let root = req_font_test_temp_root("font_test2");
        let config_path = root.join("conf").join(crate::app::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(
            config_path.as_path(),
            "[font]\nfamily = \"  Noto Sans JP  \"\n",
        )
        .expect("write font config");

        let resolved = super::load_font_config(config_path.as_path());
        assert_eq!(resolved.family.as_deref(), Some("Noto Sans JP"));
    }

    #[test]
    fn font_test3_req_font_empty_family_uses_no_override() {
        let root = req_font_test_temp_root("font_test3");
        let config_path = root.join("conf").join(crate::app::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(config_path.as_path(), "[font]\nfamily = \"   \"\n")
            .expect("write empty font config");

        let resolved = super::load_font_config(config_path.as_path());
        assert_eq!(resolved, FontConfig::default());
    }

    #[test]
    fn font_test4_req_font_invalid_toml_falls_back_without_panic() {
        let root = req_font_test_temp_root("font_test4");
        let config_path = root.join("conf").join(crate::app::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(config_path.as_path(), "[font]\nfamily = 11\n")
            .expect("write invalid font config");

        let resolved = super::load_font_config(config_path.as_path());
        assert_eq!(resolved, FontConfig::default());
    }

    #[test]
    fn font_test5_req_font_default_generated_family_is_system_ui() {
        let resolved = req_font_generated_default_config();
        assert_eq!(resolved.family.as_deref(), Some(REQ_FONT_DEFAULT_FAMILY));
    }

    #[test]
    fn font_test6_req_font_theme_decision_applies_available_family() {
        let config = FontConfig {
            family: Some("Noto Sans JP".to_string()),
        };
        let available = vec![
            ".SystemUIFont".to_string(),
            "Noto Sans JP".to_string(),
            "Consolas".to_string(),
        ];

        assert_eq!(
            req_font_theme_override_decision(&config, available.as_slice()),
            FontThemeOverrideDecision::Apply("Noto Sans JP".to_string())
        );
    }

    #[test]
    fn font_test7_req_font_invalid_family_name_falls_back_to_default_decision() {
        let config = FontConfig {
            family: Some("Definitely Missing Font Family".to_string()),
        };
        let available = vec![".SystemUIFont".to_string(), "Consolas".to_string()];

        assert_eq!(
            req_font_theme_override_decision(&config, available.as_slice()),
            FontThemeOverrideDecision::Unavailable("Definitely Missing Font Family".to_string())
        );
    }

    #[test]
    fn font_test8_req_font_default_created_config_contains_font_section() {
        let root = req_font_test_temp_root("font_test8");
        let config_path = root.join("conf").join(crate::app::PAPYRU2_CONF_FILE_NAME);

        let _ = crate::app::load_or_create_ui_color_config(config_path.as_path());
        let raw = std::fs::read_to_string(config_path.as_path()).expect("read created config");

        assert!(raw.contains("[font]"));
        assert!(raw.contains("family = \".SystemUIFont\""));
        assert!(!raw.contains("size ="));
        assert!(!raw.contains("style ="));
    }
}
