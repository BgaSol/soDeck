use std::{io, path::Path};

pub(crate) fn secure_directory(path: &Path) -> io::Result<()> {
    super::private_directory(path)?;
    #[cfg(windows)]
    unsafe {
        use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
        use windows_sys::Win32::{
            Foundation::{CloseHandle, LocalFree},
            Security::{
                Authorization::{
                    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                    SetNamedSecurityInfoW, SE_FILE_OBJECT,
                },
                GetSecurityDescriptorDacl, GetTokenInformation, TokenUser,
                DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY,
                TOKEN_USER,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        if std::fs::symlink_metadata(path)?.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("私钥目录不能为重解析点"));
        }
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut bytes = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut bytes);
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut sid = std::ptr::null_mut();
        if ConvertSidToStringSidW((*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid, &mut sid) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while *sid.add(len) != 0 {
            len += 1;
        }
        let sid_text = String::from_utf16_lossy(std::slice::from_raw_parts(sid, len));
        LocalFree(sid.cast());
        let descriptor: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid_text})(A;OICI;FA;;;SY)")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut security = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            1,
            &mut security,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = std::ptr::null_mut();
        let result =
            if GetSecurityDescriptorDacl(security, &mut present, &mut acl, &mut defaulted) == 0 {
                Err(io::Error::last_os_error())
            } else {
                let mut name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let code = SetNamedSecurityInfoW(
                    name.as_mut_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    acl,
                    std::ptr::null_mut(),
                );
                if code == 0 {
                    Ok(())
                } else {
                    Err(io::Error::from_raw_os_error(code as i32))
                }
            };
        LocalFree(security);
        result?;
    }
    Ok(())
}
