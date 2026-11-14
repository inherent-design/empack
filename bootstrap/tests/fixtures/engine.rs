//! Small native executable for testing Java/native argument, stream and exit boundaries.
use std::io::{Read, Write};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!(
            "empack {}",
            std::env::var("EMPACK_FIXTURE_VERSION").unwrap_or_else(|_| "0.8.5-beta".into())
        );
        return;
    }
    if args == ["--bootstrap-protocol"] {
        if std::env::var("EMPACK_FIXTURE_PROTOCOL").as_deref() == Ok("missing") {
            eprintln!("unrecognized argument --bootstrap-protocol");
            std::process::exit(2);
        }
        println!("empack-bootstrap/1");
        return;
    }
    if matches!(args.first().map(String::as_str), Some("--fixture-wait" | "--fixture-late-wait")) {
        use std::net::TcpStream;
        if args[0] == "--fixture-late-wait" {
            std::fs::write(&args[1], std::process::id().to_string()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let descriptor = std::env::var("EMPACK_BOOTSTRAP_CONTROL").unwrap();
        let (port, nonce) = descriptor.split_once(':').unwrap();
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream.write_all(format!("empack-bootstrap/1 {nonce}\n").as_bytes()).unwrap();
        let mut ready = [0; 6];
        stream.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"READY\n");
        std::fs::write(&args[1], std::process::id().to_string()).unwrap();
        let mut byte = [0];
        let _ = stream.read(&mut byte);
        eprintln!("fixture retired; recovery evidence cleared");
        std::process::exit(130);
    }
    if args.first().map(String::as_str) == Some("--fixture-argv") {
        let mut output = std::io::stdout().lock();
        output.write_all(&((args.len() - 1) as u32).to_be_bytes()).unwrap();
        for arg in &args[1..] {
            output.write_all(&(arg.len() as u32).to_be_bytes()).unwrap();
            output.write_all(arg.as_bytes()).unwrap();
        }
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input).unwrap();
        output.write_all(&input).unwrap();
        output.flush().unwrap();
        eprintln!("fixture returned 7");
        std::process::exit(7);
    }
    eprintln!("Unexpected fixture command");
    std::process::exit(2);
}
