use std::str;
use std::{ io::Write, net::TcpListener};

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
                let part = parts[0].split(' ').collect::<Vec<&str>>()[1];
                let path = part.split('/').collect::<Vec<&str>>();
                println!("{:?},{:?}", path, part);
                match path[1] {
                    "" => {
                        let _ = stream
                            .write(b"HTTP/1.1 200 OK\r\n\r\n")
                            .expect("Error write stream");
                    }
                    "echo" => {
                        let value = path[2];
                        let _ = stream 
                            .write(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",value.len(), value).as_bytes())
                            .expect("Error write stream");
                    }
                    _ => {
                        let _ = stream
                            .write(b"HTTP/1.1 404 Not Found\r\n\r\n")
                            .expect("Error write stream");
                    }
                }
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }
    }
}
