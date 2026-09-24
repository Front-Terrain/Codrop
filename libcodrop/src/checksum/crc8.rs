/// CRC-8 implementation using polynomial 0x07 (x^8 + x^2 + x + 1).
/// Used exclusively for verifying the Stream Header against corruption.
pub struct Crc8 {
    state: u8,
}

const CRC8_TABLE: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let mut curr = i as u8;
        let mut j = 0;
        while j < 8 {
            if (curr & 0x80) != 0 {
                curr = (curr << 1) ^ 0x07;
            } else {
                curr <<= 1;
            }
            j += 1;
        }
        table[i] = curr;
        i += 1;
    }
    table
};

impl Default for Crc8 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc8 {
    pub fn new() -> Self {
        Self { state: 0x00 }
    }

    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.state = CRC8_TABLE[(self.state ^ byte) as usize];
        }
    }

    pub fn finalize(self) -> u8 {
        self.state
    }

    pub fn compute(data: &[u8]) -> u8 {
        let mut crc = Self::new();
        crc.update(data);
        crc.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc8_basic() {
        let data = b"123456789";
        let c = Crc8::compute(data);
        // Standard check value for CRC-8/SMBus on "123456789" is 0xF4
        assert_eq!(c, 0xF4);
    }
}
