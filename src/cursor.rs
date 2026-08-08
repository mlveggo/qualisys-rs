//! Bounds-checked sequential reader for the QTM binary wire format.

use crate::error::{Error, Result};

/// Byte order of a connection.
///
/// QTM exposes a little-endian port and a big-endian port; which one you
/// connected to determines how every multi-byte field is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ByteOrder {
    #[default]
    Little,
    Big,
}

impl ByteOrder {
    #[inline]
    pub(crate) fn u16(self, b: [u8; 2]) -> u16 {
        match self {
            ByteOrder::Little => u16::from_le_bytes(b),
            ByteOrder::Big => u16::from_be_bytes(b),
        }
    }

    #[inline]
    pub(crate) fn u32(self, b: [u8; 4]) -> u32 {
        match self {
            ByteOrder::Little => u32::from_le_bytes(b),
            ByteOrder::Big => u32::from_be_bytes(b),
        }
    }

    #[inline]
    pub(crate) fn u64(self, b: [u8; 8]) -> u64 {
        match self {
            ByteOrder::Little => u64::from_le_bytes(b),
            ByteOrder::Big => u64::from_be_bytes(b),
        }
    }

    #[inline]
    pub(crate) fn put_u32(self, value: u32) -> [u8; 4] {
        match self {
            ByteOrder::Little => value.to_le_bytes(),
            ByteOrder::Big => value.to_be_bytes(),
        }
    }
}

/// A cursor over a byte slice that refuses to read past the end.
///
/// The QTM protocol is a dense binary format and decoding it is a long run of
/// fixed offsets. Doing that arithmetic by hand invites exactly the kind of
/// mistake that turns a malformed frame into a panic, so every read here is
/// checked and returns [`Error::ShortPacket`] rather than indexing out of
/// bounds.
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    order: ByteOrder,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8], order: ByteOrder) -> Self {
        Cursor {
            data,
            pos: 0,
            order,
        }
    }

    /// Bytes not yet consumed.
    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::ShortPacket {
            needed: n,
            offset: self.pos,
            available: self.remaining(),
        })?;
        if end > self.data.len() {
            return Err(Error::ShortPacket {
                needed: n,
                offset: self.pos,
                available: self.remaining(),
            });
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(self.order.u16([b[0], b[1]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(self.order.u32([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(self
            .order
            .u64([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    pub(crate) fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// Reads three consecutive `f32` as an X/Y/Z triple.
    pub(crate) fn point(&mut self) -> Result<crate::components::Point> {
        Ok(crate::components::Point {
            x: self.f32()?,
            y: self.f32()?,
            z: self.f32()?,
        })
    }

    /// Copies the next `n` bytes.
    ///
    /// A copy is deliberate: the protocol reuses one receive buffer for every
    /// frame, so borrowing would hand callers memory that the next frame
    /// overwrites underneath them.
    pub(crate) fn bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        Ok(self.take(n)?.to_vec())
    }

    /// Rejects a record count that could not possibly fit in what is left.
    ///
    /// A count field is read straight off the wire, so a corrupt or hostile
    /// frame could otherwise drive a multi-gigabyte allocation before a single
    /// byte is validated.
    pub(crate) fn check_count(&self, count: u32, bytes_each: usize) -> Result<()> {
        let needed = (count as u64).saturating_mul(bytes_each as u64);
        if needed > self.remaining() as u64 {
            return Err(Error::ShortPacket {
                needed: needed.min(usize::MAX as u64) as usize,
                offset: self.pos,
                available: self.remaining(),
            });
        }
        Ok(())
    }

    /// Reserves capacity for `count` records, having already validated the
    /// count against the remaining length.
    pub(crate) fn vec_with_capacity<T>(&self, count: u32) -> Vec<T> {
        Vec::with_capacity(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_fields() {
        let data = [0x01, 0x00, 0x00, 0x00, 0x02, 0x00];
        let mut c = Cursor::new(&data, ByteOrder::Little);
        assert_eq!(c.u32().unwrap(), 1);
        assert_eq!(c.u16().unwrap(), 2);
        assert_eq!(c.remaining(), 0);
    }

    #[test]
    fn reads_big_endian_fields() {
        let data = [0x00, 0x00, 0x00, 0x01];
        let mut c = Cursor::new(&data, ByteOrder::Big);
        assert_eq!(c.u32().unwrap(), 1);
    }

    #[test]
    fn short_read_is_an_error_not_a_panic() {
        let data = [0x01, 0x02];
        let mut c = Cursor::new(&data, ByteOrder::Little);
        assert!(matches!(c.u32(), Err(Error::ShortPacket { .. })));
    }

    #[test]
    fn bytes_returns_an_owned_copy() {
        let data = vec![1u8, 2, 3, 4];
        let mut c = Cursor::new(&data, ByteOrder::Little);
        let taken = c.bytes(4).unwrap();
        assert_eq!(taken, vec![1, 2, 3, 4]);
    }

    #[test]
    fn check_count_rejects_impossible_counts() {
        let data = [0u8; 8];
        let c = Cursor::new(&data, ByteOrder::Little);
        assert!(c.check_count(1_000_000, 32).is_err());
        assert!(c.check_count(2, 4).is_ok());
    }
}
