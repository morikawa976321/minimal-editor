//! Keep RichEdit's system-color highlights inside this process monochrome.
//! The windowed control has no selection-color setter. Redirect only its named
//! GetSysColor import; do not alter user32, DLL files, IME attributes or OS colors.
use crate::win::*;
use std::{
    ptr, slice,
    sync::atomic::{AtomicBool, Ordering},
};

static COMPOSING: AtomicBool = AtomicBool::new(false);

pub fn composing(value: bool) {
    COMPOSING.store(value, Ordering::Relaxed);
}

unsafe extern "system" fn color(index: i32) -> u32 {
    match index {
        13 => {
            if COMPOSING.load(Ordering::Relaxed) {
                0x202020
            } else {
                0x414141
            }
        }
        14 => 0xdfdfdf,
        _ => GetSysColor(index),
    }
}

fn dword(image: &[u8], at: usize) -> Result<u32, String> {
    let bytes = image
        .get(at..at.checked_add(4).ok_or("PE offset overflow")?)
        .ok_or("PE offset outside image")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn slots(image: &[u8], optional: usize) -> Result<Vec<usize>, String> {
    let mut result = Vec::new();
    for (directory, stride, names_at, iat_at) in [(1, 20, 0, 16), (13, 32, 16, 12)] {
        let start = dword(image, optional + 112 + directory * 8)? as usize;
        let size = dword(image, optional + 116 + directory * 8)? as usize;
        if start == 0 || size == 0 {
            continue;
        }
        let limit = start
            .checked_add(size)
            .filter(|end| *end <= image.len())
            .ok_or("PE import directory outside image")?;
        let mut descriptor = start;
        while descriptor
            .checked_add(stride)
            .is_some_and(|end| end <= limit)
        {
            let dll = dword(image, descriptor + if directory == 1 { 12 } else { 4 })?;
            if dll == 0 {
                break;
            }
            if directory == 13 && dword(image, descriptor)? & 1 == 0 {
                return Err("PE delay imports are not relative addresses".into());
            }
            let names = dword(image, descriptor + names_at)? as usize;
            let iat = dword(image, descriptor + iat_at)? as usize;
            if names == 0 || iat == 0 {
                return Err("PE import names unavailable".into());
            }
            let mut offset = 0usize;
            loop {
                let at = names.checked_add(offset).ok_or("PE thunk overflow")?;
                let bytes = image
                    .get(at..at.checked_add(8).ok_or("PE thunk overflow")?)
                    .ok_or("PE thunk outside image")?;
                let name = u64::from_le_bytes(bytes.try_into().unwrap());
                if name == 0 {
                    break;
                }
                if name & (1 << 63) == 0 {
                    let at = usize::try_from(name)
                        .ok()
                        .and_then(|n| n.checked_add(2))
                        .filter(|n| *n < image.len())
                        .ok_or("PE import name outside image")?;
                    let tail = &image[at..(at.saturating_add(128)).min(image.len())];
                    if tail.starts_with(b"GetSysColor\0") {
                        let slot = iat
                            .checked_add(offset)
                            .filter(|s| s.checked_add(8).is_some_and(|end| end <= image.len()))
                            .ok_or("PE import slot outside image")?;
                        result.push(slot);
                    }
                }
                offset = offset.checked_add(8).ok_or("PE thunk overflow")?;
            }
            descriptor += stride;
        }
    }
    result.sort_unstable();
    result.dedup();
    if result.is_empty() {
        return Err("RichEdit GetSysColor import not found".into());
    }
    Ok(result)
}

pub unsafe fn install(module: Handle) -> Result<(), String> {
    if std::mem::size_of::<usize>() != 8 || module.is_null() {
        return Err("Unsupported RichEdit module".into());
    }
    let base = module.cast::<u8>();
    // The module was loaded by Windows, so its first header page is readable.
    let header = slice::from_raw_parts(base, 4096);
    if &header[..2] != b"MZ" {
        return Err("Invalid RichEdit PE header".into());
    }
    let pe = dword(header, 0x3c)? as usize;
    if dword(header, pe)? != 0x4550 {
        return Err("Invalid RichEdit PE signature".into());
    }
    let optional = pe.checked_add(24).ok_or("PE header overflow")?;
    if dword(header, optional)? & 0xffff != 0x20b {
        return Err("Expected PE32+ RichEdit".into());
    }
    let size = dword(header, optional + 56)? as usize;
    if !(4096..=64 * 1024 * 1024).contains(&size) {
        return Err("Invalid RichEdit image size".into());
    }
    let addresses = slots(slice::from_raw_parts(base, size), optional)?;
    // Install before creating controls; no control can concurrently read these slots.
    for offset in addresses {
        let slot = base.add(offset).cast::<usize>();
        let mut old = 0;
        if VirtualProtect(slot.cast(), 8, 4, &mut old) == 0 {
            return Err("RichEdit color import is not writable".into());
        }
        ptr::write_unaligned(slot, color as *const () as usize);
        let mut discarded = 0;
        if VirtualProtect(slot.cast(), 8, old, &mut discarded) == 0 {
            return Err("RichEdit color import protection could not be restored".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(delay: bool) -> Vec<u8> {
        let mut image = vec![0u8; 1024];
        let directory = if delay { 13 } else { 1 };
        let stride = if delay { 32 } else { 20 };
        let mut put =
            |at: usize, value: u32| image[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put(112 + directory * 8, 0x100);
        put(116 + directory * 8, stride * 2);
        if delay {
            put(0x100, 1);
            put(0x104, 0x280);
            put(0x10c, 0x300);
            put(0x110, 0x200);
        } else {
            put(0x100, 0x200);
            put(0x10c, 0x280);
            put(0x110, 0x300);
        }
        image[0x200..0x208].copy_from_slice(&(1u64 << 63 | 7).to_le_bytes()); // ordinal
        image[0x208..0x210].copy_from_slice(&0x240u64.to_le_bytes());
        image[0x242..0x24e].copy_from_slice(b"GetSysColor\0");
        image
    }
    #[test]
    fn named_import_and_ordinal_are_distinguished() {
        assert_eq!(slots(&image(false), 0).unwrap(), vec![0x308]);
    }
    #[test]
    fn delay_import_is_supported() {
        assert_eq!(slots(&image(true), 0).unwrap(), vec![0x308]);
    }
    #[test]
    fn unrelated_name_is_not_redirected() {
        let mut image = image(false);
        image[0x24d] = b'X';
        assert!(slots(&image, 0).is_err());
    }
    #[test]
    fn invalid_offsets_are_rejected() {
        let mut image = image(false);
        image[120..124].copy_from_slice(&1000u32.to_le_bytes());
        assert!(slots(&image, 0).is_err());
        assert!(slots(&[0u8; 8], 0).is_err());
    }
}
