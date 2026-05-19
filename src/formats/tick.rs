use std::io::{self, Read};
use byteorder::{LittleEndian, ReadBytesExt};

#[derive(Debug, Clone)]
pub struct TickItem {
    pub datetime: u32,
    pub count: u16,
    pub vol_offset: u16,
    pub vol_size: u16,
    pub tick_type: u16,
    pub price: u32,
    pub volume: u32,
}

impl TickItem {
    pub fn read_header<R: Read>(mut r: R) -> io::Result<Self> {
        Ok(TickItem {
            datetime: r.read_u32::<LittleEndian>()?,
            count: r.read_u16::<LittleEndian>()?,
            vol_offset: r.read_u16::<LittleEndian>()?,
            vol_size: r.read_u16::<LittleEndian>()?,
            tick_type: r.read_u16::<LittleEndian>()?,
            price: r.read_u32::<LittleEndian>()?,
            volume: r.read_u32::<LittleEndian>()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct StockTickHeader {
    pub market: u8,
    pub code: String,
    pub date: u32,
    pub tick_size: u32,
    pub unknown: u32,
}

impl StockTickHeader {
    pub fn read_from<R: Read>(mut r: R) -> io::Result<Self> {
        let market = r.read_u8()?;
        let mut code_buf = [0u8; 7];
        r.read_exact(&mut code_buf)?;
        let code = String::from_utf8_lossy(&code_buf)
            .trim_end_matches('\0')
            .to_string();
        let date = r.read_u32::<LittleEndian>()?;
        let tick_size = r.read_u32::<LittleEndian>()?;
        let unknown = r.read_u32::<LittleEndian>()?;
        Ok(StockTickHeader {
            market,
            code,
            date,
            tick_size,
            unknown,
        })
    }
}
