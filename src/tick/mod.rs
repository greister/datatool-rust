use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};

use crate::formats::tick::TickItem;

pub fn create_tick(vipdoc: &Path, start_date: &str, end_date: &str) -> Result<()> {
    let start: u32 = start_date
        .parse()
        .with_context(|| format!("无效的开始日期: {}", start_date))?;
    let end: u32 = end_date
        .parse()
        .with_context(|| format!("无效的结束日期: {}", end_date))?;

    let newdatetick_dir = vipdoc.join("newdatetick");
    if !newdatetick_dir.exists() {
        return Err(anyhow!(
            "newdatetick目录不存在: {}",
            newdatetick_dir.display()
        ));
    }

    let newtick_dir = vipdoc.join("newtick");
    std::fs::create_dir_all(&newtick_dir)?;

    let entries = std::fs::read_dir(&newdatetick_dir)?;
    let mut htc_files: Vec<_> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "htc" || ext == "tic")
                .unwrap_or(false)
        })
        .collect();

    htc_files.sort_by_key(|e| e.file_name());

    for entry in &htc_files {
        let name = entry.file_name().to_string_lossy().to_string();
        let date_part = name.trim_end_matches(".htc").trim_end_matches(".tic");
        let file_date: u32 = match date_part.parse() {
            Ok(d) => d,
            Err(_) => {
                debug!("无法解析日期: {}", name);
                continue;
            }
        };

        if file_date < start || file_date > end {
            debug!("跳过日期 {} 不在范围内", file_date);
            continue;
        }

        info!("处理分笔: {} ({})", name, file_date);
        process_htc_file(&entry.path(), &newtick_dir)?;
    }

    println!("分笔数据转档完成: {} - {}", start_date, end_date);
    Ok(())
}

fn process_htc_file(htc_path: &Path, newtick_dir: &Path) -> Result<()> {
    let data = std::fs::read(htc_path)
        .with_context(|| format!("无法读取文件: {}", htc_path.display()))?;

    if data.len() < 2 {
        return Ok(());
    }

    let stock_count = u16::from_le_bytes([data[0], data[1]]) as usize;
    let mut pos = 2usize;

    for _ in 0..stock_count {
        if pos + 20 > data.len() {
            warn!("数据截断: pos={}, len={}", pos, data.len());
            break;
        }

        let market = data[pos];
        let code_bytes = &data[pos + 1..pos + 8];
        let code = String::from_utf8_lossy(code_bytes)
            .trim_end_matches('\0')
            .to_string();
        let _tick_date = u32::from_le_bytes(data[pos + 8..pos + 12].try_into()?);
        let tick_size = u32::from_le_bytes(data[pos + 12..pos + 16].try_into()?);
        let _unknown = u32::from_le_bytes(data[pos + 16..pos + 20].try_into()?);
        pos += 20;

        if pos + tick_size as usize > data.len() {
            warn!("分笔数据截断: code={}", code);
            break;
        }

        let tick_data = &data[pos..pos + tick_size as usize];
        pos += tick_size as usize;

        let market_str = if market == 0 {
            "sz"
        } else if market == 1 {
            "sh"
        } else {
            "bj"
        };

        write_tick_to_files(newtick_dir, market_str, &code, tick_data)?;
    }

    Ok(())
}

fn write_tick_to_files(
    newtick_dir: &Path,
    market: &str,
    code: &str,
    tick_data: &[u8],
) -> Result<()> {
    if tick_data.len() < 20 {
        return Ok(());
    }

    let prefix = format!("{}{}", market, code);

    let vid_path = newtick_dir.join(format!("{}.vid", prefix));
    let vtc_path = newtick_dir.join(format!("{}.vtc", prefix));

    let vid_data = &tick_data[20..];

    if !vid_data.is_empty() {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&vid_path)?;
        f.write_all(vid_data)?;
    }

    let _header = TickItem::read_header(&mut std::io::Cursor::new(&tick_data[..20]))?;
    let header_data = &tick_data[..20];

    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&vtc_path)?;
    f.write_all(header_data)?;

    Ok(())
}

pub fn del_tick(vipdoc: &Path, start_date: &str, end_date: &str) -> Result<()> {
    let start: u32 = start_date
        .parse()
        .with_context(|| format!("无效的开始日期: {}", start_date))?;
    let end: u32 = end_date
        .parse()
        .with_context(|| format!("无效的结束日期: {}", end_date))?;

    let newdatetick_dir = vipdoc.join("newdatetick");
    if newdatetick_dir.exists() {
        let entries = std::fs::read_dir(&newdatetick_dir)?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let date_part = name.trim_end_matches(".htc").trim_end_matches(".tic");
            if let Ok(file_date) = date_part.parse::<u32>() {
                if file_date >= start && file_date <= end {
                    std::fs::remove_file(entry.path())?;
                    info!("删除分笔源文件: {}", name);
                }
            }
        }
    }

    println!("分笔数据删除完成: {} - {}", start_date, end_date);
    Ok(())
}

pub fn check_all(vipdoc: &Path) -> Result<()> {
    let newtick_dir = vipdoc.join("newtick");
    if !newtick_dir.exists() {
        println!("newtick目录不存在");
        return Ok(());
    }

    let mut total_vid = 0u64;
    let mut total_vtc = 0u64;

    let entries = std::fs::read_dir(&newtick_dir)?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let size = entry.metadata()?.len();
        if name.ends_with(".vid") {
            total_vid += 1;
            info!("VID: {} ({} bytes)", name, size);
        } else if name.ends_with(".vtc") {
            total_vtc += 1;
            info!("VTC: {} ({} bytes)", name, size);
        }
    }

    println!(
        "分笔数据检查完成: {} 个VID文件, {} 个VTC文件",
        total_vid, total_vtc
    );
    Ok(())
}
