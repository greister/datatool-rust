pub mod day;
pub mod hashtable;
pub mod min;
pub mod tick;

use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DayRecord {
    pub date: u32,
    pub open: u32,
    pub high: u32,
    pub low: u32,
    pub close: u32,
    pub amount: f32,
    pub volume: u32,
    pub reserved: u32,
}

impl DayRecord {
    pub const SIZE: usize = 32;

    pub fn read_from<R: Read>(mut r: R) -> io::Result<Self> {
        Ok(DayRecord {
            date: r.read_u32::<LittleEndian>()?,
            open: r.read_u32::<LittleEndian>()?,
            high: r.read_u32::<LittleEndian>()?,
            low: r.read_u32::<LittleEndian>()?,
            close: r.read_u32::<LittleEndian>()?,
            amount: r.read_f32::<LittleEndian>()?,
            volume: r.read_u32::<LittleEndian>()?,
            reserved: r.read_u32::<LittleEndian>()?,
        })
    }

    pub fn write_to<W: Write>(&self, mut w: W) -> io::Result<()> {
        w.write_u32::<LittleEndian>(self.date)?;
        w.write_u32::<LittleEndian>(self.open)?;
        w.write_u32::<LittleEndian>(self.high)?;
        w.write_u32::<LittleEndian>(self.low)?;
        w.write_u32::<LittleEndian>(self.close)?;
        w.write_f32::<LittleEndian>(self.amount)?;
        w.write_u32::<LittleEndian>(self.volume)?;
        w.write_u32::<LittleEndian>(self.reserved)?;
        Ok(())
    }

    pub fn as_bytes(&self) -> io::Result<[u8; 32]> {
        let mut buf = Vec::with_capacity(32);
        self.write_to(&mut buf)?;
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&buf);
        Ok(arr)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MinRecord {
    pub date_raw: u16,
    pub time_raw: u16,
    pub open: u32,
    pub high: u32,
    pub low: u32,
    pub close: u32,
    pub amount: f32,
    pub volume: u32,
    pub reserved: u32,
}

impl MinRecord {
    pub const SIZE: usize = 32;

    pub fn read_from<R: Read>(mut r: R) -> io::Result<Self> {
        Ok(MinRecord {
            date_raw: r.read_u16::<LittleEndian>()?,
            time_raw: r.read_u16::<LittleEndian>()?,
            open: r.read_u32::<LittleEndian>()?,
            high: r.read_u32::<LittleEndian>()?,
            low: r.read_u32::<LittleEndian>()?,
            close: r.read_u32::<LittleEndian>()?,
            amount: r.read_f32::<LittleEndian>()?,
            volume: r.read_u32::<LittleEndian>()?,
            reserved: r.read_u32::<LittleEndian>()?,
        })
    }

    pub fn write_to<W: Write>(&self, mut w: W) -> io::Result<()> {
        w.write_u16::<LittleEndian>(self.date_raw)?;
        w.write_u16::<LittleEndian>(self.time_raw)?;
        w.write_u32::<LittleEndian>(self.open)?;
        w.write_u32::<LittleEndian>(self.high)?;
        w.write_u32::<LittleEndian>(self.low)?;
        w.write_u32::<LittleEndian>(self.close)?;
        w.write_f32::<LittleEndian>(self.amount)?;
        w.write_u32::<LittleEndian>(self.volume)?;
        w.write_u32::<LittleEndian>(self.reserved)?;
        Ok(())
    }

    pub fn decode_date(date_raw: u16) -> (u32, u32, u32) {
        let v = date_raw as u32;
        let year = v / 2048 + 2004;
        let rem = v % 2048;
        let month = rem / 100;
        let day = rem % 100;
        (year, month, day)
    }

    pub fn encode_date(year: u32, month: u32, day: u32) -> u16 {
        ((year - 2004) * 2048 + month * 100 + day) as u16
    }

    pub fn decode_time(time_raw: u16) -> (u32, u32) {
        let total_minutes = time_raw as u32;
        if total_minutes <= 120 {
            let actual = 570 + total_minutes;
            (actual / 60, actual % 60)
        } else {
            let actual = 660 + total_minutes;
            (actual / 60, actual % 60)
        }
    }

    pub fn encode_time(hour: u32, minute: u32) -> u16 {
        let total = hour * 60 + minute;
        if total >= 570 && total < 690 {
            (total - 570) as u16
        } else if total >= 780 && total < 900 {
            (total - 660) as u16
        } else {
            total as u16
        }
    }
}

pub const MARKETS: &[&str] = &["sh", "sz", "bj"];

pub fn market_for_code(code: &str) -> &'static str {
    if code.starts_with('6') || code.starts_with('9') {
        "sh"
    } else if code.starts_with('0') || code.starts_with('3') {
        "sz"
    } else if code.starts_with('4') || code.starts_with('8') {
        "bj"
    } else {
        "sz"
    }
}
