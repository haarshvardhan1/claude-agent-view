//! Shared incremental line reader for append-only files.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Read position of an append-only file: byte offset, inode, and the
/// trailing partial line carried to the next read.
#[derive(Debug, Default)]
pub(crate) struct TailPos {
    pub offset: u64,
    pub inode: Option<u64>,
    pub carry: Vec<u8>,
}

/// Returns the complete, trimmed, non-empty lines appended since the last
/// call, and whether the position was reset (inode changed or file shrank).
/// The offset advances by the bytes actually read, not the stat length.
pub(crate) fn read_appended(
    path: &Path,
    pos: &mut TailPos,
) -> std::io::Result<(Vec<String>, bool)> {
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    let (ino, len) = (meta.ino(), meta.len());
    let mut reset = false;
    if pos.inode.is_some_and(|i| i != ino) || len < pos.offset {
        pos.offset = 0;
        pos.carry.clear();
        reset = true;
    }
    pos.inode = Some(ino);

    file.seek(SeekFrom::Start(pos.offset))?;
    let mut new = Vec::new();
    file.read_to_end(&mut new)?;
    pos.offset += new.len() as u64;

    let mut data = std::mem::take(&mut pos.carry);
    data.extend_from_slice(&new);
    let end = data.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let mut lines = Vec::new();
    for raw in data[..end].split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(raw);
        let line = line.trim();
        if !line.is_empty() {
            lines.push(line.to_string());
        }
    }
    pos.carry = data[end..].to_vec();
    Ok((lines, reset))
}
