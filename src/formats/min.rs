use std::path::Path;

pub fn get_stock_files<P: AsRef<Path>>(
    newtick_dir: P,
    prefix: &str,
) -> Vec<String> {
    let dir = newtick_dir.as_ref();
    let mut stocks = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && name.ends_with(".vid") {
                let stock_code = &name[prefix.len()..name.len() - 4];
                if stock_code.len() == 6 && stock_code.chars().all(|c| c.is_ascii_digit()) {
                    stocks.push(stock_code.to_string());
                }
            }
        }
    }
    stocks.sort();
    stocks
}
