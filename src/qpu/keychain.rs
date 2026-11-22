//! Read-only lookup of a secret in the operating system's credential store. qxide never writes
//! there; users store the key with the system's own tools (docs/design/qx-qpu.md).
//!
//! - Windows: Credential Manager, generic credential `qxide:<account>` (`CredReadW`).
//! - macOS: login keychain, service `qxide`, account `<account>` (`security`).
//! - Other systems: Secret Service, attributes `service=qxide account=<account>` (`secret-tool`).

pub const SERVICE: &str = "qxide";

/// The secret stored for `account`, or `None` if there is none or no credential store.
pub fn read(account: &str) -> Option<String> {
    let secret = platform::read(account)?;
    let secret = secret.trim().to_string();
    (!secret.is_empty()).then_some(secret)
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    // CREDENTIALW from wincred.h.
    #[repr(C)]
    struct Credential {
        flags: u32,
        kind: u32,
        target_name: *mut u16,
        comment: *mut u16,
        last_written: FileTime,
        blob_size: u32,
        blob: *mut u8,
        persist: u32,
        attribute_count: u32,
        attributes: *mut c_void,
        target_alias: *mut u16,
        user_name: *mut u16,
    }

    const CRED_TYPE_GENERIC: u32 = 1;

    #[link(name = "advapi32")]
    extern "system" {
        fn CredReadW(target: *const u16, kind: u32, flags: u32, out: *mut *mut Credential) -> i32;
        fn CredFree(buffer: *mut c_void);
    }

    pub fn read(account: &str) -> Option<String> {
        let target: Vec<u16> = format!("{}:{account}", super::SERVICE)
            .encode_utf16()
            .chain([0])
            .collect();
        let mut cred: *mut Credential = std::ptr::null_mut();
        // SAFETY: `target` is NUL-terminated UTF-16 and `cred` is a valid out pointer; on success
        // the system allocates the credential, which is freed with `CredFree` below.
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) };
        if ok == 0 || cred.is_null() {
            return None;
        }
        // SAFETY: `cred` points to a CREDENTIALW whose blob holds `blob_size` bytes.
        let bytes = unsafe {
            let c = &*cred;
            if c.blob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.blob, c.blob_size as usize).to_vec()
            }
        };
        // SAFETY: allocated by CredReadW above, freed once.
        unsafe { CredFree(cred.cast()) };
        // `cmdkey` and the Credential Manager store the password as UTF-16.
        if bytes.len() % 2 == 0 {
            let (pairs, _) = bytes.as_chunks::<2>();
            let wide: Vec<u16> = pairs.iter().map(|&p| u16::from_le_bytes(p)).collect();
            if let Ok(s) = String::from_utf16(&wide) {
                if !s.contains('\0') {
                    return Some(s);
                }
            }
        }
        String::from_utf8(bytes).ok()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    pub fn read(account: &str) -> Option<String> {
        let out = std::process::Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                super::SERVICE,
                "-a",
                account,
                "-w",
            ])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).ok())
            .flatten()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    pub fn read(account: &str) -> Option<String> {
        let out = crate::pkg::command("secret-tool")
            .ok()?
            .args(["lookup", "service", super::SERVICE, "account", account])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).ok())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    // Touches the real credential store, so it only runs on request:
    // `QXIDE_TEST_KEYCHAIN=1 cargo test keychain -- --ignored` after storing `qxide:qxide-test`
    // with the value `keychain-test-value` (for example
    // `cmdkey /generic:qxide:qxide-test /user:qxide /pass:keychain-test-value`).
    #[test]
    #[ignore = "reads the real credential store"]
    fn reads_a_stored_secret() {
        if std::env::var_os("QXIDE_TEST_KEYCHAIN").is_none() {
            return;
        }
        assert_eq!(
            super::read("qxide-test").as_deref(),
            Some("keychain-test-value")
        );
        assert_eq!(super::read("qxide-no-such-account-3c6e1f"), None);
    }
}
