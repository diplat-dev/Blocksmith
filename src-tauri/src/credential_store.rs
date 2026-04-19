use std::{ffi::OsStr, iter, os::windows::ffi::OsStrExt, ptr};

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    },
};

use crate::error::{AppError, AppResult};

pub fn write_secret(target: &str, username: &str, secret: &str) -> AppResult<()> {
    let mut target_wide = to_wide(target);
    let mut username_wide = to_wide(username);
    let mut blob = secret.as_bytes().to_vec();

    let credential = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_wide.as_mut_ptr()),
        Comment: PWSTR::null(),
        LastWritten: Default::default(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: if blob.is_empty() {
            ptr::null_mut()
        } else {
            blob.as_mut_ptr()
        },
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(username_wide.as_mut_ptr()),
    };

    unsafe {
        CredWriteW(&credential, 0).map_err(windows_error)?;
    }

    Ok(())
}

pub fn read_secret(target: &str) -> AppResult<Option<String>> {
    let target_wide = to_wide(target);
    let mut credential_ptr: *mut CREDENTIALW = ptr::null_mut();

    let read_result =
        unsafe {
            CredReadW(
                PCWSTR(target_wide.as_ptr()),
                CRED_TYPE_GENERIC,
                Some(0),
                &mut credential_ptr,
            )
        };

    if let Err(error) = read_result {
        let message = error.to_string();
        if message.contains("Element not found") || message.contains("1168") {
            return Ok(None);
        }
        return Err(windows_error(error));
    }

    let secret = unsafe {
        let credential = &*credential_ptr;
        let bytes = std::slice::from_raw_parts(
            credential.CredentialBlob as *const u8,
            credential.CredentialBlobSize as usize,
        );
        String::from_utf8(bytes.to_vec())
            .map_err(|error| AppError::Internal(format!("stored credential was not valid UTF-8: {error}")))
    };

    unsafe {
        CredFree(credential_ptr as *const _);
    }

    secret.map(Some)
}

pub fn delete_secret(target: &str) -> AppResult<()> {
    let target_wide = to_wide(target);
    let delete_result =
        unsafe { CredDeleteW(PCWSTR(target_wide.as_ptr()), CRED_TYPE_GENERIC, Some(0)) };

    if let Err(error) = delete_result {
        let message = error.to_string();
        if message.contains("Element not found") || message.contains("1168") {
            return Ok(());
        }
        return Err(windows_error(error));
    }

    Ok(())
}

fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

fn windows_error(error: windows::core::Error) -> AppError {
    AppError::Internal(format!("windows credential manager error: {error}"))
}
