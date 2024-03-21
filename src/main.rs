use anyhow::Result;
use std::collections::HashMap;
use std::net::TcpStream;
use std::str;
use std::{io::Write, net::TcpListener};

const MAX_BUFFER_SIZE: usize = 2048;

#[derive(Debug)]
enum HttpMethod {
    GET,
    POST,
}

impl From<String> for HttpMethod {
    fn from(value: String) -> Self {
        match value.as_str() {
            "GET" => HttpMethod::GET,
            "POST" => HttpMethod::POST,
            _ => HttpMethod::GET,
        }
    }
}

#[derive(Debug)]
struct Request {
    pub path: String,
    pub method: HttpMethod,
    pub headers: HashMap<String, String>,
}

impl Request {
    fn parse_top(data: &str) -> (HttpMethod, String) {
        let parts = data.split(' ').collect::<Vec<&str>>();
        let http_method = HttpMethod::from(parts[0].to_owned());
        let path = parts[1].to_owned();
        (http_method, path)
    }

    fn parse_header(data: Vec<&str>) -> HashMap<String, String> {
        let headers = data
            .into_iter()
            .filter(|header| match header.find(':') {
                Some(_) => true,
                None => false,
            })
            .map(|header| {
                let key_value = header.split(": ").collect::<Vec<&str>>();
                (key_value[0].to_owned(), key_value[1].to_owned())
            })
            .collect::<HashMap<String, String>>();
        headers
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let string = String::from_utf8(data.to_vec())?;
        let lines = string.split("\r\n\r\n").collect::<Vec<&str>>();
        let parts = lines[0].split("\r\n").collect::<Vec<&str>>();
        let (method, path) = Request::parse_top(parts[0]);
        let headers = Request::parse_header(parts[1..].to_vec());

        Ok(Request {
            method,
            path,
            headers,
        })
    }
}

enum HTTPCode {
    OK,
    NotFound,
}

impl ToString for HTTPCode {
    fn to_string(&self) -> String {
        match self {
            Self::OK => String::from("200 OK"),
            Self::NotFound => String::from("404 Not Found"),
        }
    }
}

struct Response {
    pub code: HTTPCode,
    pub content: String,
    pub headers: Option<HashMap<String, String>>,
}

impl Response {
    pub fn into_bytes(self) -> Vec<u8> {
        let mut buff = format!("HTTP/1.1 {}\r\n", self.code.to_string());
        if let Some(hashmap) = self.headers {
            for (key, value) in hashmap.into_iter() {
                buff.push_str(format!("{}: {}\r\n", key, value).as_str());
            }
        }
        buff.push_str("\r\n");
        buff.push_str(self.content.as_str());
        buff.into_bytes()
    }
}

enum CompareType {
    Prefix,
    Exact,
}

type FnRoute = Box<dyn Fn(Request) -> Response>;
struct Route {
    pub path: String,
    compare_type: CompareType,
    handler: FnRoute,
}

impl Route {
    pub fn new(path: &str, compare_type: CompareType, handler: FnRoute) -> Self {
        Route {
            path: path.to_owned(),
            compare_type,
            handler,
        }
    }

    pub fn matches(&self, req: &Request) -> Option<&FnRoute> {
        match self.compare_type {
            CompareType::Exact => {
                if self.path == req.path {
                    Some(&self.handler)
                } else {
                    None
                }
            }
            CompareType::Prefix => {
                if req.path.starts_with(&self.path) {
                    Some(&self.handler)
                } else {
                    None
                }
            }
        }
    }
}

struct Routes {
    routes: Vec<Route>,
}

impl Routes {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    pub fn add(&mut self, route: Route) {
        self.routes.push(route);
    }

    pub fn execute(&self, stream: &mut TcpStream, req: Request) {
        for route in self.routes.iter() {
            if let Some(handler) = route.matches(&req) {
                let res = handler(req);
                Routes::send_response(stream, res);
                return;
            }
        }
        let res = Response {
            code: HTTPCode::NotFound,
            content: String::from(""),
            headers: None,
        };
        Routes::send_response(stream, res);
    }

    fn send_response(stream: &mut TcpStream, data: Response) {
        let res = stream.write(&data.into_bytes());
        if let Err(err) = res {
            println!("Error sending response: {}", err);
        }
    }
}

fn read_request(stream: &TcpStream) -> Result<Request> {
    let mut buf = [0; MAX_BUFFER_SIZE];
    let len = stream.peek(&mut buf).expect("peek failed");
    let buf = &buf[..len - 1];
    Request::parse(buf)
}

fn echo(req: Request) -> Response {
    let value = req.path.replace("/echo/", "");
    let headers = HashMap::from([(String::from("content-length"), value.len().to_string())]);
    Response {
        code: HTTPCode::OK,
        content: value,
        headers: Some(headers),
    }
}

fn user_agent(req: Request) -> Response {
    match req.headers.get("User-Agent") {
        Some(value) => {
            let headers =
                HashMap::from([(String::from("content-length"), value.len().to_string())]);
            Response {
                code: HTTPCode::OK,
                content: value.to_owned(),
                headers: Some(headers),
            }
        }
        None => Response {
            code: HTTPCode::OK,
            content: String::from(""),
            headers: None,
        },
    }
}

fn main() {
    println!("Logs from your program will appear here!");

    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();
    let mut routes = Routes::new();
    routes.add(Route::new(
        "/",
        CompareType::Exact,
        Box::new(|_| Response {
            code: HTTPCode::OK,
            headers: None,
            content: String::from(""),
        }),
    ));
    routes.add(Route::new("/echo", CompareType::Prefix, Box::new(echo)));
    routes.add(Route::new(
        "/user-agent",
        CompareType::Exact,
        Box::new(user_agent),
    ));

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                println!("accepted new connection");

                let req = match read_request(&stream) {
                    Ok(val) => val,
                    Err(err) => {
                        println!("error read request: {}", err);
                        return;
                    }
                };

                routes.execute(&mut stream, req);
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }
    }
}
