use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use log::debug;

use crate::formats::{MinRecord, MARKETS};
use crate::formats::min::get_stock_files;

pub fn create_min(vipdoc: &Path, start_date: &str, end_date: &str) -> Result<()> {
    let start: u32 = start_date
        .parse()
        .with_context(|| format!("无效的开始日期: {}", start_date))?;
    let end: u32 = end_date
        .parse()
        .with_context(|| format!("无效的结束日期: {}", end_date))?;

    let newtick_dir = vipdoc.join("newtick");
    if !newtick_dir.exists() {
        return Err(anyhow!("newtick目录不存在: {}", newtick_dir.display()));
    }

    for market in MARKETS {
        let stocks = get_stock_files(&newtick_dir, market);

        for code in stocks {
            let vid_path = newtick_dir.join(format!("{}{}.vid", market, code));
            let vtc_path = newtick_dir.join(format!("{}{}.vtc", market, code));

            if !vtc_path.exists() {
                continue;
            }

            let vtc_data = std::fs::read(&vtc_path)?;
            let vid_data = if vid_path.exists() {
                std::fs::read(&vid_path)?
            } else {
                Vec::new()
            };

            let records_1min = generate_min_records(&vtc_data, &vid_data, start, end)?;
            let records_5min = aggregate_to_5min(&records_1min);

            if !records_1min.is_empty() {
                write_min_file(vipdoc, market, &code, "minline", ".01", &records_1min)?;
            }

            if !records_5min.is_empty() {
                write_min_file(vipdoc, market, &code, "fzline", ".5", &records_5min)?;
            }
        }
    }

    println!("分钟线数据生成完成: {} - {}", start_date, end_date);
    Ok(())
}

pub fn create_min_all(vipdoc: &Path) -> Result<()> {
    let newtick_dir = vipdoc.join("newtick");
    if !newtick_dir.exists() {
        return Err(anyhow!("newtick目录不存在: {}", newtick_dir.display()));
    }

    for market in MARKETS {
        let stocks = get_stock_files(&newtick_dir, market);

        for code in stocks {
            let vid_path = newtick_dir.join(format!("{}{}.vid", market, code));
            let vtc_path = newtick_dir.join(format!("{}{}.vtc", market, code));

            if !vtc_path.exists() {
                continue;
            }

            let vtc_data = std::fs::read(&vtc_path)?;
            let vid_data = if vid_path.exists() {
                std::fs::read(&vid_path)?
            } else {
                Vec::new()
            };

            let records_1min = generate_all_min_records(&vtc_data, &vid_data)?;
            let records_5min = aggregate_to_5min(&records_1min);

            if !records_1min.is_empty() {
                write_min_file(vipdoc, market, &code, "minline", ".01", &records_1min)?;
            }

            if !records_5min.is_empty() {
                write_min_file(vipdoc, market, &code, "fzline", ".5", &records_5min)?;
            }
        }
    }

    println!("全部分笔数据转分钟数据完成");
    Ok(())
}

fn generate_min_records(
    vtc_data: &[u8],
    vid_data: &[u8],
    start_date: u32,
    end_date: u32,
) -> Result<Vec<MinRecord>> {
    let all = generate_all_min_records(vtc_data, vid_data)?;
    Ok(all
        .into_iter()
        .filter(|r| {
            let (y, m, d) = MinRecord::decode_date(r.date_raw);
            let date_val = y * 10000 + m * 100 + d;
            date_val >= start_date && date_val <= end_date
        })
        .collect())
}

fn generate_all_min_records(vtc_data: &[u8], _vid_data: &[u8]) -> Result<Vec<MinRecord>> {
    if vtc_data.is_empty() {
        return Ok(Vec::new());
    }

    let num_ticks = vtc_data.len() / 20;
    let mut ticks: Vec<TickEntry> = Vec::with_capacity(num_ticks);

    for i in 0..num_ticks {
        let offset = i * 20;
        if offset + 20 > vtc_data.len() {
            break;
        }
        let datetime = u32::from_le_bytes(vtc_data[offset..offset + 4].try_into()?);
        let _count = u16::from_le_bytes(vtc_data[offset + 4..offset + 6].try_into()?);
        let _vol_offset = u16::from_le_bytes(vtc_data[offset + 6..offset + 8].try_into()?);
        let _vol_size = u16::from_le_bytes(vtc_data[offset + 8..offset + 10].try_into()?);
        let tick_type = u16::from_le_bytes(vtc_data[offset + 10..offset + 12].try_into()?);
        let price = u32::from_le_bytes(vtc_data[offset + 12..offset + 16].try_into()?);
        let volume = u32::from_le_bytes(vtc_data[offset + 16..offset + 20].try_into()?);

        ticks.push(TickEntry {
            datetime,
            price,
            volume,
            tick_type,
        });
    }

    ticks.sort_by_key(|t| t.datetime);

    let mut minute_bars: BTreeMap<(u16, u16), MinBar> = BTreeMap::new();

    for tick in &ticks {
        let date_raw = (tick.datetime & 0xFFFF) as u16;
        let time_raw = ((tick.datetime >> 16) & 0xFFFF) as u16;

        let key = (date_raw, time_raw);
        let bar = minute_bars.entry(key).or_insert_with(|| MinBar {
            date_raw,
            time_raw,
            open: tick.price,
            high: tick.price,
            low: tick.price,
            close: tick.price,
            volume: 0,
            amount: 0.0,
        });

        bar.high = bar.high.max(tick.price);
        bar.low = bar.low.min(tick.price);
        bar.close = tick.price;
        bar.volume += tick.volume;
    }

    let mut records: Vec<MinRecord> = minute_bars
        .into_values()
        .map(|bar| MinRecord {
            date_raw: bar.date_raw,
            time_raw: bar.time_raw,
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
            amount: bar.amount,
            volume: bar.volume,
            reserved: 0,
        })
        .collect();

    records.sort_by_key(|r| (r.date_raw, r.time_raw));
    Ok(records)
}

struct TickEntry {
    datetime: u32,
    price: u32,
    volume: u32,
    tick_type: u16,
}

struct MinBar {
    date_raw: u16,
    time_raw: u16,
    open: u32,
    high: u32,
    low: u32,
    close: u32,
    volume: u32,
    amount: f32,
}

fn aggregate_to_5min(records: &[MinRecord]) -> Vec<MinRecord> {
    let mut result: Vec<MinRecord> = Vec::new();
    let mut current: Option<MinRecord> = None;
    let mut current_slot: u16 = 0;

    for rec in records {
        let slot = rec.time_raw / 5;
        if current.is_none() || slot != current_slot {
            if let Some(prev) = current.take() {
                result.push(prev);
            }
            current_slot = slot;
            current = Some(MinRecord {
                date_raw: rec.date_raw,
                time_raw: slot * 5,
                open: rec.open,
                high: rec.high,
                low: rec.low,
                close: rec.close,
                amount: rec.amount,
                volume: rec.volume,
                reserved: 0,
            });
        } else if let Some(ref mut cur) = current {
            cur.high = cur.high.max(rec.high);
            cur.low = cur.low.min(rec.low);
            cur.close = rec.close;
            cur.volume += rec.volume;
            cur.amount += rec.amount;
        }
    }

    if let Some(prev) = current.take() {
        result.push(prev);
    }

    result
}

fn write_min_file(
    vipdoc: &Path,
    market: &str,
    code: &str,
    subdir: &str,
    ext: &str,
    records: &[MinRecord],
) -> Result<()> {
    let dir = vipdoc.join(market).join(subdir);
    std::fs::create_dir_all(&dir)?;

    let filepath = dir.join(format!("{}{}{}", market, code, ext));

    let mut f = BufWriter::new(std::fs::File::create(&filepath)?);
    for rec in records {
        rec.write_to(&mut f)?;
    }

    debug!("写入分钟线: {} ({} 条记录)", filepath.display(), records.len());
    Ok(())
}

pub fn del_min(vipdoc: &Path, start_date: &str, end_date: &str) -> Result<()> {
    let start: u32 = start_date
        .parse()
        .with_context(|| format!("无效的开始日期: {}", start_date))?;
    let end: u32 = end_date
        .parse()
        .with_context(|| format!("无效的结束日期: {}", end_date))?;

    for market in MARKETS {
        for subdir in &["minline", "fzline"] {
            let ext = if *subdir == "minline" { ".01" } else { ".5" };
            let dir = vipdoc.join(market).join(subdir);
            if !dir.exists() {
                continue;
            }

            let entries = std::fs::read_dir(&dir)?;
            for entry in entries.flatten() {
                let path = entry.path();
                if !path
                    .extension()
                    .map(|e| {
                        let e = e.to_string_lossy();
                        e == ext.trim_start_matches('.')
                    })
                    .unwrap_or(false)
                {
                    continue;
                }

                let file_data = std::fs::read(&path)?;
                let num_records = file_data.len() / 32;
                let mut kept = Vec::new();

                for i in 0..num_records {
                    let offset = i * 32;
                    let mut cursor = std::io::Cursor::new(&file_data[offset..offset + 32]);
                    let rec = MinRecord::read_from(&mut cursor)?;
                    let (y, m, d) = MinRecord::decode_date(rec.date_raw);
                    let date_val = y * 10000 + m * 100 + d;
                    if date_val < start || date_val > end {
                        kept.push(rec);
                    }
                }

                if kept.is_empty() {
                    std::fs::remove_file(&path)?;
                } else {
                    let mut f = BufWriter::new(std::fs::File::create(&path)?);
                    for rec in &kept {
                        rec.write_to(&mut f)?;
                    }
                }
            }
        }
    }

    println!("分钟线数据删除完成: {} - {}", start_date, end_date);
    Ok(())
}
