use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Настройки в TOML. Править их можно и с устройства — экран настроек
/// вызывает нативную клавиатуру inkview, см. [`crate::keyboard`], — и с
/// компьютера, положив файл рядом с приложением.
///
/// `serde(default)` означает, что отсутствующий ключ берётся из [`Default`]:
/// файл можно урезать до одного `server`, и всё остальное подставится.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    pub download_dir: PathBuf,
    pub formats: Vec<String>,
    pub limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: "http://192.168.1.10:8080".to_string(),
            user: None,
            password: None,
            library: None,
            download_dir: PathBuf::from("/mnt/ext1/Books"),
            formats: vec!["FB2".into(), "EPUB".into(), "PDF".into(), "MOBI".into()],
            limit: 200,
        }
    }
}

/// Шапка приписывается при каждом сохранении: `toml` комментарии не
/// сохраняет, а файл должен оставаться понятным тому, кто открыл его руками.
const HEADER: &str = "\
# pocket_calibre — настройки. Файл перезаписывается экраном настроек.
# server        — адрес calibre content server
# user/password — если сервер требует авторизацию (только basic)
# library       — id библиотеки; без него берётся библиотека по умолчанию
# formats       — по убыванию предпочтения, качается первый доступный

";

impl Config {
    /// Ищет конфиг рядом с исполняемым файлом, затем в стандартных папках
    /// PocketBook. Если ничего нет — создаёт файл с настройками по умолчанию
    /// и возвращает их вместе с путём.
    ///
    /// Третий элемент — сообщение для строки состояния, если что-то пошло
    /// не так или конфига не было вовсе.
    pub fn load() -> (Self, PathBuf, Option<String>) {
        let candidates = Self::candidate_paths();

        for path in &candidates {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };

            return match toml::from_str::<Self>(&text) {
                Ok(cfg) => (cfg, path.clone(), None),
                // Битый файл не перезаписываем: пользователь правил его руками,
                // и молча стереть правки хуже, чем поработать на умолчаниях.
                Err(e) => (
                    Self::default(),
                    path.clone(),
                    Some(format!("Ошибка в {}: {e}", path.display())),
                ),
            };
        }

        let target = candidates
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from("pocket_calibre.toml"));

        let defaults = Self::default();
        let note = match defaults.save(&target) {
            Ok(()) => "Настройки не найдены — укажите адрес сервера".to_string(),
            Err(e) => format!("Не удалось создать {}: {e}", target.display()),
        };

        (defaults, target, Some(note))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let body = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        std::fs::write(path, format!("{HEADER}{body}"))
    }

    fn candidate_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            paths.push(dir.join("pocket_calibre.toml"));
        }
        paths.push(PathBuf::from("/mnt/ext1/applications/pocket_calibre.toml"));
        paths.push(PathBuf::from("/mnt/ext1/system/config/pocket_calibre.toml"));

        paths
    }
}

/// Убирает из имени файла всё, что может не понравиться FAT-разделу ридера.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();

    let cleaned = cleaned.trim().trim_end_matches('.').to_string();

    // Ограничение длины имени на FAT32 — 255 байт, режем с запасом по символам.
    if cleaned.chars().count() > 120 {
        cleaned.chars().take(120).collect()
    } else if cleaned.is_empty() {
        "book".to_string()
    } else {
        cleaned
    }
}

pub fn ensure_dir(dir: &Path) -> std::io::Result<()> {
    if dir.exists() {
        Ok(())
    } else {
        std::fs::create_dir_all(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_round_trip() {
        let cfg = Config {
            server: "http://192.0.2.10:8080".to_string(),
            library: Some("Calibre_Library".to_string()),
            // Кавычки и пробелы — то, на чём ломался прежний key=value формат.
            password: Some("па \"роль\" с = и #".to_string()),
            download_dir: PathBuf::from("/mnt/ext1/Мои книги"),
            ..Default::default()
        };

        let text = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();

        assert_eq!(parsed.server, cfg.server);
        assert_eq!(parsed.library, cfg.library);
        assert_eq!(parsed.password, cfg.password);
        assert_eq!(parsed.download_dir, cfg.download_dir);
        assert_eq!(parsed.formats, cfg.formats);
        assert_eq!(parsed.limit, cfg.limit);
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let parsed: Config = toml::from_str("server = \"http://host:8080\"").unwrap();

        assert_eq!(parsed.server, "http://host:8080");
        assert_eq!(parsed.user, None);
        assert_eq!(parsed.limit, Config::default().limit);
        assert_eq!(parsed.formats, Config::default().formats);
    }
}
