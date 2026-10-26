//! Owner-only Windows credential security descriptors.

use crate::error::{Error, Result};
use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        },
        DACL_SECURITY_INFORMATION, GetFileSecurityW, GetTokenInformation,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

fn error() -> Error {
    Error::new(
        "credential_store",
        "cannot enforce owner-only Windows credential permissions",
    )
}
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

struct Allocation(*mut c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

unsafe fn text(pointer: *const u16) -> String {
    let mut length = 0;
    // SAFETY: callers supply a Windows-allocated null-terminated UTF-16 string.
    unsafe {
        while *pointer.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
    }
}

fn descriptor() -> Result<Allocation> {
    // SAFETY: API output pointers remain live, sized buffers are allocated from
    // returned lengths, and each successful Windows allocation/handle is released.
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(error());
        }
        let mut length = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length);
        let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err(error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(error());
        }
        let sid_allocation = Allocation(sid.cast());
        let sid = text(sid);
        let sddl = format!("O:{sid}D:P(A;;FA;;;SY)(A;;FA;;;{sid})");
        drop(sid_allocation);
        let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(error());
        }
        Ok(Allocation(descriptor))
    }
}

fn canonical(descriptor: *mut c_void) -> Result<String> {
    // SAFETY: descriptor is valid for the call and LocalFree owns the returned string.
    unsafe {
        let mut string = ptr::null_mut();
        if ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut string,
            ptr::null_mut(),
        ) == 0
        {
            return Err(error());
        }
        let allocation = Allocation(string.cast());
        let value = text(string);
        drop(allocation);
        Ok(value)
    }
}

pub(super) fn protect(path: &Path) -> Result<()> {
    let descriptor = descriptor()?;
    let path = wide(path);
    // SAFETY: both pointers are valid for the duration of SetFileSecurityW.
    if unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    } == 0
    {
        return Err(error());
    }
    // Grant the creating user full access before setting an explicit owner. This
    // works even when the parent granted creation/write but not WRITE_OWNER.
    if unsafe { SetFileSecurityW(path.as_ptr(), OWNER_SECURITY_INFORMATION, descriptor.0) } == 0 {
        return Err(error());
    }
    Ok(())
}

pub(super) fn validate(path: &Path) -> Result<()> {
    let expected = descriptor()?;
    let path = wide(path);
    // SAFETY: GetFileSecurityW reports the required buffer size before writing it.
    unsafe {
        let flags = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let mut length = 0;
        GetFileSecurityW(path.as_ptr(), flags, ptr::null_mut(), 0, &mut length);
        if length == 0 {
            return Err(error());
        }
        let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        if GetFileSecurityW(
            path.as_ptr(),
            flags,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        ) == 0
        {
            return Err(error());
        }
        if canonical(buffer.as_mut_ptr().cast())? != canonical(expected.0)? {
            return Err(error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_inherited_permissions_and_accepts_protected_owner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credentials");
        std::fs::write(&path, b"not a secret").unwrap();
        assert!(validate(&path).is_err());
        protect(&path).unwrap();
        validate(&path).unwrap();
    }
}
