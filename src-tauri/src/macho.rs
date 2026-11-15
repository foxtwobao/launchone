//! Read executable architectures without requiring Xcode command-line tools.
use crate::error::{message, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

fn invalid() -> crate::error::AppError {
    message("应用可执行文件的芯片信息无效，未安装")
}
fn u32_at(bytes: &[u8], little: bool) -> u32 {
    let bytes: [u8; 4] = bytes.try_into().expect("four byte field");
    if little {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    }
}
fn u64_at(bytes: &[u8], little: bool) -> u64 {
    let bytes: [u8; 8] = bytes.try_into().expect("eight byte field");
    if little {
        u64::from_le_bytes(bytes)
    } else {
        u64::from_be_bytes(bytes)
    }
}
fn thin_cpu(header: &[u8]) -> Result<u32> {
    let little = match header[..4] {
        [0xce, 0xfa, 0xed, 0xfe] | [0xcf, 0xfa, 0xed, 0xfe] => true,
        [0xfe, 0xed, 0xfa, 0xce] | [0xfe, 0xed, 0xfa, 0xcf] => false,
        _ => return Err(invalid()),
    };
    Ok(u32_at(&header[4..8], little))
}
fn name(cpu: u32) -> Option<&'static str> {
    match cpu {
        0x01000007 => Some("x86_64"),
        0x0100000c => Some("aarch64"),
        _ => None,
    }
}

pub fn architectures(path: &Path) -> Result<Vec<String>> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut header = [0; 8];
    file.read_exact(&mut header).map_err(|_| invalid())?;
    let (little, wide) = match header[..4] {
        [0xca, 0xfe, 0xba, 0xbe] => (false, false),
        [0xbe, 0xba, 0xfe, 0xca] => (true, false),
        [0xca, 0xfe, 0xba, 0xbf] => (false, true),
        [0xbf, 0xba, 0xfe, 0xca] => (true, true),
        _ => {
            if length < 28 {
                return Err(invalid());
            }
            return Ok(name(thin_cpu(&header)?)
                .into_iter()
                .map(str::to_owned)
                .collect());
        }
    };
    let count = u32_at(&header[4..8], little) as usize;
    if count == 0 || count > 32 {
        return Err(invalid());
    }
    let entry_size = if wide { 32 } else { 20 };
    let table_end = 8 + count * entry_size;
    let mut table = vec![0; count * entry_size];
    file.read_exact(&mut table).map_err(|_| invalid())?;
    let mut result = Vec::new();
    let mut ranges = Vec::new();
    for entry in table.chunks_exact(entry_size) {
        let cpu = u32_at(&entry[..4], little);
        let (offset, size) = if wide {
            (
                u64_at(&entry[8..16], little),
                u64_at(&entry[16..24], little),
            )
        } else {
            (
                u32_at(&entry[8..12], little) as u64,
                u32_at(&entry[12..16], little) as u64,
            )
        };
        let end = offset.checked_add(size).ok_or_else(invalid)?;
        if offset < table_end as u64
            || size < 28
            || end > length
            || ranges.iter().any(|&(a, b)| offset < b && end > a)
        {
            return Err(invalid());
        }
        ranges.push((offset, end));
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut header).map_err(|_| invalid())?;
        if thin_cpu(&header)? != cpu {
            return Err(invalid());
        }
        if let Some(arch) = name(cpu) {
            if !result.iter().any(|value| value == arch) {
                result.push(arch.into());
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn thin(cpu: u32) -> Vec<u8> {
        let mut bytes = vec![0xcf, 0xfa, 0xed, 0xfe];
        bytes.extend(cpu.to_le_bytes());
        bytes.resize(32, 0);
        bytes
    }
    fn inspect(bytes: &[u8]) -> Result<Vec<String>> {
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), bytes).unwrap();
        architectures(temp.path())
    }
    #[test]
    fn thin_binaries_use_executable_architecture() {
        assert_eq!(inspect(&thin(0x0100000c)).unwrap(), ["aarch64"]);
        assert_eq!(inspect(&thin(0x01000007)).unwrap(), ["x86_64"]);
        assert!(inspect(&[0; 32]).is_err());
    }
    #[test]
    fn universal_checks_each_slice_not_just_the_table() {
        let mut bytes = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2];
        for (cpu, offset) in [(0x01000007u32, 64u32), (0x0100000c, 96)] {
            for field in [cpu, 0, offset, 32, 0] {
                bytes.extend(field.to_be_bytes());
            }
        }
        bytes.resize(64, 0);
        bytes.extend(thin(0x01000007));
        bytes.extend(thin(0x0100000c));
        assert_eq!(inspect(&bytes).unwrap(), ["x86_64", "aarch64"]);
        bytes[100..104].copy_from_slice(&0x01000007u32.to_le_bytes());
        assert!(inspect(&bytes).is_err());
    }
    #[test]
    fn rejects_oversized_tables_out_of_bounds_and_overlapping_slices() {
        assert!(inspect(&[0xca, 0xfe, 0xba, 0xbe, 0xff, 0xff, 0xff, 0xff]).is_err());
        let mut bytes = vec![0xca, 0xfe, 0xba, 0xbf, 0, 0, 0, 1];
        bytes.extend(0x01000007u32.to_be_bytes());
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(u64::MAX.to_be_bytes());
        bytes.extend(32u64.to_be_bytes());
        bytes.extend([0; 8]);
        assert!(inspect(&bytes).is_err());
    }
}
