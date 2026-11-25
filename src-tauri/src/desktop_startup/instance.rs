use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

pub enum RequestError {
    Unavailable,
    Closing,
}

pub struct Endpoint {
    address: PathBuf,
}
pub struct Server {
    stop: CancellationToken,
    #[cfg(unix)]
    path: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Endpoint {
    pub fn new(root: &Path) -> Result<Self, String> {
        let digest: String = Sha256::digest(root.as_os_str().as_encoded_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        #[cfg(windows)]
        let address = PathBuf::from(format!(
            "\\\\.\\pipe\\sodeck-{}-{}",
            windows::sid()?,
            &digest[..24]
        ));
        #[cfg(unix)]
        let address = {
            use std::os::unix::fs::MetadataExt;
            let uid = unsafe { libc::geteuid() };
            let directory = PathBuf::from("/tmp").join(format!("sodeck-{uid}"));
            crate::platform::private_directory(&directory).map_err(|e| e.to_string())?;
            if std::fs::metadata(&directory)
                .map_err(|e| e.to_string())?
                .uid()
                != uid
            {
                return Err("本地激活目录不属于当前用户".into());
            }
            directory.join(format!("{}.sock", &digest[..24]))
        };
        Ok(Self { address })
    }

    pub fn activate(&self) -> Result<(), RequestError> {
        tauri::async_runtime::block_on(async {
            let action = async {
                #[cfg(windows)]
                let mut connection = tokio::net::windows::named_pipe::ClientOptions::new()
                    .open(&self.address)
                    .map_err(|_| RequestError::Unavailable)?;
                #[cfg(windows)]
                windows::verify_server(&connection).map_err(|_| RequestError::Unavailable)?;
                #[cfg(unix)]
                let mut connection = tokio::net::UnixStream::connect(&self.address)
                    .await
                    .map_err(|_| RequestError::Unavailable)?;
                #[cfg(unix)]
                if !connection
                    .peer_cred()
                    .is_ok_and(|credentials| credentials.uid() == unsafe { libc::geteuid() })
                {
                    return Err(RequestError::Unavailable);
                }
                connection
                    .write_all(b"SODECK1\n")
                    .await
                    .map_err(|_| RequestError::Unavailable)?;
                let mut response = [0u8; 1];
                connection
                    .read_exact(&mut response)
                    .await
                    .map_err(|_| RequestError::Unavailable)?;
                match response[0] {
                    b'1' => Ok(()),
                    b'0' => Err(RequestError::Closing),
                    _ => Err(RequestError::Unavailable),
                }
            };
            tokio::time::timeout(Duration::from_secs(2), action)
                .await
                .unwrap_or(Err(RequestError::Unavailable))
        })
    }

    pub fn listen(
        &self,
        callback: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<Server, String> {
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let callback = Arc::new(callback);
        #[cfg(windows)]
        {
            let address = self.address.clone();
            let initial = tauri::async_runtime::block_on(async { windows::pipe(&address, true) })
                .map_err(|e| format!("无法建立本地激活入口：{e}"))?;
            tauri::async_runtime::spawn(async move {
                let mut listener = initial;
                loop {
                    tokio::select! { _ = cancellation.cancelled() => break, result = listener.connect() => if result.is_err() { break; } }
                    let next = match windows::pipe(&address, false) {
                        Ok(pipe) => pipe,
                        Err(_) => break,
                    };
                    let connection = std::mem::replace(&mut listener, next);
                    let callback = callback.clone();
                    tokio::spawn(async move {
                        let _ = exchange(connection, callback).await;
                    });
                }
            });
        }
        #[cfg(unix)]
        {
            match std::fs::remove_file(&self.address) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
            let listener =
                std::os::unix::net::UnixListener::bind(&self.address).map_err(|e| e.to_string())?;
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            let listener = tauri::async_runtime::block_on(async {
                tokio::net::UnixListener::from_std(listener)
            })
            .map_err(|e| e.to_string())?;
            tauri::async_runtime::spawn(async move {
                loop {
                    let connection = tokio::select! { _ = cancellation.cancelled() => break, result = listener.accept() => match result { Ok((connection, _)) => connection, Err(_) => break } };
                    if !connection
                        .peer_cred()
                        .is_ok_and(|credentials| credentials.uid() == unsafe { libc::geteuid() })
                    {
                        continue;
                    }
                    let callback = callback.clone();
                    tokio::spawn(async move {
                        let _ = exchange(connection, callback).await;
                    });
                }
            });
        }
        Ok(Server {
            stop,
            #[cfg(unix)]
            path: self.address.clone(),
        })
    }
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut connection: S,
    callback: Arc<impl Fn() -> bool>,
) -> io::Result<()> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut request = [0u8; 8];
        connection.read_exact(&mut request).await?;
        if &request != b"SODECK1\n" {
            return Err(io::Error::other("未知的本地激活协议"));
        }
        connection
            .write_all(if callback() { b"1" } else { b"0" })
            .await?;
        connection.shutdown().await
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "激活请求超时"))?
}

#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            },
            GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    pub(super) fn sid() -> Result<String, String> {
        sid_for(unsafe { GetCurrentProcess() })
    }

    fn sid_for(process: windows_sys::Win32::Foundation::HANDLE) -> Result<String, String> {
        unsafe {
            let mut token = std::ptr::null_mut();
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            let mut bytes = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut bytes);
            let mut buffer = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
            let result = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            );
            CloseHandle(token);
            if result == 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            let mut text = std::ptr::null_mut();
            if ConvertSidToStringSidW(
                (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid,
                &mut text,
            ) == 0
            {
                return Err(io::Error::last_os_error().to_string());
            }
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            Ok(value)
        }
    }

    pub(super) fn verify_server(
        pipe: &tokio::net::windows::named_pipe::NamedPipeClient,
    ) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            System::{
                Pipes::GetNamedPipeServerProcessId,
                Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
            },
            UI::WindowsAndMessaging::AllowSetForegroundWindow,
        };
        unsafe {
            let mut pid = 0;
            if GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) == 0 {
                return Err(io::Error::last_os_error());
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            let owner = sid_for(process);
            CloseHandle(process);
            if owner.map_err(io::Error::other)? != sid().map_err(io::Error::other)? {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "激活入口不属于当前用户",
                ));
            }
            AllowSetForegroundWindow(pid);
            Ok(())
        }
    }

    pub(super) fn pipe(
        path: &Path,
        first: bool,
    ) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        let descriptor: Vec<u16> = format!("D:P(A;;GA;;;{})", sid().map_err(io::Error::other)?)
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
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
            let mut attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: security,
                bInheritHandle: 0,
            };
            let result = tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(
                    path,
                    (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
                );
            LocalFree(security);
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_roundtrip_and_closing_reply() {
        let root =
            std::env::temp_dir().join(format!("sodeck-activation-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let endpoint = Endpoint::new(&std::fs::canonicalize(&root).unwrap()).unwrap();
        let server = endpoint.listen(|| true).unwrap();
        assert!(endpoint.activate().is_ok());
        drop(server);
        std::thread::sleep(Duration::from_millis(50));
        let server = endpoint.listen(|| false).unwrap();
        assert!(matches!(endpoint.activate(), Err(RequestError::Closing)));
        drop(server);
        std::fs::remove_dir_all(root).unwrap();
    }
}
