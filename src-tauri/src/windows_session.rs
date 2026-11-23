use crate::error::Result;
use std::{fs, io, path::Path, ptr, slice};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
};

pub(crate) fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(encrypted) => Ok(Some(dpapi(&encrypted, false)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let encrypted = dpapi(bytes, true)?;
    crate::config::atomic_write(path, &encrypted)
}

pub(crate) fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn dpapi(bytes: &[u8], protect: bool) -> io::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| io::ErrorKind::InvalidInput)?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let flags = CRYPTPROTECT_UI_FORBIDDEN;
    // DPAPI borrows the input and allocates the output with LocalAlloc.
    unsafe {
        let success = if protect {
            CryptProtectData(&input, ptr::null(), ptr::null(), ptr::null(), ptr::null(), flags, &mut output)
        } else {
            CryptUnprotectData(&input, ptr::null_mut(), ptr::null(), ptr::null(), ptr::null(), flags, &mut output)
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = if output.cbData == 0 {
            Vec::new()
        } else {
            slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}
