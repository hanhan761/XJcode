use anyhow::{Context, Result, ensure};
use std::{ffi::c_void, mem::size_of, ptr};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    },
    Security::{GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub fn private_pipe(name: &str) -> Result<NamedPipeServer> {
    let mut token = ptr::null_mut();
    ensure!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0,
        "无法读取当前 Windows 身份"
    );
    let mut size = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
    }
    let mut data = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            data.as_mut_ptr() as *mut c_void,
            size,
            &mut size,
        )
    };
    unsafe {
        CloseHandle(token);
    };
    ensure!(ok != 0, "无法读取 Windows SID");
    let user = unsafe { &*(data.as_ptr() as *const TOKEN_USER) };
    let mut sid = ptr::null_mut();
    ensure!(
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } != 0,
        "无法转换 Windows SID"
    );
    let mut len = 0;
    unsafe {
        while *sid.add(len) != 0 {
            len += 1;
        }
    }
    let sid_text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid, len) });
    unsafe {
        LocalFree(sid as _);
    }
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid_text})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = ptr::null_mut();
    ensure!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } != 0,
        "无法创建登录管道权限"
    );
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let result = unsafe {
        ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, &mut attributes as *mut _ as *mut c_void)
    };
    unsafe {
        LocalFree(descriptor as _);
    };
    result.context("无法创建私有登录管道")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn current_user_can_exchange_a_framed_result_and_ack() {
        let name = format!(r"\\.\pipe\xjcode-test-{}", uuid::Uuid::new_v4());
        let mut server = private_pipe(&name).unwrap();
        let task = tokio::spawn(async move {
            let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(name)
                .unwrap();
            client.write_u32_le(4).await.unwrap();
            client.write_all(b"test").await.unwrap();
            assert_eq!(client.read_u8().await.unwrap(), 1);
        });
        server.connect().await.unwrap();
        assert_eq!(server.read_u32_le().await.unwrap(), 4);
        let mut data = [0; 4];
        server.read_exact(&mut data).await.unwrap();
        assert_eq!(&data, b"test");
        server.write_u8(1).await.unwrap();
        task.await.unwrap();
    }
}
