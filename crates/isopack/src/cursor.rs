use crate::types::Error;
use crate::types::Result;

/// A position marker in a stream that can be used to seek back
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    pub absolute_pos: u64,
}

/// A cursor tracks position within a borrowed buffer slice
/// Supports both mmap (fixed slice) and streaming (changing slices) use cases
#[derive(Debug, Clone)]
pub struct Cursor<'a> {
    slice: &'a [u8],
    pos: usize,
    base_offset: u64,
    min_valid_pos: usize,
}

impl<'a> Cursor<'a> {
    /// Create cursor at start of slice (mmap use case)
    pub fn new(slice: &'a [u8]) -> Self {
        Self {
            slice,
            pos: 0,
            base_offset: 0,
            min_valid_pos: 0,
        }
    }

    /// Create cursor with context (streaming use case)
    pub fn with_context(
        slice: &'a [u8],
        start_pos: usize,
        base_offset: u64,
        min_valid: usize,
    ) -> Self {
        Self {
            slice,
            pos: start_pos,
            base_offset,
            min_valid_pos: min_valid,
        }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn absolute_pos(&self) -> u64 {
        self.base_offset + self.pos as u64
    }

    pub fn remaining(&self) -> usize {
        self.slice.len().saturating_sub(self.pos)
    }

    pub fn mark(&self) -> Location {
        Location {
            absolute_pos: self.absolute_pos(),
        }
    }

    pub fn set_pos(&mut self, pos: usize) -> Result<()> {
        if pos < self.min_valid_pos {
            return Err(Error::PositionFreed);
        }
        if pos > self.slice.len() {
            return Err(Error::OutOfBounds);
        }
        self.pos = pos;
        Ok(())
    }

    pub fn seek(&mut self, loc: Location) -> Result<()> {
        if loc.absolute_pos < self.base_offset + self.min_valid_pos as u64 {
            return Err(Error::PositionFreed);
        }
        if loc.absolute_pos < self.base_offset {
            return Err(Error::SeekBeforeBuffer);
        }
        let relative_pos = (loc.absolute_pos - self.base_offset) as usize;
        if relative_pos > self.slice.len() {
            return Err(Error::SeekAfterBuffer);
        }
        self.pos = relative_pos;
        Ok(())
    }

    #[inline]
    pub(crate) fn need(&self, n: usize) -> Result<()> {
        if self.pos < self.min_valid_pos {
            return Err(Error::PositionFreed);
        }
        if self.pos + n > self.slice.len() {
            Err(Error::Pending(self.pos + n - self.slice.len()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn read_byte(&mut self) -> Result<u8> {
        self.need(1)?;
        let byte = self.slice[self.pos];
        self.pos += 1;
        Ok(byte)
    }

    pub(crate) fn peek_byte(&self) -> Result<u8> {
        self.need(1)?;
        Ok(self.slice[self.pos])
    }

    pub(crate) fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        self.need(len)?;
        let slice = &self.slice[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    pub(crate) fn skip(&mut self, len: usize) -> Result<()> {
        self.need(len)?;
        self.pos += len;
        Ok(())
    }

    pub fn as_slice(&self) -> &'a [u8] {
        &self.slice[self.pos..]
    }

    pub(crate) fn full_slice(&self) -> &'a [u8] {
        self.slice
    }
}

/// Helper for streaming use case - manages a growable buffer with compaction
pub struct StreamBuffer {
    pub data: Vec<u8>,
    pub base_offset: u64,
    pub valid_start: usize,
}

impl StreamBuffer {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            base_offset: 0,
            valid_start: 0,
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            data: Vec::with_capacity(cap),
            base_offset: 0,
            valid_start: 0,
        }
    }

    pub fn extend(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }

    pub fn cursor(&self) -> Cursor<'_> {
        Cursor::with_context(
            &self.data[self.valid_start..],
            0,
            self.base_offset + self.valid_start as u64,
            0,
        )
    }

    pub fn mark_consumed(&mut self, bytes_from_valid_start: usize) {
        self.valid_start += bytes_from_valid_start;
    }

    pub fn compact(&mut self) -> usize {
        let freed = self.valid_start;
        if freed > 0 {
            self.data.drain(..self.valid_start);
            self.base_offset += freed as u64;
            self.valid_start = 0;
        }
        freed
    }

    pub fn len(&self) -> usize {
        self.data.len() - self.valid_start
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_basic() {
        let data = b"hello world";
        let mut cursor = Cursor::new(data);
        
        assert_eq!(cursor.pos(), 0);
        assert_eq!(cursor.absolute_pos(), 0);
        assert_eq!(cursor.remaining(), 11);
        
        let bytes = cursor.read_bytes(5).unwrap();
        assert_eq!(bytes, b"hello");
        assert_eq!(cursor.pos(), 5);
        assert_eq!(cursor.absolute_pos(), 5);
    }

    #[test]
    fn cursor_mark_and_seek() {
        let data = b"0123456789";
        let mut cursor = Cursor::new(data);
        
        cursor.read_bytes(3).unwrap();
        let mark = cursor.mark();
        assert_eq!(mark.absolute_pos, 3);
        
        cursor.read_bytes(2).unwrap();
        assert_eq!(cursor.pos(), 5);
        
        cursor.seek(mark).unwrap();
        assert_eq!(cursor.pos(), 3);
        
        let remaining = cursor.read_bytes(7).unwrap();
        assert_eq!(remaining, b"3456789");
    }

    #[test]
    fn cursor_with_offset() {
        let data = b"abc";
        let cursor = Cursor::with_context(data, 0, 1000, 0);
        
        assert_eq!(cursor.absolute_pos(), 1000);
        assert_eq!(cursor.mark().absolute_pos, 1000);
    }

    #[test]
    fn cursor_pending() {
        let data = b"short";
        let mut cursor = Cursor::new(data);
        
        match cursor.read_bytes(10) {
            Err(Error::Pending(n)) => assert_eq!(n, 5),
            _ => panic!("Expected Pending error"),
        }
    }

    #[test]
    fn stream_buffer_basic() {
        let mut buffer = StreamBuffer::new();
        buffer.extend(b"hello");
        buffer.extend(b" world");
        
        let cursor = buffer.cursor();
        assert_eq!(cursor.remaining(), 11);
    }

    #[test]
    fn stream_buffer_compact() {
        let mut buffer = StreamBuffer::new();
        buffer.extend(b"0123456789");
        
        // Consume first 5 bytes
        buffer.mark_consumed(5);
        assert_eq!(buffer.valid_start, 5);
        
        // Compact
        let freed = buffer.compact();
        assert_eq!(freed, 5);
        assert_eq!(buffer.valid_start, 0);
        assert_eq!(buffer.data.len(), 5);
        assert_eq!(buffer.base_offset, 5);
        
        // Cursor should reflect new state
        let cursor = buffer.cursor();
        assert_eq!(cursor.absolute_pos(), 5);
        assert_eq!(cursor.remaining(), 5);
    }

    #[test]
    fn stream_buffer_workflow() {
        let mut buffer = StreamBuffer::new();
        
        // Add first chunk
        buffer.extend(b"message1|");
        
        {
            let mut cursor = buffer.cursor();
            let msg = cursor.read_bytes(9).unwrap();
            assert_eq!(msg, b"message1|");
        }
        
        buffer.mark_consumed(9);
        
        // Add second chunk
        buffer.extend(b"message2|");
        
        {
            let mut cursor = buffer.cursor();
            let msg = cursor.read_bytes(9).unwrap();
            assert_eq!(msg, b"message2|");
        }
        
        buffer.mark_consumed(9);
        
        // Compact
        buffer.compact();
        assert_eq!(buffer.base_offset, 18);
        assert_eq!(buffer.data.len(), 0);
    }
}
