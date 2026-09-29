// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Fixed-capacity output buffer (no allocation). Overflow is a caller bug: it is caught by
//! `debug_assert!` and reported as `Err(())` so release builds fail closed instead of panicking.

use core::fmt;

pub struct Buf<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> Buf<'a> {
    #[inline(always)]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    #[inline]
    pub fn put(&mut self, src: &[u8]) -> Result<(), ()> {
        let end = self.len + src.len();
        if end > self.buf.len() {
            debug_assert!(false, "Buf overflow: need {end}, have {}", self.buf.len());
            return Err(());
        }
        self.buf[self.len..end].copy_from_slice(src);
        self.len = end;
        Ok(())
    }

    #[inline(always)]
    pub fn u8(&mut self, v: u8) -> Result<(), ()> {
        self.put(&[v])
    }

    #[inline(always)]
    pub fn u16(&mut self, v: u16) -> Result<(), ()> {
        self.put(&v.to_le_bytes())
    }

    #[inline(always)]
    pub fn u32(&mut self, v: u32) -> Result<(), ()> {
        self.put(&v.to_le_bytes())
    }

    #[inline(always)]
    pub fn u64(&mut self, v: u64) -> Result<(), ()> {
        self.put(&v.to_le_bytes())
    }
}

impl fmt::Write for Buf<'_> {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.put(s.as_bytes()).map_err(|_| fmt::Error)
    }
}

/// Little-endian cursor over borrowed input.
pub struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    #[inline(always)]
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }

    #[inline(always)]
    pub fn remaining(&self) -> usize {
        self.b.len() - self.pos
    }

    #[inline]
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.b.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    #[inline]
    pub fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.take(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Some(a)
    }

    #[inline(always)]
    pub fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    #[inline(always)]
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.arr::<2>()?))
    }

    #[inline(always)]
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.arr::<4>()?))
    }

    #[inline(always)]
    pub fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.arr::<8>()?))
    }
}
