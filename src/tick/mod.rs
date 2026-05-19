use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};

const HTC_HEADER_SIZE: usize = 16;
const STOCK_HEADER_SIZE: usize = 30;
const VID_RECORD_SIZE: usize = 38;

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
        process_htc_file_v4(&entry.path(), &newtick_dir)?;
    }

    println!("分笔数据转档完成: {} - {}", start_date, end_date);
    Ok(())
}

fn process_htc_file_v4(htc_path: &Path, newtick_dir: &Path) -> Result<()> {
    let data = std::fs::read(htc_path)
        .with_context(|| format!("无法读取文件: {}", htc_path.display()))?;

    if data.len() < HTC_HEADER_SIZE {
        return Err(anyhow!("htc文件太小: {} bytes", data.len()));
    }

    let _version = u32::from_le_bytes(data[0..4].try_into()?);
    let file_date = u32::from_le_bytes(data[4..8].try_into()?);
    let _file_date2 = u32::from_le_bytes(data[8..12].try_into()?);
    let stock_count = u32::from_le_bytes(data[12..16].try_into()?) as usize;

    let file_prefix = &data[0..8];

    let mut pos = HTC_HEADER_SIZE;

    for i in 0..stock_count {
        if pos + STOCK_HEADER_SIZE > data.len() {
            warn!("数据截断: stock_idx={}, pos={}, len={}", i, pos, data.len());
            break;
        }

        let market = data[pos];
        let code_bytes = &data[pos + 1..pos + 8];
        let code = String::from_utf8_lossy(code_bytes)
            .trim_end_matches('\0')
            .to_string();
        let _stock_date = u32::from_le_bytes(data[pos + 8..pos + 12].try_into()?);
        let _decomp_size = u32::from_le_bytes(data[pos + 12..pos + 16].try_into()?);
        let comp_size = u32::from_le_bytes(data[pos + 16..pos + 20].try_into()?);

        let compressed_data_start = pos + STOCK_HEADER_SIZE;

        if compressed_data_start + comp_size as usize > data.len() {
            warn!(
                "压缩数据截断: code={}, need={} but available={}",
                code,
                comp_size,
                data.len() - compressed_data_start
            );
            break;
        }

        let compressed_data =
            &data[compressed_data_start..compressed_data_start + comp_size as usize];

        let market_str = match market {
            0 => "sh",
            1 => "sz",
            _ => "bj",
        };

        let prefix = format!("{}{}", market_str, code);

        let mut vid_data = Vec::with_capacity(VID_RECORD_SIZE);
        vid_data.extend_from_slice(file_prefix);
        vid_data.extend_from_slice(&file_date.to_le_bytes());
        vid_data.extend_from_slice(&1u16.to_le_bytes());
        vid_data.extend_from_slice(&0u16.to_le_bytes());
        vid_data.extend_from_slice(&data[pos + 8..pos + STOCK_HEADER_SIZE]);

        write_tick_files(newtick_dir, &prefix, &vid_data, compressed_data)?;

        pos = compressed_data_start + comp_size as usize;
    }

    Ok(())
}

fn write_tick_files(
    newtick_dir: &Path,
    prefix: &str,
    vid_header: &[u8],
    compressed_data: &[u8],
) -> Result<()> {
    let vid_path = newtick_dir.join(format!("{}.vid", prefix));
    let vtc_path = newtick_dir.join(format!("{}.vtc", prefix));

    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&vid_path)?;
        f.write_all(vid_header)?;
    }

    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&vtc_path)?;
        f.write_all(compressed_data)?;
    }

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

    let newtick_dir = vipdoc.join("newtick");
    if newtick_dir.exists() {
        let entries = std::fs::read_dir(&newtick_dir)?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".vid") || name.ends_with(".vtc") {
                std::fs::remove_file(entry.path())?;
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
