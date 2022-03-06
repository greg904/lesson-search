use std::{
    fs::File,
    io::{self, Read, Write},
    mem,
    net::{TcpListener, TcpStream, ToSocketAddrs},
    os::unix::prelude::{AsRawFd, FromRawFd},
    str,
};

pub(crate) struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

pub(crate) struct Response {
    pub status_code: u32,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub(crate) struct HttpServer {
    listener: TcpListener,
    stop_eventfd: File,
}

fn serve_stream<F: FnMut(Request) -> Response>(
    mut stream: TcpStream,
    respond: &mut F,
) -> io::Result<()> {
    let mut buf = [0u8; 512];
    let mut read = 0;
    while read < buf.len() {
        read += stream.read(&mut buf[read..])?;
        let first_space = match buf.iter().position(|&b| b == b' ') {
            Some(space) => space,
            None => continue,
        };
        let second_space = match buf[(first_space + 1)..].iter().position(|&b| b == b' ') {
            Some(space) => first_space + 1 + space,
            None => continue,
        };
        let req = Request {
            method: str::from_utf8(&buf[..first_space])
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?
                .to_owned(),
            url: str::from_utf8(&buf[(first_space + 1)..second_space])
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?
                .to_owned(),
            // TODO: parse headers
            headers: Vec::new(),
        };
        let res = respond(req);
        let mut res_bytes = Vec::new();
        let human_code = match res.status_code {
            200 => "OK",
            _ => "Unknown",
        };
        write!(
            res_bytes,
            "HTTP/1.1 {} {}",
            res.status_code, human_code
        )?;
        for header in res.headers.iter() {
            write!(res_bytes, "\r\n{}: {}", header.0, header.1)?;
        }
        write!(res_bytes, "\r\n\r\n")?;
        res_bytes.extend_from_slice(&res.body);
        stream.write_all(&res_bytes)?;
        return Ok(());
    }
    return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid HTTP request",
    ));
}

impl HttpServer {
    pub(crate) fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let stop_eventfd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) };
        if stop_eventfd == -1 {
            return Err(io::Error::last_os_error());
        }
        let stop_eventfd = unsafe { File::from_raw_fd(stop_eventfd) };
        Ok(Self {
            listener,
            stop_eventfd,
        })
    }

    pub(crate) fn serve<F: FnMut(Request) -> Response>(&self, mut respond: F) -> io::Result<()> {
        loop {
            let mut fds = [
                libc::pollfd {
                    fd: self.stop_eventfd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let ret = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
            if ret == -1 {
                return Err(io::Error::last_os_error());
            }
            // Check the stop eventfd.
            if fds[0].revents != 0 {
                break;
            }
            if fds[1].revents != 0 {
                let (stream, _) = self.listener.accept().unwrap();
                serve_stream(stream, &mut respond)?;
            }
        }
        Ok(())
    }

    pub(crate) fn stop(&self) {
        let b = 1u64.to_le_bytes();
        let ret =
            unsafe { libc::write(self.stop_eventfd.as_raw_fd(), mem::transmute(&b), b.len()) };
        if ret == -1 {
            panic!("failed to write to eventfd: {}", io::Error::last_os_error());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpStream,
        sync::Arc,
        thread,
    };

    use super::{HttpServer, Response};

    #[test]
    fn basic_test() {
        const ADDR: &str = "127.0.0.1:61458";
        let server = Arc::new(HttpServer::bind(ADDR).unwrap());
        let server_clone = server.clone();
        let client_thread = thread::spawn(move || {
            let mut client = TcpStream::connect(ADDR).unwrap();
            client.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
            let mut res = Vec::new();
            client.read_to_end(&mut res).unwrap();
            server_clone.stop();
            assert_eq!(
                res,
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nHello, world!"
            );
        });
        server
            .serve(|_req| Response {
                status_code: 200,
                headers: vec![("Content-Type".to_owned(), "text/plain".to_owned())],
                body: b"Hello, world!".to_vec(),
            })
            .unwrap();
        client_thread.join().unwrap();
    }
}
