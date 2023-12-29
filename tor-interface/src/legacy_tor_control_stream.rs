// standard
use std::collections::VecDeque;
use std::default::Default;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::option::Option;
use std::string::ToString;
use std::time::Duration;

// extern crates
use regex::Regex;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("control stream read timeout must not be zero")]
    ReadTimeoutZero(),

    #[error("could not connect to control port")]
    CreationFailed(#[source] std::io::Error),

    #[error("configure control port socket failed")]
    ConfigurationFailed(#[source] std::io::Error),

    #[error("control port parsing regex creation failed")]
    ParsingRegexCreationFailed(#[source] regex::Error),

    #[error("control port stream read failure")]
    ReadFailed(#[source] std::io::Error),

    #[error("control port stream closed by remote")]
    ClosedByRemote(),

    #[error("received control port response invalid utf8")]
    InvalidResponse(#[source] std::str::Utf8Error),

    #[error("failed to parse control port reply: {0}")]
    ReplyParseFailed(String),

    #[error("control port stream write failure")]
    WriteFailed(#[source] std::io::Error),
}

pub(crate) struct LegacyControlStream {
    stream: TcpStream,
    closed_by_remote: bool,
    pending_data: Vec<u8>,
    pending_lines: VecDeque<String>,
    pending_reply: Vec<String>,
    reading_multiline_value: bool,
    // regexes used to parse control port responses
    single_line_data: Regex,
    multi_line_data: Regex,
    end_reply_line: Regex,
}

type StatusCode = u32;
pub(crate) struct Reply {
    pub status_code: StatusCode,
    pub reply_lines: Vec<String>,
}

impl LegacyControlStream {
    pub fn new(addr: &SocketAddr, read_timeout: Duration) -> Result<LegacyControlStream, Error> {
        if read_timeout.is_zero() {
            return Err(Error::ReadTimeoutZero());
        }

        let stream = TcpStream::connect(addr).map_err(Error::CreationFailed)?;
        stream
            .set_read_timeout(Some(read_timeout))
            .map_err(Error::ConfigurationFailed)?;

        // pre-allocate a kilobyte for the read buffer
        const READ_BUFFER_SIZE: usize = 1024;
        let pending_data = Vec::with_capacity(READ_BUFFER_SIZE);

        let single_line_data =
            Regex::new(r"^\d\d\d-.*").map_err(Error::ParsingRegexCreationFailed)?;
        let multi_line_data =
            Regex::new(r"^\d\d\d+.*").map_err(Error::ParsingRegexCreationFailed)?;
        let end_reply_line =
            Regex::new(r"^\d\d\d .*").map_err(Error::ParsingRegexCreationFailed)?;

        Ok(LegacyControlStream {
            stream,
            closed_by_remote: false,
            pending_data,
            pending_lines: Default::default(),
            pending_reply: Default::default(),
            reading_multiline_value: false,
            // regex
            single_line_data,
            multi_line_data,
            end_reply_line,
        })
    }

    #[cfg(test)]
    pub(crate) fn closed_by_remote(&mut self) -> bool {
        self.closed_by_remote
    }

    fn read_line(&mut self) -> Result<Option<String>, Error> {
        // read pending bytes from stream until we have a line to return
        while self.pending_lines.is_empty() {
            let byte_count = self.pending_data.len();
            match self.stream.read_to_end(&mut self.pending_data) {
                Err(err) => {
                    if err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut {
                        if byte_count == self.pending_data.len() {
                            return Ok(None);
                        }
                    } else {
                        return Err(Error::ReadFailed(err));
                    }
                }
                Ok(0usize) => {
                    self.closed_by_remote = true;
                    return Err(Error::ClosedByRemote());
                }
                Ok(_count) => (),
            }

            // split our read buffer into individual lines
            let mut begin = 0;
            for index in 1..self.pending_data.len() {
                if self.pending_data[index - 1] == b'\r' && self.pending_data[index] == b'\n' {
                    let end = index - 1;
                    // view into byte vec of just the found line
                    let line_view: &[u8] = &self.pending_data[begin..end];
                    // convert to string
                    let line_string =
                        std::str::from_utf8(line_view).map_err(Error::InvalidResponse)?;

                    // save in pending list
                    self.pending_lines.push_back(line_string.to_string());
                    // update begin (and skip over \r\n)
                    begin = end + 2;
                }
            }
            // leave any leftover bytes in the buffer for the next call
            self.pending_data.drain(0..begin);
        }

        Ok(self.pending_lines.pop_front())
    }

    pub fn read_reply(&mut self) -> Result<Option<Reply>, Error> {
        loop {
            let current_line = match self.read_line()? {
                Some(line) => line,
                None => return Ok(None),
            };

            // make sure the status code matches (if we are not in the
            // middle of a multi-line read
            if let Some(first_line) = self.pending_reply.first() {
                if !self.reading_multiline_value {
                    let first_status_code = &first_line[0..3];
                    let current_status_code = &current_line[0..3];
                    if first_status_code != current_status_code {
                        return Err(Error::ReplyParseFailed(format!(
                            "mismatched status codes, {} != {}",
                            first_status_code, current_status_code
                        )));
                    }
                }
            }

            // end of a response
            if self.end_reply_line.is_match(&current_line) {
                if self.reading_multiline_value {
                    return Err(Error::ReplyParseFailed(
                        "found multi-line end reply but not reading a multi-line reply".to_string(),
                    ));
                }
                self.pending_reply.push(current_line);
                break;
            // single line data from getinfo and friends
            } else if self.single_line_data.is_match(&current_line) {
                if self.reading_multiline_value {
                    return Err(Error::ReplyParseFailed(
                        "found single-line reply but still reading a multi-line reply".to_string(),
                    ));
                }
                self.pending_reply.push(current_line);
            // begin of multiline data from getinfo and friends
            } else if self.multi_line_data.is_match(&current_line) {
                if self.reading_multiline_value {
                    return Err(Error::ReplyParseFailed(
                        "found multi-line start reply but still reading a multi-line reply"
                            .to_string(),
                    ));
                }
                self.pending_reply.push(current_line);
                self.reading_multiline_value = true;
            // multiline data to be squashed to a single entry
            } else {
                if !self.reading_multiline_value {
                    return Err(Error::ReplyParseFailed(
                        "found a multi-line intermediate reply but not reading a multi-line reply"
                            .to_string(),
                    ));
                }
                // don't bother writing the end of multiline token
                if current_line == "." {
                    self.reading_multiline_value = false;
                } else {
                    let multiline = match self.pending_reply.last_mut() {
                        Some(multiline) => multiline,
                        // if our logic here is right, then
                        // self.reading_multiline_value == !self.pending_reply.is_empty()
                        // should always be true regardless of the data received
                        // from the control port
                        None => unreachable!(),
                    };
                    multiline.push('\n');
                    multiline.push_str(&current_line);
                }
            }
        }

        // take ownership of the reply lines
        let mut reply_lines: Vec<String> = Default::default();
        std::mem::swap(&mut self.pending_reply, &mut reply_lines);

        // parse out the response code for easier matching
        let status_code_string = match reply_lines.first() {
            Some(line) => line[0..3].to_string(),
            // the lines have already been parsed+validated in the above loop
            None => unreachable!(),
        };
        let status_code: u32 = match status_code_string.parse() {
            Ok(status_code) => status_code,
            Err(_) => {
                return Err(Error::ReplyParseFailed(format!(
                    "unable to parse '{}' as status code",
                    status_code_string
                )))
            }
        };

        // strip the redundant status code from start of lines
        for line in reply_lines.iter_mut() {
            if line.starts_with(&status_code_string) {
                *line = line[4..].to_string();
            }
        }

        Ok(Some(Reply {
            status_code,
            reply_lines,
        }))
    }

    pub fn write(&mut self, cmd: &str) -> Result<(), Error> {
        if let Err(err) = write!(self.stream, "{}\r\n", cmd) {
            self.closed_by_remote = true;
            return Err(Error::WriteFailed(err));
        }
        Ok(())
    }
}
