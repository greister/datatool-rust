use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

#[derive(Debug)]
pub struct Config {
    sections: HashMap<String, HashMap<String, String>>,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref()).with_context(|| {
            format!(
                "无法读取配置文件: {}",
                path.as_ref().display()
            )
        })?;

        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current_section = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].trim().to_string();
                sections
                    .entry(current_section.clone())
                    .or_default();
            } else if let Some(pos) = line.find('=') {
                let key = line[..pos].trim().to_string();
                let value = line[pos + 1..].trim().to_string();
                sections
                    .entry(current_section.clone())
                    .or_default()
                    .insert(key, value);
            }
        }

        Ok(Config { sections })
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections.get(section)?.get(key).map(|s| s.as_str())
    }

    pub fn vipdoc_path(&self) -> Result<PathBuf> {
        let path_str = self
            .get("PATH", "VIPDOC")
            .ok_or_else(|| anyhow!("配置文件中未找到 [PATH] VIPDOC 设置"))?;

        let path = PathBuf::from(path_str);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(std::env::current_dir()?.join(path))
        }
    }

    pub fn ctrl_tckv4(&self) -> bool {
        self.get("CTRL", "TCKV4")
            .map(|v| v != "0")
            .unwrap_or(true)
    }

    pub fn mindays(&self) -> Option<u32> {
        self.get("MINDAYS", "MINDAYS")
            .or_else(|| self.get("PATH", "MINDAYS"))
            .and_then(|v| v.parse().ok())
    }

    pub fn transmin(&self) -> Option<bool> {
        self.get("PATH", "TRANSMIN")
            .map(|v| v != "0")
    }
}
