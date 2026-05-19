use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};

use crate::formats::{DayRecord, MARKETS};
use crate::formats::day::{CodRecord, Md1Block, COD_RECORD_SIZE, MD1_BLOCK_SIZE};

pub fn create_day(vipdoc: &Path, start_date: &str, end_date: &str) -> Result<()> {
    let start: u32 = start_date
        .parse()
        .with_context(|| format!("无效的开始日期: {}", start_date))?;
    let end: u32 = end_date
        .parse()
        .with_context(|| format!("无效的结束日期: {}", end_date))?;

    let refmhq_dir = vipdoc.join("refmhq");
    if !refmhq_dir.exists() {
        return Err(anyhow!("refmhq目录不存在: {}", refmhq_dir.display()));
    }

    for market in MARKETS {
        process_refmhq_for_market(&refmhq_dir, vipdoc, market, start, end)?;
    }

    println!("日线数据转档完成: {} - {}", start_date, end_date);
    Ok(())
}

fn process_refmhq_for_market(
    refmhq_dir: &Path,
    vipdoc: &Path,
    market_prefix: &str,
    start_date: u32,
    end_date: u32,
) -> Result<()> {
    let entries = std::fs::read_dir(refmhq_dir)
        .with_context(|| format!("无法读取refmhq目录: {}", refmhq_dir.display()))?;

    let mut cod_files: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(market_prefix) && name.ends_with(".cod") {
            cod_files.push(entry.path());
        }
    }

    for cod_path in cod_files {
        let filename = cod_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let base = &filename[..filename.len() - 4];
        let date_part = &base[market_prefix.len()..];

        let file_date: u32 = match date_part.parse() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if file_date < start_date || file_date > end_date {
            debug!("跳过日期 {} 不在范围内", file_date);
            continue;
        }

        let md1_path = refmhq_dir.join(format!("{}.md1", base));
        if !md1_path.exists() {
            warn!("缺少md1文件: {}", md1_path.display());
            continue;
        }

        info!("处理日线: {} ({})", base, file_date);
        merge_daily_data(&cod_path, &md1_path, vipdoc, market_prefix)?;
    }

    Ok(())
}

fn merge_daily_data(
    cod_path: &Path,
    md1_path: &Path,
    vipdoc: &Path,
    market_prefix: &str,
) -> Result<()> {
    let cod_data = std::fs::read(cod_path)?;
    let md1_data = std::fs::read(md1_path)?;

    let num_records = cod_data.len() / COD_RECORD_SIZE;

    for i in 0..num_records {
        let offset = i * COD_RECORD_SIZE;
        let cod = parse_cod_record(&cod_data[offset..offset + COD_RECORD_SIZE])?;

        let stock_code = cod.stock_code.trim_end_matches('\0');
        if stock_code.is_empty() {
            continue;
        }

        let md1_offset = cod.seq_num as usize * MD1_BLOCK_SIZE;
        if md1_offset + MD1_BLOCK_SIZE > md1_data.len() {
            warn!("md1偏移越界: {} seq={}", stock_code, cod.seq_num);
            continue;
        }

        let md1 = parse_md1_block(&md1_data[md1_offset..md1_offset + MD1_BLOCK_SIZE])?;

        let day_record = DayRecord {
            date: 0,
            open: (md1.open * 100.0) as u32,
            high: (md1.high * 100.0) as u32,
            low: (md1.low * 100.0) as u32,
            close: (md1.close * 100.0) as u32,
            amount: md1.amount as f32,
            volume: md1.volume,
            reserved: 0x10000,
        };

        let lday_dir = vipdoc
            .join(market_prefix)
            .join("lday");
        std::fs::create_dir_all(&lday_dir)?;

        let day_file = lday_dir.join(format!("{}{}.day", market_prefix, stock_code));
        append_or_update_day_record(&day_file, day_record, &cod_path)?;
    }

    Ok(())
}

fn parse_cod_record(data: &[u8]) -> Result<CodRecord> {
    if data.len() < COD_RECORD_SIZE {
        return Err(anyhow!("cod记录太短"));
    }
    let stock_code = String::from_utf8_lossy(&data[..6])
        .trim_end_matches('\0')
        .to_string();
    let seq_num = u16::from_le_bytes([data[0x20], data[0x21]]);
    Ok(CodRecord { stock_code, seq_num })
}

fn parse_md1_block(data: &[u8]) -> Result<Md1Block> {
    if data.len() < MD1_BLOCK_SIZE {
        return Err(anyhow!("md1块太短"));
    }

    let open = f64::from_le_bytes(data[0x0C..0x14].try_into()?);
    let high = f64::from_le_bytes(data[0x14..0x1C].try_into()?);
    let low = f64::from_le_bytes(data[0x1C..0x24].try_into()?);
    let close = f64::from_le_bytes(data[0x24..0x2C].try_into()?);
    let volume = u32::from_le_bytes(data[0x38..0x3C].try_into()?);
    let amount = f64::from_le_bytes(data[0x48..0x50].try_into()?);

    Ok(Md1Block {
        open,
        high,
        low,
        close,
        volume,
        amount,
    })
}

fn append_or_update_day_record(
    day_file: &Path,
    mut record: DayRecord,
    cod_path: &Path,
) -> Result<()> {
    let cod_filename = cod_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let market_prefix = extract_market_prefix(&cod_filename);
    let date_str = &cod_filename[market_prefix.len()..cod_filename.len() - 4];
    record.date = date_str.parse().unwrap_or(0);

    if !day_file.exists() {
        let mut f = BufWriter::new(std::fs::File::create(day_file)?);
        record.write_to(&mut f)?;
        return Ok(());
    }

    let file_data = std::fs::read(day_file)?;
    let num_existing = file_data.len() / 32;

    let mut records: Vec<DayRecord> = Vec::with_capacity(num_existing + 1);
    for i in 0..num_existing {
        let offset = i * 32;
        let mut cursor = std::io::Cursor::new(&file_data[offset..offset + 32]);
        records.push(DayRecord::read_from(&mut cursor)?);
    }

    let mut found = false;
    for rec in &mut records {
        if rec.date == record.date {
            *rec = record;
            found = true;
            break;
        }
    }

    if !found {
        records.push(record);
    }

    records.sort_by_key(|r| r.date);

    let mut f = BufWriter::new(std::fs::File::create(day_file)?);
    for rec in &records {
        rec.write_to(&mut f)?;
    }

    Ok(())
}

fn extract_market_prefix(filename: &str) -> String {
    for m in MARKETS {
        if filename.starts_with(m) {
            return m.to_string();
        }
    }
    String::new()
}

pub fn del_day(vipdoc: &Path, start_date: &str, end_date: &str) -> Result<()> {
    let start: u32 = start_date
        .parse()
        .with_context(|| format!("无效的开始日期: {}", start_date))?;
    let end: u32 = end_date
        .parse()
        .with_context(|| format!("无效的结束日期: {}", end_date))?;

    for market in MARKETS {
        let lday_dir = vipdoc.join(market).join("lday");
        if !lday_dir.exists() {
            continue;
        }

        let entries = std::fs::read_dir(&lday_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().map(|e| e == "day").unwrap_or(false) {
                continue;
            }

            let file_data = std::fs::read(&path)?;
            let num_records = file_data.len() / 32;

            let mut kept = Vec::new();
            for i in 0..num_records {
                let offset = i * 32;
                let mut cursor = std::io::Cursor::new(&file_data[offset..offset + 32]);
                let rec = DayRecord::read_from(&mut cursor)?;
                if rec.date < start || rec.date > end {
                    kept.push(rec);
                }
            }

            if kept.is_empty() {
                std::fs::remove_file(&path)?;
                info!("删除空文件: {}", path.display());
            } else {
                let mut f = BufWriter::new(std::fs::File::create(&path)?);
                for rec in &kept {
                    rec.write_to(&mut f)?;
                }
            }
        }
    }

    println!("日线数据删除完成: {} - {}", start_date, end_date);
    Ok(())
}

pub fn check_all(vipdoc: &Path) -> Result<()> {
    let mut total_files = 0u64;
    let mut total_records = 0u64;

    for market in MARKETS {
        let lday_dir = vipdoc.join(market).join("lday");
        if !lday_dir.exists() {
            continue;
        }

        let entries = std::fs::read_dir(&lday_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().map(|e| e == "day").unwrap_or(false) {
                continue;
            }

            let file_data = std::fs::read(&path)?;
            let num = file_data.len() / 32;
            let remainder = file_data.len() % 32;

            total_files += 1;
            total_records += num as u64;

            if remainder != 0 {
                warn!(
                    "文件大小异常: {} ({} 字节, 余数 {})",
                    path.display(),
                    file_data.len(),
                    remainder
                );
            }

            let name = path.file_name().unwrap().to_string_lossy();
            if num > 0 {
                let mut cursor = std::io::Cursor::new(&file_data[0..32]);
                let first = DayRecord::read_from(&mut cursor)?;
                let mut cursor = std::io::Cursor::new(&file_data[(num - 1) * 32..num * 32]);
                let last = DayRecord::read_from(&mut cursor)?;
                info!(
                    "{}: {} 条记录, {} - {}",
                    name, num, first.date, last.date
                );
            }
        }
    }

    println!("日线数据检查完成: {} 个文件, {} 条记录", total_files, total_records);
    Ok(())
}
