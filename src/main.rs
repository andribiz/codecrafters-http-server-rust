use anyhow::Result;
use bytes::{BufMut, BytesMut};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::str;
use std::sync::Arc;
use std::{collections::HashMap, env};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const MAX_BUFFER_SIZE: usize = 2048;

#[derive(Debug, PartialEq)]
enum HttpMethod {
    GET,
    POST,
}

impl From<&str> for HttpMethod {
    fn from(value: &str) -> Self {
        match value {
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
        let http_method = HttpMethod::from(parts[0]);
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

enum HttpCode {
    OK,
    NotFound,
}

impl ToString for HttpCode {
    fn to_string(&self) -> String {
        match self {
            Self::OK => String::from("200 OK"),
            Self::NotFound => String::from("404 Not Found"),
        }
    }
}

struct Response {
    pub code: HttpCode,
    pub content: Option<Vec<u8>>,
    pub headers: Option<HashMap<String, String>>,
}

impl Response {
    pub fn into_bytes(self) -> Vec<u8> {
        let mut buff = vec![];
        buff.put(format!("HTTP/1.1 {}\r\n", self.code.to_string()).as_bytes());
        if let Some(hashmap) = self.headers {
            for (key, value) in hashmap.into_iter() {
                buff.put(format!("{}: {}\r\n", key, value).as_bytes());
            }
        }
        buff.put(&b"\r\n"[..]);
        if let Some(content) = self.content {
            buff.put(content.as_slice());
        }
        buff
    }
}

enum CompareType {
    Prefix,
    Exact,
}

type FnRoute = Box<dyn Fn(Request, &String) -> Response + Send + Sync>;
struct Route {
    pub path: String,
    method: HttpMethod,
    compare_type: CompareType,
    handler: FnRoute,
}

impl Route {
    pub fn new(method: &str, path: &str, compare_type: CompareType, handler: FnRoute) -> Self {
        Route {
            method: HttpMethod::from(method),
            path: path.to_owned(),
            compare_type,
            handler,
        }
    }

    pub fn matches(&self, req: &Request) -> Option<&FnRoute> {
        match self.compare_type {
            CompareType::Exact => {
                if self.path == req.path && self.method == req.method {
                    Some(&self.handler)
                } else {
                    None
                }
            }
            CompareType::Prefix => {
                if req.path.starts_with(&self.path) && self.method == req.method {
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
    directory: String,
}

impl Routes {
    pub fn new(directory: String) -> Self {
        Self {
            routes: Vec::new(),
            directory,
        }
    }

    pub fn add(&mut self, route: Route) {
        self.routes.push(route);
    }

    pub async fn execute(&self, stream: &mut TcpStream, req: Request) {
        for route in self.routes.iter() {
            if let Some(handler) = route.matches(&req) {
                let res = handler(req, &self.directory);
                Routes::send_response(stream, res).await;
                return;
            }
        }
        let res = Response {
            code: HttpCode::NotFound,
            content: None,
            headers: None,
        };
        Routes::send_response(stream, res).await;
    }

    async fn send_response(stream: &mut TcpStream, data: Response) {
        let res = stream.write_all(&data.into_bytes()).await;
        if let Err(err) = res {
            println!("Error sending response: {}", err);
        }
    }
}

async fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut buf = BytesMut::with_capacity(MAX_BUFFER_SIZE);
    let len = stream.read_buf(&mut buf).await?;

    let buf = &buf[..len - 1];
    Request::parse(buf)
}

fn echo(req: Request, _directory: &String) -> Response {
    let value = req.path.replace("/echo/", "");
    let headers = HashMap::from([
        (String::from("Content-Length"), value.len().to_string()),
        (String::from("Content-Type"), String::from("text/plain")),
    ]);
    Response {
        code: HttpCode::OK,
        content: Some(value.into_bytes()),
        headers: Some(headers),
    }
}

fn user_agent(req: Request, _directory: &String) -> Response {
    match req.headers.get("User-Agent") {
        Some(value) => {
            let headers = HashMap::from([
                (String::from("Content-Length"), value.len().to_string()),
                (String::from("Content-Type"), String::from("text/plain")),
            ]);
            Response {
                code: HttpCode::OK,
                content: Some(value.to_owned().into_bytes()),
                headers: Some(headers),
            }
        }
        None => Response {
            code: HttpCode::OK,
            content: None,
            headers: None,
        },
    }
}

fn get_file(req: Request, directory: &String) -> Response {
    let Some(filename) = req.path.strip_prefix("/files/") else {
        return Response {
            code: HttpCode::NotFound,
            content: None,
            headers: None,
        };
    };
    let filename = format!("/{}/{}", &directory, filename);
    let path_filename = Path::new(&filename);
    if !path_filename.exists() {
        return Response {
            code: HttpCode::NotFound,
            content: None,
            headers: None,
        };
    }
    match File::open(path_filename) {
        Ok(mut f) => {
            let mut buf = vec![];
            match f.read_to_end(&mut buf) {
                Ok(len) => {
                    let headers = HashMap::from([
                        (String::from("Content-Length"), len.to_string()),
                        (
                            String::from("Content-Type"),
                            String::from("application/octet-stream"),
                        ),
                    ]);
                    Response {
                        code: HttpCode::OK,
                        content: Some(buf),
                        headers: Some(headers),
                    }
                }
                Err(_) => Response {
                    code: HttpCode::NotFound,
                    content: None,
                    headers: None,
                },
            }
        }
        Err(_) => Response {
            code: HttpCode::NotFound,
            content: None,
            headers: None,
        },
    }
}

#[tokio::main]
async fn main() {
    println!("Logs from your program will appear here!");
    let args = env::args().collect::<Vec<String>>();
    let listener = TcpListener::bind("127.0.0.1:4221").await.unwrap();
    if args.len() != 3 && &args[1] != "--directory" {
        println!("Missing params directory");
        return;
    }
    let mut routes = Routes::new(args[2].clone());
    routes.add(Route::new(
        "GET",
        "/",
        CompareType::Exact,
        Box::new(|_, _| Response {
            code: HttpCode::OK,
            headers: None,
            content: None,
        }),
    ));
    routes.add(Route::new(
        "GET",
        "/echo",
        CompareType::Prefix,
        Box::new(echo),
    ));
    routes.add(Route::new(
        "GET",
        "/user-agent",
        CompareType::Exact,
        Box::new(user_agent),
    ));
    routes.add(Route::new(
        "GET",
        "/files",
        CompareType::Prefix,
        Box::new(get_file),
    ));

    let arc_routes = Arc::new(routes);

    loop {
        match listener.accept().await {
            Ok((mut stream, _)) => {
                println!("accepted new connection");
                let routes_clone = arc_routes.clone();
                tokio::spawn(async move {
                    let req = match read_request(&mut stream).await {
                        Ok(val) => val,
                        Err(err) => {
                            println!("error read request: {}", err);
                            return;
                        }
                    };

                    routes_clone.execute(&mut stream, req).await;
                });
            }
            Err(e) => println!("Error: {}", e),
        }
    }
}
