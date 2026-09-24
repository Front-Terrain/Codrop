#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamHash64 {
    state: u64,
    total_len: u64,
    buf: [u8; 8],
    buf_len: usize,
}

const PRIME1: u64 = 0x9E3779B185EBCA87;
const PRIME2: u64 = 0xC2B2AE3D27D4EB4F;
const PRIME3: u64 = 0x165667B19E3779F9;

impl StreamHash64 {
    pub fn new() -> Self {
        Self {
            state: PRIME1,
            total_len: 0,
            buf: [0u8; 8],
            buf_len: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total_len += data.len() as u64;

        // If we have buffered leftover bytes, fill the buffer first
        if self.buf_len > 0 {
            let needed = 8 - self.buf_len;
            let to_copy = data.len().min(needed);
            self.buf[self.buf_len..self.buf_len + to_copy].copy_from_slice(&data[..to_copy]);
            self.buf_len += to_copy;
            data = &data[to_copy..];

            if self.buf_len == 8 {
                let val = u64::from_le_bytes(self.buf);
                self.state = self.state.wrapping_add(val.wrapping_mul(PRIME2));
                self.state = self.state.rotate_left(31).wrapping_mul(PRIME1);
                self.buf_len = 0;
            }
        }

        // Process full 8-byte words directly from data
        while data.len() >= 8 {
            let val = u64::from_le_bytes(data[..8].try_into().unwrap());
            self.state = self.state.wrapping_add(val.wrapping_mul(PRIME2));
            self.state = self.state.rotate_left(31).wrapping_mul(PRIME1);
            data = &data[8..];
        }

        // Buffer any remaining tail bytes (< 8)
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub fn finalize(&self) -> u64 {
        let mut st = self.state;
        for i in 0..self.buf_len {
            st = st.wrapping_add((self.buf[i] as u64).wrapping_mul(PRIME3));
            st = st.rotate_left(11).wrapping_mul(PRIME1);
        }

        let mut h = st ^ (self.total_len.wrapping_mul(PRIME3));
        h ^= h >> 33;
        h = h.wrapping_mul(PRIME2);
        h ^= h >> 29;
        h = h.wrapping_mul(PRIME1);
        h ^= h >> 32;
        h
    }

    pub fn compute(data: &[u8]) -> u64 {
        let mut hasher = Self::new();
        hasher.update(data);
        hasher.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_hash_deterministic() {
        let data = b"The quick brown fox jumps over the lazy dog";
        let h1 = StreamHash64::compute(data);
        let h2 = StreamHash64::compute(data);
        assert_eq!(h1, h2);

        // Incremental vs single-pass
        let mut incremental = StreamHash64::new();
        incremental.update(&data[..10]);
        incremental.update(&data[10..]);
        assert_eq!(h1, incremental.finalize());
    }
}
