use std::str;
use std::{io::Read, io::Write, net::TcpListener};

const MAX_BUFFER_SIZE: usize = 2048;

fn main() {
    println!("Logs from your program will appear here!");

    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                println!("accepted new connection");
                let mut buf = [0; MAX_BUFFER_SIZE];
                let len = stream.peek(&mut buf).expect("peek failed");
                let buf = &buf[..len - 1];
                let string = String::from_utf8(buf.to_vec()).expect("failed convert string");
                let parts = string.split("\r\n").collect::<Vec<&str>>();
                let part = parts[0].split(" ").collect::<Vec<&str>>()[1];
                match part {
                    "/" => {
                        let _ = stream
                            .write(b"HTTP/1.1 200 OK\r\n\r\n")
                            .expect("Error write stream");
                    }
                    _ => {
                        let _ = stream
                            .write(b"HTTP/1.1 400 Not Found\r\n\r\n")
                            .expect("Error write stream");
                    }
                };
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }
    }
}
