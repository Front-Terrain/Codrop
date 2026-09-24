/// CRC-32C (Castagnoli) implementation using polynomial 0x1EDC6F41 (reversed: 0x82F63B78).
/// Optimized table-driven implementation for high-speed block integrity checks.
pub struct Crc32c {
    state: u32,
}

const CRC32C_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let poly = 0x82F63B78;
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ poly;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

impl Crc32c {
    pub fn new() -> Self {
        Self { state: 0xFFFFFFFF }
    }

    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            let index = ((self.state ^ (byte as u32)) & 0xFF) as usize;
            self.state = (self.state >> 8) ^ CRC32C_TABLE[index];
        }
    }

    pub fn finalize(self) -> u32 {
        self.state ^ 0xFFFFFFFF
    }

    pub fn compute(data: &[u8]) -> u32 {
        let mut crc = Self::new();
        crc.update(data);
        crc.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32c_standard_check() {
        let data = b"123456789";
        let c = Crc32c::compute(data);
        // Standard check value for CRC-32C (Castagnoli) on "123456789" is 0xE3069283
        assert_eq!(c, 0xE3069283);
    }
}
