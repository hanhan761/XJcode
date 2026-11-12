//! The login window closes only after the launcher acknowledges receipt.
use std::io::{self, Read, Write};
pub fn send<T: Read + Write>(connection: &mut T, payload: &[u8]) -> io::Result<()> {
    if payload.len() > 16384 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "login payload too large")); }
    connection.write_all(&(payload.len() as u32).to_le_bytes())?;
    connection.write_all(payload)?;
    connection.flush()?;
    let mut ack = [0];
    connection.read_exact(&mut ack)?;
    if ack[0] != 1 { return Err(io::Error::new(io::ErrorKind::InvalidData, "login receipt not confirmed")); }
    Ok(())
}
#[cfg(test)] mod tests {
 use super::*;
 struct Fake { output:Vec<u8>, ack:Option<u8> }
 impl Read for Fake { fn read(&mut self,b:&mut [u8])->io::Result<usize> { match self.ack.take() {Some(v)=>{b[0]=v;Ok(1)},None=>Ok(0)} } }
 impl Write for Fake { fn write(&mut self,b:&[u8])->io::Result<usize> {self.output.extend_from_slice(b);Ok(b.len())} fn flush(&mut self)->io::Result<()> {Ok(())} }
 #[test] fn requires_positive_receipt_and_preserves_frame() {
  let mut f=Fake{output:vec![],ack:Some(1)};send(&mut f,b"abc").unwrap();assert_eq!(f.output,b"\x03\x00\x00\x00abc");
  for ack in [None,Some(0),Some(2)] {let mut f=Fake{output:vec![],ack};assert!(send(&mut f,b"abc").is_err());}
  let mut f=Fake{output:vec![],ack:Some(1)};assert!(send(&mut f,&[0;16385]).is_err());assert!(f.output.is_empty());
 }
}

#[cfg(all(test, windows))]
mod windows_pipe_tests {
    use super::send;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn sends_to_private_pipe_and_waits_for_ack() {
        let name = format!(r"\\.\pipe\xjcode-handoff-test-{}", uuid::Uuid::new_v4());
        let mut server = xjcode_native::ipc::private_pipe(&name).unwrap();
        let client = std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(name).unwrap();
            send(&mut pipe, b"fixture-only")
        });
        server.connect().await.unwrap();
        assert_eq!(server.read_u32_le().await.unwrap(), 12);
        let mut data = [0; 12];
        server.read_exact(&mut data).await.unwrap();
        assert_eq!(&data, b"fixture-only");
        assert!(!client.is_finished());
        server.write_u8(1).await.unwrap();
        client.join().unwrap().unwrap();
    }
}
