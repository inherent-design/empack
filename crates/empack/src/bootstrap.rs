//! One invocation's cancellation connection; it grants no pack or mutation authority.
use empack_lib::application::process_runtime::Cancellation;
use std::{io, net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
};

pub(super) struct Control(JoinHandle<()>);

impl Drop for Control {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) async fn attach(
    descriptor: Option<&std::ffi::OsStr>,
    cancel: Cancellation,
) -> io::Result<Option<Control>> {
    let Some(descriptor) = descriptor else {
        return Ok(None);
    };
    let descriptor = descriptor.to_str().ok_or_else(invalid_descriptor)?;
    connect(descriptor, cancel).await.map(Some)
}

fn invalid_descriptor() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Invalid empack bootstrap control descriptor",
    )
}

async fn connect(descriptor: &str, cancel: Cancellation) -> io::Result<Control> {
    let (port, nonce) = descriptor.split_once(':').ok_or_else(invalid_descriptor)?;
    let port: u16 = port.parse().map_err(|_| invalid_descriptor())?;
    if port == 0 || nonce.len() != 64 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid_descriptor());
    }
    let handshake = async {
        // Never connect to a pack-controlled remote address. The one-use nonce
        // prevents a different local connection from satisfying this handshake.
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
        stream
            .write_all(format!("empack-bootstrap/1 {nonce}\n").as_bytes())
            .await?;
        let mut ready = [0u8; 6];
        stream.read_exact(&mut ready).await?;
        if &ready != b"READY\n" {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Bootstrap control peer did not authenticate the invocation",
            ));
        }
        Ok(stream)
    };
    let mut stream = tokio::time::timeout(Duration::from_secs(10), handshake)
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "Bootstrap cancellation handshake timed out before command execution",
            )
        })??;
    Ok(Control(tokio::spawn(async move {
        // Either an explicit cancellation byte or loss of the Java parent is an
        // execution input. The operation's eventual result remains authoritative.
        let _ = stream.read_u8().await;
        cancel.cancel();
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn protocol_probe_reports_interoperability_without_accepting_a_command() {
        let empack_lib::application::CliLoad::Display(message) =
            empack_lib::application::CliConfig::load_for_process_from([
                "empack",
                "--bootstrap-protocol",
            ])
            .unwrap()
        else {
            panic!("Protocol probe must be a display-only result")
        };
        assert_eq!(message, "empack-bootstrap/1\n");
        assert!(
            empack_lib::application::CliConfig::load_for_process_from([
                "empack",
                "--bootstrap-protocol",
                "status",
            ])
            .is_err()
        );
    }

    #[tokio::test]
    async fn cancellation_and_parent_loss_reach_the_existing_engine_token() {
        for explicit in [false, true] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let descriptor = format!(
                "{}:{}",
                listener.local_addr().unwrap().port(),
                "a".repeat(64)
            );
            let cancel = Cancellation::default();
            let token = cancel.clone();
            let attaching = tokio::spawn(async move { connect(&descriptor, token).await });
            let (mut peer, _) = listener.accept().await.unwrap();
            let mut hello = vec![0; "empack-bootstrap/1 ".len() + 65];
            peer.read_exact(&mut hello).await.unwrap();
            assert_eq!(
                hello,
                format!("empack-bootstrap/1 {}\n", "a".repeat(64)).as_bytes()
            );
            peer.write_all(b"READY\n").await.unwrap();
            let owner = attaching.await.unwrap().unwrap();
            assert!(!cancel.is_cancelled());
            if explicit {
                peer.write_all(b"C").await.unwrap();
            } else {
                drop(peer);
            }
            tokio::time::timeout(Duration::from_secs(2), async {
                while !cancel.is_cancelled() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            drop(owner);
        }
    }

    #[tokio::test]
    async fn malformed_descriptors_are_refused_without_connecting() {
        for descriptor in [
            "",
            "0:abc",
            "https://remote:8080",
            "22:../file",
            "65536:abc",
        ] {
            assert_eq!(
                connect(descriptor, Cancellation::default())
                    .await
                    .err()
                    .unwrap()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[tokio::test]
    async fn wrong_peer_acknowledgement_never_admits_command_execution() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let descriptor = format!(
            "{}:{}",
            listener.local_addr().unwrap().port(),
            "b".repeat(64)
        );
        let attaching =
            tokio::spawn(async move { connect(&descriptor, Cancellation::default()).await });
        let (mut peer, _) = listener.accept().await.unwrap();
        let mut hello = vec![0; "empack-bootstrap/1 ".len() + 65];
        peer.read_exact(&mut hello).await.unwrap();
        peer.write_all(b"WRONG\n").await.unwrap();
        assert_eq!(
            attaching.await.unwrap().err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
