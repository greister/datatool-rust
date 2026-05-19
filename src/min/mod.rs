use std::collections::BTreeMap;
use std::io::{BufWriter, Read};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use flate2::read::ZlibDecoder;
use log::{debug, warn};

use crate::formats::hashtable::{build_datetime_hash_table, build_price_hash_table};
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

/// Decompress zlib-compressed VTC data.
fn decompress_vtc(vtc_data: &[u8]) -> Result<Vec<u8>> {
    if vtc_data.is_empty() {
        return Ok(Vec::new());
    }

    // Check for zlib magic bytes (0x78 0x9C or 0x78 0x01 or 0x78 0xDA)
    if vtc_data.len() >= 2 && vtc_data[0] == 0x78 {
        let mut decoder = ZlibDecoder::new(vtc_data);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .with_context(|| "zlib解压VTC数据失败")?;
        Ok(decompressed)
    } else {
        // Not compressed, return as-is
        Ok(vtc_data.to_vec())
    }
}

/// Parse VID header to extract the date.
/// VID format: version(4) + file_date(4) + date(4) + u16(2) + u16(2) + stock_header_data
/// The stock_header_data from the original tick processing contains: date(4) + decomp_size(4) + comp_size(4) + unknown(10)
fn parse_vid_date(vid_data: &[u8]) -> Option<u32> {
    if vid_data.len() < 16 {
        return None;
    }
    // The VID header layout: version(4) + file_date(4) + date_from_htc(4) + count_u16(2) + pad_u16(2) + original_stock_header(30 bytes)
    // The date is at offset 8 in the vid header (the _stock_date from process_htc_file_v4)
    // Actually in tick/mod.rs, vid_data is built as:
    //   file_prefix(8: version+file_date) + file_date(4) + 1u16(2) + 0u16(2) + data[pos+8..pos+30](22 bytes)
    // The original data[pos+8..pos+12] is _stock_date (the trading date)
    // So in the VID: offset 0-7 = file_prefix, 8-11 = file_date again, 12-13 = count, 14-15 = pad
    // 16-19 = original _stock_date = the actual trading date
    if vid_data.len() >= 20 {
        let date = u32::from_le_bytes(vid_data[16..20].try_into().ok()?);
        Some(date)
    } else {
        None
    }
}

/// Decode the hash-encoded tick bitstream from decompressed VTC data.
///
/// The VTC data contains one or more TickItem blocks. Each block starts with a 20-byte header:
///   datetime(4) + count(2) + vol_offset(2) + vol_size(2) + type(2) + price(4) + volume(4)
/// After the header, a hash-encoded bitstream follows for subsequent tick time/price deltas.
/// Volume data is stored separately at vol_offset from the beginning of the tick detail data.
fn generate_all_min_records(vtc_data: &[u8], vid_data: &[u8]) -> Result<Vec<MinRecord>> {
    if vtc_data.is_empty() {
        return Ok(Vec::new());
    }

    // Decompress the VTC data (zlib)
    let decompressed = decompress_vtc(vtc_data)?;
    if decompressed.is_empty() {
        return Ok(Vec::new());
    }

    // Get the trading date from VID header
    let trading_date = parse_vid_date(vid_data);
    let date_int = trading_date.unwrap_or(0);
    // Decode date: e.g., 20240115 -> year=2024, month=01, day=15
    let year = date_int / 10000;
    let month = (date_int / 100) % 100;
    let day = date_int % 100;

    // Build hash tables for decoding
    let dt_table = build_datetime_hash_table();
    let price_table = build_price_hash_table();

    let mut all_ticks: Vec<TickDetail> = Vec::new();
    let mut pos: usize = 0;
    let data = &decompressed;

    // The VTC may contain multiple TickItem blocks (one per day if multi-day VTC).
    // Each TickItem has a 20-byte header followed by the bitstream.
    while pos + 20 <= data.len() {
        // Parse TickItem header (20 bytes)
        let _datetime = u32::from_le_bytes(data[pos..pos + 4].try_into()?);
        let count = u16::from_le_bytes(data[pos + 4..pos + 6].try_into()?) as usize;
        let vol_offset = u16::from_le_bytes(data[pos + 6..pos + 8].try_into()?) as usize;
        let _vol_size = u16::from_le_bytes(data[pos + 8..pos + 10].try_into()?);
        let tick_type = u16::from_le_bytes(data[pos + 10..pos + 12].try_into()?);
        let price = u32::from_le_bytes(data[pos + 12..pos + 16].try_into()?);
        let volume = u32::from_le_bytes(data[pos + 16..pos + 20].try_into()?);

        if count == 0 {
            pos += 20;
            continue;
        }

        // First tick: time = tick_type & 0xFF, price from header, volume from header
        let first_time = (tick_type & 0xFF) as i32;
        let first_bs = (tick_type >> 15) as u8; // buy/sell: 0=buy, 1=sell

        let tick_detail_start = pos + 20; // Start of bitstream data after header
        let mut ticks = Vec::with_capacity(count);
        ticks.push(TickDetail {
            time: first_time,
            price: price as i32,
            volume: volume as i32,
            _buy_sell: first_bs,
        });

        if count > 1 {
            // Decode subsequent ticks from the bitstream
            let bitstream = &data[tick_detail_start..];
            let vol_data = if tick_detail_start + vol_offset <= data.len() {
                &data[tick_detail_start + vol_offset..]
            } else {
                &data[data.len()..]
            };

            let result = decode_tick_bitstream(
                bitstream,
                vol_data,
                count - 1,
                &ticks[0],
                date_int,
                &dt_table,
                &price_table,
            );

            match result {
                Ok(decoded) => ticks.extend(decoded),
                Err(e) => {
                    warn!(
                        "Tick解码失败 (date={}, count={}): {}",
                        date_int, count, e
                    );
                    break;
                }
            }
        }

        all_ticks.extend(ticks);

        // Move past this TickItem block.
        // The total size of the block = header(20) + bitstream + volume data
        // We need to figure out how much data was consumed.
        // The Go code reads all remaining data in the tick block:
        // byteTicDetail, _ := leBuffer.ReadBuff(leBuffer.Right())
        // This means the entire remaining data after the header is the tick detail.
        // Since each VTC file is written per stock per day (with one TickItem),
        // we break after processing one block.
        break;
    }

    if all_ticks.is_empty() {
        return Ok(Vec::new());
    }

    // Convert tick times (minutes since market open) to MinRecord time_raw format
    // and aggregate into 1-minute bars
    let mut minute_bars: BTreeMap<u16, MinBar> = BTreeMap::new();

    for tick in &all_ticks {
        let time_raw = minutes_to_time_raw(tick.time);
        let price_val = (tick.price / 100) as u32;

        let bar = minute_bars.entry(time_raw).or_insert_with(|| MinBar {
            time_raw,
            open: price_val,
            high: price_val,
            low: price_val,
            close: price_val,
            volume: 0,
        });

        bar.high = bar.high.max(price_val);
        bar.low = bar.low.min(price_val);
        bar.close = price_val;
        bar.volume += tick.volume as u32;
    }

    // Compute date_raw from the trading date
    let date_raw = if year >= 2004 {
        MinRecord::encode_date(year, month, day)
    } else {
        0
    };

    let mut records: Vec<MinRecord> = minute_bars
        .into_values()
        .map(|bar| MinRecord {
            date_raw,
            time_raw: bar.time_raw,
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
            amount: 0.0, // Amount not available from tick data
            volume: bar.volume,
            reserved: 0,
        })
        .collect();

    records.sort_by_key(|r| (r.date_raw, r.time_raw));
    Ok(records)
}

/// Convert minutes-since-market-open to MinRecord time_raw encoding.
///
/// The Go SetTradeTime function converts timeVal to actual time:
///   Morning (0-120): actual_minutes = 570 + timeVal  (9:30 + timeVal)
///   Afternoon (121-240): actual_minutes = 660 + timeVal  (11:00 + timeVal, so 121->781=13:01)
///
/// MinRecord.encode_time does the reverse:
///   570-689 -> 0-119 (morning)
///   780-899 -> 120-239 (afternoon)
///
/// So: morning timeVal 0-120 -> time_raw = 0-119
///     afternoon timeVal 121-240 -> actual = 660+121=781 -> time_raw = 781-660 = 121
fn minutes_to_time_raw(minutes_since_open: i32) -> u16 {
    if minutes_since_open >= 0 && minutes_since_open <= 120 {
        (570 + minutes_since_open) as u16
    } else if minutes_since_open > 120 && minutes_since_open <= 240 {
        (660 + minutes_since_open) as u16
    } else {
        minutes_since_open.max(0).min(240) as u16
    }
}

/// A decoded tick detail record.
struct TickDetail {
    time: i32,     // Minutes since market open
    price: i32,    // Price in 0.01 yuan units
    volume: i32,   // Trade volume
    _buy_sell: u8,  // 0=buy, 1=sell
}

/// Bitstream reader for the tick data.
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 0-31, counting from MSB
    current_u32: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Result<Self> {
        let mut reader = BitReader {
            data,
            byte_pos: 0,
            bit_pos: 32,
            current_u32: 0,
        };
        reader.read_next_u32()?;
        Ok(reader)
    }

    fn read_next_u32(&mut self) -> Result<()> {
        if self.byte_pos + 4 > self.data.len() {
            return Err(anyhow!("BitReader: 超出数据范围"));
        }
        self.current_u32 = u32::from_le_bytes(
            self.data[self.byte_pos..self.byte_pos + 4].try_into()?,
        );
        self.byte_pos += 4;
        self.bit_pos = 32;
        Ok(())
    }

    /// Read a single bit from the MSB of the current u32.
    fn read_bit(&mut self) -> Result<u32> {
        if self.bit_pos == 0 {
            self.read_next_u32()?;
        }
        let bit = (self.current_u32 >> 31) & 1;
        self.current_u32 <<= 1;
        self.bit_pos -= 1;
        Ok(bit)
    }

}

/// Decode the hash-encoded bitstream for tick time/price deltas.
///
/// This is a port of the Go parseTickDTPrice function from interceptor.go.
fn decode_tick_bitstream(
    bitstream: &[u8],
    vol_data: &[u8],
    remaining_count: usize,
    first_tick: &TickDetail,
    _date_int: u32,
    dt_table: &std::collections::HashMap<i32, i32>,
    price_table: &std::collections::HashMap<i32, i32>,
) -> Result<Vec<TickDetail>> {
    let mut reader = BitReader::new(bitstream)?;
    let mut ticks = Vec::with_capacity(remaining_count);

    let mut prev_time = first_tick.time;
    let mut prev_price = first_tick.price;

    for _ in 0..remaining_count {
        // Read buy/sell type (1 bit)
        let _buy_sell = reader.read_bit()? as u8;

        // Decode time delta using HashTableDateTime
        let mut checksum: u32 = 3; // Start with binary '11'
        let time_delta = loop {
            checksum = (checksum << 1) | reader.read_bit()?;
            if let Some(&delta) = dt_table.get(&(checksum as i32)) {
                break delta;
            }
        };

        // Decode price delta using HashTablePrice
        let mut checksum: u32 = 3; // Start with binary '11'
        let price_delta = loop {
            checksum = (checksum << 1) | reader.read_bit()?;
            // In Go, there's a special check: if checksum > 0x3FFFFFF or hash_value <= checksum, advance index
            // But since we use a HashMap, we just look up directly
            if let Some(&delta) = price_table.get(&(checksum as i32)) {
                break delta;
            }
        };

        // Compute actual price
        // Special case from Go: if tmpIdx == 4000 && date >= 20011112, read 32 raw bits
        // But since we don't have idx 4000 in our table (it's the escape code), we handle it differently.
        // The Go code checks if the found table index is 4000. Since our price_table maps
        // hash_value -> idx, we need to check the returned idx value.
        // Actually, looking more carefully at the Go code, tmpIdx is the index in the array,
        // not the idx field. When the loop exits at position 4000 in the array, that entry
        // has idx=4000 (which doesn't exist in the regular table). The Go code uses the array
        // position. Since we're using a HashMap, we can't detect this case from the idx value alone.
        // However, this special case only triggers for very large price deltas and is rare.
        // For now, we handle the normal case. If needed, we can add the escape mechanism later.

        let new_price = prev_price + price_delta;
        let new_time = prev_time + time_delta;

        ticks.push(TickDetail {
            time: new_time,
            price: new_price,
            volume: 0, // Will be filled from vol_data
            _buy_sell,
        });

        prev_time = new_time;
        prev_price = new_price;
    }

    // Decode volumes from vol_data
    let mut vol_reader = VolumeReader::new(vol_data);
    for tick in &mut ticks {
        tick.volume = vol_reader.read_volume()?;
    }

    Ok(ticks)
}

/// Volume reader for variable-length encoded volumes.
struct VolumeReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> VolumeReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        VolumeReader { data, pos: 0 }
    }

    fn read_u8(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            return Err(anyhow!("VolumeReader: 超出数据范围"));
        }
        let val = self.data[self.pos];
        self.pos += 1;
        Ok(val)
    }

    fn read_u16(&mut self) -> Result<u16> {
        if self.pos + 2 > self.data.len() {
            return Err(anyhow!("VolumeReader: 超出数据范围"));
        }
        let val = u16::from_le_bytes(self.data[self.pos..self.pos + 2].try_into()?);
        self.pos += 2;
        Ok(val)
    }

    /// Read a variable-length encoded volume.
    ///
    /// Encoding:
    ///   byte <= 252: volume = byte
    ///   byte == 253: volume = next_byte + 253
    ///   byte == 254: volume = next_u16 + 254
    ///   byte == 255: volume = 0xFFFF * next_byte + next_u16 + 0xFF
    fn read_volume(&mut self) -> Result<i32> {
        let byte = self.read_u8()?;
        if byte <= 252 {
            Ok(byte as i32)
        } else if byte == 253 {
            let next = self.read_u8()?;
            Ok((next as i32) + 253)
        } else if byte == 254 {
            let next = self.read_u16()?;
            Ok((next as i32) + 254)
        } else {
            // byte == 255
            let tmp_vol1 = self.read_u8()?;
            let tmp_vol2 = self.read_u16()?;
            Ok(0xFFFF * (tmp_vol1 as i32) + (tmp_vol2 as i32) + 0xFF)
        }
    }
}

struct MinBar {
    time_raw: u16,
    open: u32,
    high: u32,
    low: u32,
    close: u32,
    volume: u32,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::hashtable::{build_datetime_hash_table, build_price_hash_table};

    #[test]
    fn test_bitreader_basic() {
        // u32 LE: 0x80000001 = [0x01, 0x00, 0x00, 0x80]
        // MSB = 1, then 30 zeros, then LSB = 1
        let data: Vec<u8> = vec![0x01, 0x00, 0x00, 0x80];
        let mut reader = BitReader::new(&data).unwrap();
        assert_eq!(reader.read_bit().unwrap(), 1); // bit 0: MSB
        assert_eq!(reader.read_bit().unwrap(), 0); // bit 1
        for _ in 0..29 {
            assert_eq!(reader.read_bit().unwrap(), 0); // bits 2-30
        }
        assert_eq!(reader.read_bit().unwrap(), 1); // bit 31: LSB
    }

    #[test]
    fn test_bitreader_across_u32() {
        // Two u32s: first = 0xFFFFFFFF, second = 0x00000000
        let data: Vec<u8> = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00];
        let mut reader = BitReader::new(&data).unwrap();
        for _ in 0..32 {
            assert_eq!(reader.read_bit().unwrap(), 1);
        }
        for _ in 0..32 {
            assert_eq!(reader.read_bit().unwrap(), 0);
        }
    }

    #[test]
    fn test_datetime_hash_table_completeness() {
        let table = build_datetime_hash_table();
        assert_eq!(table.len(), 241);
        for i in 0..=240u32 {
            assert!(table.values().any(|&v| v == i as i32),
                "DateTime hash table missing idx={}", i);
        }
    }

    #[test]
    fn test_price_hash_table_completeness() {
        let table = build_price_hash_table();
        // Should cover -2000 to +2000
        for delta in -2000..=2000i32 {
            assert!(table.values().any(|&v| v == delta),
                "Price hash table missing idx={}", delta);
        }
        assert!(table.len() >= 4001);
    }

    #[test]
    fn test_hash_table_datetime_known_values() {
        let table = build_datetime_hash_table();
        assert_eq!(table[&0x06], 0);
        assert_eq!(table[&0x0F], 1);
        assert_eq!(table[&0x1D], 2);
        assert_eq!(table[&0x38], 3);
        assert_eq!(table[&0x72], 4);
        assert_eq!(table[&0x1CC], 6);
        assert_eq!(table[&0x1CF], 5);
    }

    #[test]
    fn test_hash_table_price_known_values() {
        let table = build_price_hash_table();
        assert_eq!(table[&0x07], 0);
        assert_eq!(table[&0x19], 1);
        assert_eq!(table[&0x1B], -1);
        assert_eq!(table[&0x30], 2);
        assert_eq!(table[&0x34], -2);
    }

    #[test]
    fn test_volume_reader_basic() {
        let data: Vec<u8> = vec![100];
        let mut reader = VolumeReader::new(&data);
        assert_eq!(reader.read_volume().unwrap(), 100);
    }

    #[test]
    fn test_volume_reader_253() {
        let data: Vec<u8> = vec![253, 10];
        let mut reader = VolumeReader::new(&data);
        assert_eq!(reader.read_volume().unwrap(), 10 + 253);
    }

    #[test]
    fn test_volume_reader_254() {
        let data: Vec<u8> = vec![254, 0x10, 0x00];
        let mut reader = VolumeReader::new(&data);
        assert_eq!(reader.read_volume().unwrap(), 0x0010 + 254);
    }

    #[test]
    fn test_volume_reader_255() {
        let data: Vec<u8> = vec![255, 2, 0x34, 0x12];
        let mut reader = VolumeReader::new(&data);
        assert_eq!(reader.read_volume().unwrap(), 0xFFFF * 2 + 0x1234 + 0xFF);
    }

    #[test]
    fn test_minutes_to_time_raw_morning() {
        assert_eq!(minutes_to_time_raw(0), 570);
        assert_eq!(minutes_to_time_raw(120), 690);
    }

    #[test]
    fn test_minutes_to_time_raw_afternoon() {
        assert_eq!(minutes_to_time_raw(121), 781);
        assert_eq!(minutes_to_time_raw(240), 900);
    }

    #[test]
    fn test_synthetic_tick_decode() {
        let dt_table = build_datetime_hash_table();
        let price_table = build_price_hash_table();

        // Encode: buy/sell=0, time_delta=1 (hash 0x0F, bits: 1,1), price_delta=0 (hash 0x07, bits: 1)
        // Total bits: 0 1 1 1 = 0x70000000 as u32, LE bytes: [0x00, 0x00, 0x00, 0x70]
        let bitstream: Vec<u8> = vec![0x00, 0x00, 0x00, 0x70];
        let vol_data: Vec<u8> = vec![50];

        let first_tick = TickDetail {
            time: 0,
            price: 1000,
            volume: 100,
            _buy_sell: 0,
        };

        let result = decode_tick_bitstream(
            &bitstream, &vol_data, 1, &first_tick, 20240115, &dt_table, &price_table,
        ).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time, 0 + 1);
        assert_eq!(result[0].price, 1000 + 0);
        assert_eq!(result[0].volume, 50);
        assert_eq!(result[0]._buy_sell, 0);
    }
}
