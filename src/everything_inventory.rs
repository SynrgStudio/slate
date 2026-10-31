//! Checked IPC QUERY2 full-path/size/mtime pages; no file opens or stat calls.
use crate::periscope_store::Entry;
use std::path::PathBuf;
pub(crate) struct Page {
    pub entries: Vec<Entry>,
    pub total: usize,
    pub returned: usize,
    pub offset: usize,
}
fn word(data: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        data.get(at..at + 4)
            .ok_or("Truncated inventory DWORD")?
            .try_into()
            .unwrap(),
    ))
}
fn long(data: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        data.get(at..at + 8)
            .ok_or("Truncated inventory metadata")?
            .try_into()
            .unwrap(),
    ))
}
pub(crate) fn parse(data: &[u8]) -> Result<Page, String> {
    let total = word(data, 0)? as usize;
    let returned = word(data, 4)? as usize;
    let offset = word(data, 8)? as usize;
    if word(data, 12)? != 0x54 || word(data, 16)? != 1 {
        return Err("Everything did not supply the requested inventory metadata/order".into());
    }
    if returned > 8192
        || (returned > 0 && offset.checked_add(returned).is_none_or(|end| end > total))
    {
        return Err("Invalid inventory page count".into());
    }
    let minimum = 20 + returned * 8;
    if minimum > data.len() {
        return Err("Truncated inventory item table".into());
    }
    let mut entries = Vec::new();
    for index in 0..returned {
        let flags = word(data, 20 + index * 8)?;
        let at = word(data, 24 + index * 8)? as usize;
        if at < minimum || at % 2 != 0 {
            return Err("Invalid inventory data offset".into());
        }
        let length = word(data, at)? as usize;
        if length == 0 || length > 32768 {
            return Err("Invalid inventory path length".into());
        }
        let end = at
            .checked_add(4 + (length + 1) * 2)
            .ok_or("Inventory length overflow")?;
        let bytes = data.get(at + 4..end).ok_or("Truncated inventory path")?;
        if bytes[bytes.len() - 2..] != [0, 0] {
            return Err("Inventory path is not terminated".into());
        }
        let words = bytes[..bytes.len() - 2]
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect::<Vec<_>>();
        if words.contains(&0) {
            return Err("Inventory path contains NUL".into());
        }
        let path = String::from_utf16(&words).map_err(|_| "Invalid inventory Unicode")?;
        let size = long(data, end)?;
        let modified = long(data, end + 8)?;
        if flags & 3 == 0 {
            entries.push(Entry {
                path: PathBuf::from(path),
                size: if size == u64::MAX {
                    0
                } else {
                    size.min(i64::MAX as u64)
                },
                modified: if modified == u64::MAX {
                    0
                } else {
                    modified.saturating_sub(116444736000000000) / 10_000_000
                } as i64,
            });
        }
    }
    Ok(Page {
        entries,
        total,
        returned,
        offset,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "manual read-only Everything metadata integration"]
    fn live_inventory_reads_metadata_without_opening_files() {
        let root = PathBuf::from("D:/slate-baselines");
        let start = std::time::Instant::now();
        let page = crate::everything_ipc::inventory(
            &crate::everything_ipc::expression("", &[root.clone()]),
            0,
        )
        .unwrap();
        let entry = page
            .entries
            .iter()
            .find(|entry| entry.path == root.join("large-document-5m.md"))
            .expect("fixture in Everything inventory");
        assert_eq!(entry.size, 5_191_998);
        assert!(entry.modified > 0);
        let eof = crate::everything_ipc::inventory(
            &crate::everything_ipc::expression("", &[root]),
            page.total + 8192,
        )
        .unwrap();
        assert_eq!(eof.returned, 0);
        println!(
            "EVERYTHING_INVENTORY total={} files={} wall_ms={:.3}",
            page.total,
            page.entries.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
    #[test]
    fn empty_page_beyond_a_shrunk_live_inventory_is_valid_but_nonempty_overflow_is_not() {
        let mut bytes = Vec::new();
        for value in [3_u32, 0, 100, 0x54, 1] {
            bytes.extend(value.to_le_bytes());
        }
        let page = parse(&bytes).unwrap();
        assert_eq!(page.offset, 100);
        assert_eq!(page.total, 3);
        assert_eq!(page.returned, 0);
        bytes[4..8].copy_from_slice(&1_u32.to_le_bytes());
        assert!(parse(&bytes).is_err());
    }
    #[test]
    fn parses_length_prefixed_unicode_and_unaligned_metadata_and_rejects_corruption() {
        let path = "D:\\café\\note.md";
        let words = path.encode_utf16().collect::<Vec<_>>();
        let mut bytes = Vec::new();
        for value in [1_u32, 1, 0, 0x54, 1, 0, 28, words.len() as u32] {
            bytes.extend(value.to_le_bytes());
        }
        for word in words {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(42_u64.to_le_bytes());
        bytes.extend((116444736000000000_u64 + 99 * 10_000_000).to_le_bytes());
        let result = parse(&bytes).unwrap();
        assert_eq!(result.entries[0].path, PathBuf::from(path));
        assert_eq!(result.entries[0].size, 42);
        assert_eq!(result.entries[0].modified, 99);
        let mut wrong = bytes.clone();
        wrong[24..28].copy_from_slice(&2_u32.to_le_bytes());
        assert!(parse(&wrong).is_err());
        assert!(parse(&bytes[..bytes.len() - 1]).is_err());
        let mut wrong = bytes;
        wrong[12..16].copy_from_slice(&4_u32.to_le_bytes());
        assert!(parse(&wrong).is_err());
    }
}
