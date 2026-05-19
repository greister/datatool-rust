use std::io::{self, Read};
use byteorder::{LittleEndian, ReadBytesExt};

pub const COD_RECORD_SIZE: usize = 150;
pub const MD1_BLOCK_SIZE: usize = 512;

#[derive(Debug)]
pub struct CodRecord {
    pub stock_code: String,
    pub seq_num: u16,
}

impl CodRecord {
    pub fn read_from<R: Read>(mut r: R) -> io::Result<Self> {
        let mut code_buf = [0u8; 6];
        r.read_exact(&mut code_buf)?;
        let mut skip = [0u8; 26];
        r.read_exact(&mut skip)?;
        let seq_num = r.read_u16::<LittleEndian>()?;
        let mut tail = [0u8; COD_RECORD_SIZE - 6 - 26 - 2];
        r.read_exact(&mut tail)?;

        let stock_code = String::from_utf8_lossy(&code_buf)
            .trim_end_matches('\0')
            .to_string();

        Ok(CodRecord { stock_code, seq_num })
    }
}

#[derive(Debug)]
pub struct Md1Block {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u32,
    pub amount: f64,
}

impl Md1Block {
    pub fn read_from<R: Read>(mut r: R) -> io::Result<Self> {
        let mut header = [0u8; 12];
        r.read_exact(&mut header)?;

        let open = r.read_f64::<LittleEndian>()?;
        let high = r.read_f64::<LittleEndian>()?;
        let low = r.read_f64::<LittleEndian>()?;
        let close = r.read_f64::<LittleEndian>()?;

        let mut skip1 = [0u8; 16];
        r.read_exact(&mut skip1)?;

        let volume = r.read_u32::<LittleEndian>()?;

        let mut skip2 = [0u8; 12];
        r.read_exact(&mut skip2)?;

        let amount = r.read_f64::<LittleEndian>()?;

        let mut tail = vec![0u8; MD1_BLOCK_SIZE - 12 - 32 - 16 - 4 - 12 - 8];
        r.read_exact(&mut tail)?;

        Ok(Md1Block {
            open,
            high,
            low,
            close,
            volume,
            amount,
        })
    }
}
