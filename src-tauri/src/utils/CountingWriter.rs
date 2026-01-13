use std::io::{self, Seek, SeekFrom, Write};
use std::sync::{Arc, Mutex};

pub struct CountingWriter<W: Write + Seek> {
  inner: W,
  written: Arc<Mutex<u64>>,
}

impl<W: Write + Seek> CountingWriter<W> {
  pub fn new(inner: W, written: Arc<Mutex<u64>>) -> Self {
    Self { inner, written }
  }
}

impl<W: Write + Seek> Write for CountingWriter<W> {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    let size = self.inner.write(buf)?;
    if let Ok(mut w) = self.written.lock() {
      *w += size as u64;
    }
    Ok(size)
  }
  fn flush(&mut self) -> io::Result<()> {
    self.inner.flush()
  }
}

impl<W: Write + Seek> Seek for CountingWriter<W> {
  fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
    self.inner.seek(pos)
  }
}
