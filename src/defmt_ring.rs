pub struct DefmtRing<const N: usize> {
    buf: [u8; N],
    head: usize,
    len: usize,
    dropped_bytes: usize,
}

impl<const N: usize> DefmtRing<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            head: 0,
            len: 0,
            dropped_bytes: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn dropped_bytes(&self) -> usize {
        self.dropped_bytes
    }

    pub fn push_slice(&mut self, bytes: &[u8]) -> usize {
        let available = N.saturating_sub(self.len);
        let written = available.min(bytes.len());

        let mut index = 0usize;
        while index < written {
            let tail = (self.head + self.len) % N;
            self.buf[tail] = bytes[index];
            self.len += 1;
            index += 1;
        }

        self.dropped_bytes += bytes.len().saturating_sub(written);
        written
    }

    pub fn pop_into(&mut self, out: &mut [u8]) -> usize {
        let read = self.len.min(out.len());

        let mut index = 0usize;
        while index < read {
            out[index] = self.buf[self.head];
            self.head = (self.head + 1) % N;
            self.len -= 1;
            index += 1;
        }

        read
    }
}

impl<const N: usize> Default for DefmtRing<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::DefmtRing;

    #[test]
    fn pops_bytes_in_fifo_order() {
        let mut ring = DefmtRing::<8>::new();
        assert_eq!(ring.push_slice(b"abcd"), 4);

        let mut out = [0u8; 4];
        assert_eq!(ring.pop_into(&mut out), 4);
        assert_eq!(&out, b"abcd");
        assert!(ring.is_empty());
    }

    #[test]
    fn partial_pop_leaves_remaining_bytes_queued() {
        let mut ring = DefmtRing::<8>::new();
        assert_eq!(ring.push_slice(b"abcdef"), 6);

        let mut first = [0u8; 2];
        assert_eq!(ring.pop_into(&mut first), 2);
        assert_eq!(&first, b"ab");
        assert_eq!(ring.len(), 4);

        let mut second = [0u8; 4];
        assert_eq!(ring.pop_into(&mut second), 4);
        assert_eq!(&second, b"cdef");
        assert!(ring.is_empty());
    }

    #[test]
    fn overflow_drops_new_bytes_without_corrupting_existing_data() {
        let mut ring = DefmtRing::<4>::new();
        assert_eq!(ring.push_slice(b"abcd"), 4);
        assert_eq!(ring.push_slice(b"ef"), 0);
        assert_eq!(ring.dropped_bytes(), 2);

        let mut out = [0u8; 4];
        assert_eq!(ring.pop_into(&mut out), 4);
        assert_eq!(&out, b"abcd");
        assert!(ring.is_empty());
    }
}
