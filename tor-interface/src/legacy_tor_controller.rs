// standard
use std::default::Default;
use std::net::SocketAddr;
use std::option::Option;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::string::ToString;
#[cfg(test)]
use std::time::{Duration, Instant};

// extern crates
use hmac::Mac;
use rand::rngs::OsRng;
use rand::TryRngCore;
use regex::Regex;
#[cfg(test)]
use serial_test::serial;

// internal crates
use crate::legacy_tor_control_stream::*;
#[cfg(test)]
use crate::legacy_tor_process::*;
use crate::legacy_tor_version::*;
use crate::tor_crypto::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("response regex creation failed")]
    ParsingRegexCreationFailed(#[source] regex::Error),

    #[error("control stream read reply failed")]
    ReadReplyFailed(#[source] crate::legacy_tor_control_stream::Error),

    #[error("unexpected synchronous reply recieved")]
    UnexpectedSynchonousReplyReceived(),

    #[error("control stream write command failed")]
    WriteCommandFailed(#[source] crate::legacy_tor_control_stream::Error),

    #[error("invalid command arguments: {0}")]
    InvalidCommandArguments(String),

    #[error("command failed: {0} {}", .1.join("\n"))]
    CommandFailed(u32, Vec<String>),

    #[error("failed to parse command reply: {0}")]
    CommandReplyParseFailed(String),

    #[error("failed to parse received tor version")]
    TorVersionParseFailed(#[source] crate::legacy_tor_version::Error),

    #[error("unable to read cookie file: {1:?}")]
    CookieReadingFailed(#[source] std::io::Error, PathBuf),

    #[error("[SAFE]COOKIE authentication not supported")]
    CookiesNotSupported(),

    #[error("impostor sent invalid SAFECOOKIE HMAC")]
    BadCookieHash(),

    #[error("failed to generate random data")]
    RngError(#[source] <OsRng as TryRngCore>::Error),
}

// Per-command data
#[derive(Default)]
pub(crate) struct AddOnionFlags {
    pub discard_pk: bool,
    pub detach: bool,
    pub v3_auth: bool,
    pub non_anonymous: bool,
    pub max_streams_close_circuit: bool,
}

#[derive(Default)]
pub(crate) struct OnionClientAuthAddFlags {
    pub permanent: bool,
}

pub(crate) enum AsyncEvent {
    Unknown {
        lines: Vec<String>,
    },
    StatusClient {
        severity: String,
        action: String,
        arguments: Vec<(String, String)>,
    },
    HsDesc {
        action: String,
        hs_address: V3OnionServiceId,
    },
}

#[derive(Default, Debug, PartialEq, Eq)]
struct ProtocolInfo {
    auth_cookie: bool,
    auth_safecookie: bool,
    auth_null: bool,
    cookiefile: PathBuf,
}

pub(crate) struct LegacyTorController {
    // underlying control stream
    control_stream: LegacyControlStream,
    // list of async replies to be handled
    async_replies: Vec<Reply>,
    // regex for parsing events
    status_event_pattern: Regex,
    status_event_argument_pattern: Regex,
    hs_desc_pattern: Regex,
    protocolinfo_data: Option<ProtocolInfo>,
}

fn quoted_string(string: &str) -> String {
    // replace \ with \\ and " with \"
    // see: https://spec.torproject.org/control-spec/message-format.html?highlight=QuotedString#description-format
    string.replace("\\", "\\\\").replace("\"", "\\\"")
}


// All authentication cookies are 32 bytes long.  Controllers MUST NOT
// use the contents of a non-32-byte-long file as an authentication
// cookie.
pub(crate) fn read_cookie(from: &Path) -> std::io::Result<[u8; 32]> {
    let mut f = std::fs::File::open(from)?;
    let mut ret = [0u8; 32];
    f.read_exact(&mut ret[..])?;
    let mut nonce = [0u8; 1];
    if f.read_exact(&mut nonce[..]).is_ok() {
        Err(std::io::Error::new(std::io::ErrorKind::FileTooLarge, "cookies are 32 bytes"))
    } else {
        Ok(ret)
    }
}

fn tonibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => 0xA + (c - b'a'),
        b'A'..=b'F' => 0xA + (c - b'A'),
        _ => unreachable!(),
    }
}

fn hmac_sha256(key: &str, blob1: &[u8], blob2: &[u8], blob3: &[u8]) -> hmac::Hmac<sha2::Sha256> {
    let mut hmac = hmac::Hmac::new_from_slice(key.as_bytes()).unwrap();
    hmac.update(blob1);
    hmac.update(blob2);
    hmac.update(blob3);
    hmac
}

fn reply_ok(reply: Reply) -> Result<Reply, Error> {
    match reply.status_code {
        250u32 => Ok(reply),
        code => Err(Error::CommandFailed(code, reply.reply_lines)),
    }
}


// https://raw.githubusercontent.com/torproject/torspec/refs/heads/main/control-spec.txt
// 250-AUTH METHODS=COOKIE,SAFECOOKIE COOKIEFILE="/home/nabijaczleweli/.tor/control_auth_cookie"
// 250-AUTH METHODS=HASHEDPASSWORD
// 250-AUTH METHODS=COOKIE,SAFECOOKIE,HASHEDPASSWORD COOKIEFILE="/home/nabijaczleweli/.tor/coo kie \\\" \320\266 \n 2"
// 250-AUTH METHODS=COOKIE,SAFECOOKIE,HASHEDPASSWORD COOKIEFILE="/home/nabijaczleweli/.tor/C/\001\002\003\004\005\006\007\010\t\n\013\014\r\016\017\020\021\022\023\024\025\026\027\030\031\032\033\034\035\036\037 !\"#$%&\'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\177\200\201\202\203\204\205\206\207\210\211\212\213\214\215\216\217\220\221\222\223\224\225\226\227\230\231\232\233\234\235\236\237\240\241\242\243\244\245\246\247\250\251\252\253\254\255\256\257\260\261\262\263\264\265\266\267\270\271\272\273\274\275\276\277\300\301\302\303\304\305\306\307\310\311\312\313\314\315\316\317\320\321\322\323\324\325\326\327\330\331\332\333\334\335\336\337\340\341\342\343\344\345\346\347\350\351\352\353\354\355\356\357\360\361\362\363\364\365\366\367\370\371\372\373\374\375\376\377"
// 250-AUTH METHODS=NULL
fn parse_auth_methods(auth: &str) -> ProtocolInfo {
    let mut ret = ProtocolInfo::default();
    let mut two = auth["AUTH METHODS=".len()..].splitn(2, ' ');
    if let Some(methods) = two.next() {
        for m in methods.split(',') {
            match m {
                "COOKIE" => ret.auth_cookie = true,
                "SAFECOOKIE" => ret.auth_safecookie = true,
                "NULL" => ret.auth_null = true,
                _ => {}
            }
        }
    }
    let remainder = two.next();
    if (ret.auth_cookie || ret.auth_safecookie) && remainder.map(|r| r.starts_with("COOKIEFILE=\"")).unwrap_or(false) {
        let mut remainder = remainder.unwrap()["COOKIEFILE=\"".len()..].as_bytes();

        let mut path = vec![];
        // https://datatracker.ietf.org/doc/html/rfc2822 qcontent
        while let Some(mut byte) = remainder.get(0).copied() {
            if byte == b'"' {
                break;
            }
            remainder = &remainder[1..];
            if byte == b'\\' {
                let mut consume = 1;
                match (remainder.get(0), remainder.get(1), remainder.get(2)) {
                    (Some(b't'), ..) => byte = b'\t',
                    (Some(b'n'), ..) => byte = b'\n',
                    (Some(b'r'), ..) => byte = b'\r',
                    (Some(b'\"'), ..) => byte = b'\"',
                    (Some(b'\''), ..) => byte = b'\'',
                    (Some(b'\\'), ..) => byte = b'\\',
                    (Some(h @ b'0'..=b'3'), Some(t @ b'0'..=b'7'), Some(u @ b'0'..=b'7')) => {
                        byte = ((h - b'0') << 6) | ((t - b'0') << 3) | (u - b'0');
                        consume = 3;
                    }
                    _ => {
                        path.clear();
                        break;
                    }
                }
                remainder = &remainder[consume..];
            }
            path.push(byte);
        }
        // On UNIX, paths are sequences of non-0 bytes. We know this.
        // On tor/Win32, paths are sequences of ASCII bytes(?): https://101010.pl/@nabijaczleweli/114655491521731646
        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            ret.cookiefile = OsString::from_vec(path.into()).into();
        }
        #[cfg(not(unix))]
        {
            // TODO: string_from_utf8_lossy_owned
            ret.cookiefile = String::from_utf8_lossy(&path).into();
        }
    }
    ret
}

#[cfg(test)]
#[test]
fn parse_auth_methods_test() {
    assert_eq!(parse_auth_methods(r####"AUTH METHODS=COOKIE,SAFECOOKIE COOKIEFILE="/home/nabijaczleweli/.tor/control_auth_cookie""####), ProtocolInfo {
        auth_cookie: true,
        auth_safecookie: true,
        auth_null: false,
        cookiefile: Path::new("/home/nabijaczleweli/.tor/control_auth_cookie").to_owned(),
    });
    assert_eq!(parse_auth_methods(r####"AUTH METHODS=HASHEDPASSWORD"####), ProtocolInfo::default());
    assert_eq!(parse_auth_methods(r####"AUTH METHODS=COOKIE,SAFECOOKIE,HASHEDPASSWORD COOKIEFILE="/home/nabijaczleweli/.tor/coo kie \\\" \320\266 \n 2""####), ProtocolInfo {
        auth_cookie: true,
        auth_safecookie: true,
        auth_null: false,
        cookiefile: Path::new("/home/nabijaczleweli/.tor/coo kie \\\" ж \n 2").to_owned(),
    });
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let mut buf = b"/home/nabijaczleweli/.tor/C/"[..].to_owned();
        for b in 1..=0xFF {
            buf.push(b);
        }
        assert_eq!(parse_auth_methods(r####"AUTH METHODS=COOKIE,SAFECOOKIE,HASHEDPASSWORD COOKIEFILE="/home/nabijaczleweli/.tor/C/\001\002\003\004\005\006\007\010\t\n\013\014\r\016\017\020\021\022\023\024\025\026\027\030\031\032\033\034\035\036\037 !\"#$%&\'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\177\200\201\202\203\204\205\206\207\210\211\212\213\214\215\216\217\220\221\222\223\224\225\226\227\230\231\232\233\234\235\236\237\240\241\242\243\244\245\246\247\250\251\252\253\254\255\256\257\260\261\262\263\264\265\266\267\270\271\272\273\274\275\276\277\300\301\302\303\304\305\306\307\310\311\312\313\314\315\316\317\320\321\322\323\324\325\326\327\330\331\332\333\334\335\336\337\340\341\342\343\344\345\346\347\350\351\352\353\354\355\356\357\360\361\362\363\364\365\366\367\370\371\372\373\374\375\376\377""####), ProtocolInfo {
            auth_cookie: true,
            auth_safecookie: true,
            auth_null: false,
            cookiefile: OsString::from_vec(buf).into(),
        });
    }
    assert_eq!(parse_auth_methods(r####"AUTH METHODS=NULL"####), ProtocolInfo {
        auth_null: true,
        ..ProtocolInfo::default()
    });
}

impl LegacyTorController {
    pub fn new(control_stream: LegacyControlStream) -> Result<LegacyTorController, Error> {
        let status_event_pattern =
            Regex::new(r#"^STATUS_CLIENT (?P<severity>NOTICE|WARN|ERR) (?P<action>[A-Za-z]+)"#)
                .map_err(Error::ParsingRegexCreationFailed)?;
        let status_event_argument_pattern =
            Regex::new(r#"(?P<key>[A-Z]+)=(?P<value>[A-Za-z0-9_]+|"[^"]+")"#)
                .map_err(Error::ParsingRegexCreationFailed)?;
        let hs_desc_pattern = Regex::new(
            r#"HS_DESC (?P<action>REQUESTED|UPLOAD|RECEIVED|UPLOADED|IGNORE|FAILED|CREATED) (?P<hsaddress>[a-z2-7]{56})"#
        ).map_err(Error::ParsingRegexCreationFailed)?;

        Ok(LegacyTorController {
            control_stream,
            async_replies: Default::default(),
            // regex
            status_event_pattern,
            status_event_argument_pattern,
            hs_desc_pattern,
            protocolinfo_data: None,
        })
    }

    // return curently available events, does not block waiting
    // for an event
    fn wait_async_replies(&mut self) -> Result<Vec<Reply>, Error> {
        let mut replies: Vec<Reply> = Default::default();
        // take any previously received async replies
        std::mem::swap(&mut self.async_replies, &mut replies);

        // and keep consuming until none are available
        loop {
            if let Some(reply) = self
                .control_stream
                .read_reply()
                .map_err(Error::ReadReplyFailed)?
            {
                replies.push(reply);
            } else {
                // no more replies immediately available so return
                return Ok(replies);
            }
        }
    }

    fn reply_to_event(&self, reply: &mut Reply) -> Result<AsyncEvent, Error> {
        if reply.status_code != 650u32 {
            return Err(Error::UnexpectedSynchonousReplyReceived());
        }

        // not sure this is what we want but yolo
        let reply_text = reply.reply_lines.join(" ");
        if let Some(caps) = self.status_event_pattern.captures(&reply_text) {
            let severity = match caps.name("severity") {
                Some(severity) => severity.as_str(),
                None => unreachable!(),
            };
            let action = match caps.name("action") {
                Some(action) => action.as_str(),
                None => unreachable!(),
            };

            let mut arguments: Vec<(String, String)> = Default::default();
            for caps in self
                .status_event_argument_pattern
                .captures_iter(&reply_text)
            {
                let key = match caps.name("key") {
                    Some(key) => key.as_str(),
                    None => unreachable!(),
                };
                let value = {
                    let value = match caps.name("value") {
                        Some(value) => value.as_str(),
                        None => unreachable!(),
                    };
                    if value.starts_with('\"') && value.ends_with('\"') {
                        &value[1..value.len() - 1]
                    } else {
                        value
                    }
                };
                arguments.push((key.to_string(), value.to_string()));
            }

            return Ok(AsyncEvent::StatusClient {
                severity: severity.to_string(),
                action: action.to_string(),
                arguments,
            });
        }

        if let Some(caps) = self.hs_desc_pattern.captures(&reply_text) {
            let action = match caps.name("action") {
                Some(action) => action.as_str(),
                None => unreachable!(),
            };
            let hs_address = match caps.name("hsaddress") {
                Some(hs_address) => hs_address.as_str(),
                None => unreachable!(),
            };

            if let Ok(hs_address) = V3OnionServiceId::from_string(hs_address) {
                return Ok(AsyncEvent::HsDesc {
                    action: action.to_string(),
                    hs_address,
                });
            }
        }

        // no luck parsing reply, just return full text
        let mut reply_lines: Vec<String> = Default::default();
        std::mem::swap(&mut reply_lines, &mut reply.reply_lines);

        Ok(AsyncEvent::Unknown { lines: reply_lines })
    }

    pub fn wait_async_events(&mut self) -> Result<Vec<AsyncEvent>, Error> {
        let mut async_replies = self.wait_async_replies()?;
        let mut async_events: Vec<AsyncEvent> = Default::default();

        for reply in async_replies.iter_mut() {
            async_events.push(self.reply_to_event(reply)?);
        }

        Ok(async_events)
    }

    // wait for a sync reply, save off async replies for later
    fn wait_sync_reply(&mut self) -> Result<Reply, Error> {
        loop {
            if let Some(reply) = self
                .control_stream
                .read_reply()
                .map_err(Error::ReadReplyFailed)?
            {
                match reply.status_code {
                    650u32 => self.async_replies.push(reply),
                    _ => return Ok(reply),
                }
            }
        }
    }

    fn write_command(&mut self, text: &str) -> Result<Reply, Error> {
        self.control_stream
            .write(text)
            .map_err(Error::WriteCommandFailed)?;
        self.wait_sync_reply()
    }

    //
    // Tor Commands
    //
    // The section where we can find the specification in control-spec.txt
    // for the underlying command is listed in parentheses
    //
    // Each of these command wrapper methods block until completion
    //

    // SETCONF (3.1)
    fn setconf_cmd(&mut self, key_values: &[(&str, String)]) -> Result<Reply, Error> {
        if key_values.is_empty() {
            return Err(Error::InvalidCommandArguments(
                "SETCONF key-value pairs list must not be empty".to_string(),
            ));
        }
        let mut command_buffer = vec!["SETCONF".to_string()];

        for (key, value) in key_values.iter() {
            command_buffer.push(format!("{}=\"{}\"", key, quoted_string(value.trim())));
        }
        let command = command_buffer.join(" ");

        self.write_command(&command)
    }

    // GETCONF (3.3)
    #[cfg(test)]
    fn getconf_cmd(&mut self, keywords: &[&str]) -> Result<Reply, Error> {
        if keywords.is_empty() {
            return Err(Error::InvalidCommandArguments(
                "GETCONF keywords list must not be empty".to_string(),
            ));
        }
        let command = format!("GETCONF {}", keywords.join(" "));

        self.write_command(&command)
    }

    // SETEVENTS (3.4)
    fn setevents_cmd(&mut self, event_codes: &[&str]) -> Result<Reply, Error> {
        if event_codes.is_empty() {
            return Err(Error::InvalidCommandArguments(
                "SETEVENTS event codes list mut not be empty".to_string(),
            ));
        }
        let command = format!("SETEVENTS {}", event_codes.join(" "));

        self.write_command(&command)
    }

    // AUTHENTICATE (3.5)
    fn authenticate_cmd(&mut self, password: &str) -> Result<Reply, Error> {
        let command = format!("AUTHENTICATE \"{}\"", quoted_string(password));

        self.write_command(&command)
    }

    // AUTHENTICATE (3.5)
    fn authenticate_cmd_cookie(&mut self, cookie: &[u8]) -> Result<Reply, Error> {
        let mut command = b"AUTHENTICATE "[..].to_owned();
        for b in cookie {
            write!(&mut command, "{:02x}", b).map_err(|e| Error::InvalidCommandArguments(e.to_string()))?;
        }

        self.write_command(unsafe { str::from_utf8_unchecked(&command) })
    }

    // AUTHCHALLENGE (3.24)
    fn authchallenge_cmd(&mut self, client_nonce: &[u8]) -> Result<Reply, Error> {
        let mut command = b"AUTHCHALLENGE SAFECOOKIE "[..].to_owned();
        for b in client_nonce {
            write!(&mut command, "{:02x}", b).map_err(|e| Error::InvalidCommandArguments(e.to_string()))?;
        }

        self.write_command(unsafe { str::from_utf8_unchecked(&command) })
    }

    // PROTOCOLINFO (3.21)
    fn protocolinfo_cmd(&mut self) -> Result<Reply, Error> {
        self.write_command("PROTOCOLINFO 1")
    }

    // GETINFO (3.9)
    fn getinfo_cmd(&mut self, keywords: &[&str]) -> Result<Reply, Error> {
        if keywords.is_empty() {
            return Err(Error::InvalidCommandArguments(
                "GETINFO keywords list must not be empty".to_string(),
            ));
        }
        let command = format!("GETINFO {}", keywords.join(" "));

        self.write_command(&command)
    }

    // ADD_ONION (3.27)
    fn add_onion_cmd(
        &mut self,
        key: Option<&Ed25519PrivateKey>,
        flags: &AddOnionFlags,
        max_streams: Option<u16>,
        virt_port: u16,
        target: Option<SocketAddr>,
        client_auth: Option<&[X25519PublicKey]>,
    ) -> Result<Reply, Error> {
        let mut command_buffer = vec!["ADD_ONION".to_string()];

        // set our key or request a new one
        if let Some(key) = key {
            command_buffer.push(key.to_key_blob());
        } else {
            command_buffer.push("NEW:ED25519-V3".to_string());
        }

        // set our flags
        let mut flag_buffer: Vec<&str> = Default::default();
        if flags.discard_pk {
            flag_buffer.push("DiscardPK");
        }
        if flags.detach {
            flag_buffer.push("Detach");
        }
        if flags.v3_auth {
            flag_buffer.push("V3Auth");
        }
        if flags.non_anonymous {
            flag_buffer.push("NonAnonymous");
        }
        if flags.max_streams_close_circuit {
            flag_buffer.push("MaxStreamsCloseCircuit");
        }

        if !flag_buffer.is_empty() {
            command_buffer.push(format!("Flags={}", flag_buffer.join(",")));
        }

        // set max concurrent streams
        if let Some(max_streams) = max_streams {
            command_buffer.push(format!("MaxStreams={}", max_streams));
        }

        // set our onion service target
        if let Some(target) = target {
            command_buffer.push(format!("Port={},{}", virt_port, target));
        } else {
            command_buffer.push(format!("Port={}", virt_port));
        }
        // setup client auth
        if let Some(client_auth) = client_auth {
            for key in client_auth.iter() {
                command_buffer.push(format!("ClientAuthV3={}", key.to_base32()));
            }
        }

        // finally send the command
        let command = command_buffer.join(" ");

        self.write_command(&command)
    }

    // DEL_ONION (3.38)
    fn del_onion_cmd(&mut self, service_id: &V3OnionServiceId) -> Result<Reply, Error> {
        let command = format!("DEL_ONION {}", service_id);

        self.write_command(&command)
    }

    // ONION_CLIENT_AUTH_ADD (3.30)
    fn onion_client_auth_add_cmd(
        &mut self,
        service_id: &V3OnionServiceId,
        private_key: &X25519PrivateKey,
        client_name: Option<String>,
        flags: &OnionClientAuthAddFlags,
    ) -> Result<Reply, Error> {
        let mut command_buffer = vec!["ONION_CLIENT_AUTH_ADD".to_string()];

        // set the onion service id
        command_buffer.push(service_id.to_string());

        // set our client's private key
        command_buffer.push(format!("x25519:{}", private_key.to_base64()));

        if let Some(client_name) = client_name {
            command_buffer.push(format!("ClientName={}", client_name));
        }

        if flags.permanent {
            command_buffer.push("Flags=Permanent".to_string());
        }

        // finally send command
        let command = command_buffer.join(" ");

        self.write_command(&command)
    }

    // ONION_CLIENT_AUTH_REMOVE (3.31)
    fn onion_client_auth_remove_cmd(
        &mut self,
        service_id: &V3OnionServiceId,
    ) -> Result<Reply, Error> {
        let command = format!("ONION_CLIENT_AUTH_REMOVE {}", service_id);

        self.write_command(&command)
    }

    //
    // Public high-level typesafe command method wrappers
    //

    pub fn setconf(&mut self, key_values: &[(&str, String)]) -> Result<(), Error> {
        self.setconf_cmd(key_values).and_then(reply_ok).map(|_| ())
    }

    #[cfg(test)]
    pub fn getconf(&mut self, keywords: &[&str]) -> Result<Vec<(String, String)>, Error> {
        let reply = self.getconf_cmd(keywords).and_then(reply_ok)?;

        let mut key_values: Vec<(String, String)> = Default::default();
        for line in reply.reply_lines {
            match line.find('=') {
                Some(index) => key_values
                    .push((line[0..index].to_string(), line[index + 1..].to_string())),
                None => key_values.push((line, String::new())),
            }
        }
        Ok(key_values)
    }

    pub fn setevents(&mut self, events: &[&str]) -> Result<(), Error> {
        self.setevents_cmd(events).and_then(reply_ok).map(|_| ())
    }

    pub fn authenticate(&mut self, password: &str) -> Result<(), Error> {
        self.authenticate_cmd(password).and_then(reply_ok).map(|_| ())
    }

    fn ensure_protocolinfo(&mut self) {
        if self.protocolinfo_data.is_some() {
            return;
        }

        // https://raw.githubusercontent.com/torproject/torspec/refs/heads/main/control-spec.txt
        // 250-VERSION Tor=\"0.4.7.16\"
        match self.protocolinfo_cmd() {
            Ok(reply) if reply.status_code == 250 => {
                self.protocolinfo_data = Some(reply.reply_lines.iter()
                    .find(|l| l.starts_with("AUTH METHODS="))
                    .map(|auth| parse_auth_methods(auth))
                    .unwrap_or_default());
            }
            _ => self.protocolinfo_data = Some(Default::default()),
        }
    }

    fn authenticate_safecookie(&mut self, data: [u8; 32]) -> Result<Reply, Error> {
        let mut client_nonce = [0u8; 32];
        OsRng.try_fill_bytes(&mut client_nonce).map_err(Error::RngError)?;
        let reply = self.authchallenge_cmd(&client_nonce).and_then(reply_ok)?;

        if reply.reply_lines.len() != 1 || !reply.reply_lines[0].starts_with("AUTHCHALLENGE SERVERHASH=") {
            return Err(Error::CommandReplyParseFailed(reply.reply_lines.get(0).cloned().unwrap_or_else(|| "[no response]".to_string())));
        }
        let mut chunks = reply.reply_lines[0]["AUTHCHALLENGE SERVERHASH=".len()..].splitn(2, ' ');

        let sh = chunks.next().map(|sh| sh.as_bytes())
            .filter(|sh| sh.len() % 64 == 0)
            .filter(|sh| sh.iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')))
            .ok_or_else(|| Error::CommandReplyParseFailed(reply.reply_lines[0].clone()))?;
        let mut server_hash = Vec::new();
        server_hash.resize(sh.len() / 2, 0);
        for (hilo, dest) in sh.chunks_exact(2).zip(server_hash.iter_mut()) {
            *dest = tonibble(hilo[0]) << 4 | tonibble(hilo[1]);
        }

        let sn = chunks.next().map(|sh| sh.as_bytes())
            .filter(|sn| sn.starts_with(b"SERVERNONCE="))
            .map(|sn| &sn[b"SERVERNONCE=".len()..])
            .filter(|sh| sh.len() == 64)
            .filter(|sh| sh.iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')))
            .ok_or_else(|| Error::CommandReplyParseFailed(reply.reply_lines[0].clone()))?;
        let mut server_nonce = [0u8; 32];
        for (hilo, dest) in sn.chunks_exact(2).zip(server_nonce.iter_mut()) {
            *dest = tonibble(hilo[0]) << 4 | tonibble(hilo[1]);
        }

        hmac_sha256("Tor safe cookie authentication server-to-controller hash", &data, &client_nonce, &server_nonce)
            .verify_slice(&server_hash).map_err(|_| Error::BadCookieHash())?;

        self.authenticate_cmd_cookie(
            hmac_sha256("Tor safe cookie authentication controller-to-server hash", &data, &client_nonce, &server_nonce).finalize().into_bytes().as_slice())
    }

    pub fn authenticate_cookie(&mut self, data: [u8; 32]) -> Result<(), Error> {
        self.ensure_protocolinfo();
        let Some(pi) = self.protocolinfo_data.as_ref()
            else { unreachable!() };

        let reply = if pi.auth_safecookie && pi.auth_cookie {
            match self.authenticate_safecookie(data) {
                r @ Ok(_) | r @ Err(Error::BadCookieHash()) => r?,
                _ => self.authenticate_cmd_cookie(&data)?,
            }
        } else if pi.auth_safecookie {
            self.authenticate_safecookie(data)?
        } else if pi.auth_cookie {
            self.authenticate_cmd_cookie(&data)?
        } else {
            return Err(Error::CookiesNotSupported());
        };

        reply_ok(reply).map(|_| ())
    }

    pub fn authenticate_auto(&mut self) -> Result<(), Error> {
        self.ensure_protocolinfo();
        let Some(pi) = self.protocolinfo_data.as_ref()
            else { unreachable!() };

        if pi.auth_null {
            self.authenticate("")
        } else if (pi.auth_cookie || pi.auth_safecookie) && pi.cookiefile != Path::new("") {
            self.authenticate_cookie(read_cookie(&pi.cookiefile).map_err(|e| Error::CookieReadingFailed(e, pi.cookiefile.clone()))?)
        } else {
            self.authenticate("") // fallback
        }
    }

    pub fn getinfo(&mut self, keywords: &[&str]) -> Result<Vec<(String, String)>, Error> {
        let reply = self.getinfo_cmd(keywords).and_then(reply_ok)?;

        let mut key_values: Vec<(String, String)> = Default::default();
        for line in reply.reply_lines {
            match line.find('=') {
                Some(index) => key_values
                    .push((line[0..index].to_string(), line[index + 1..].to_string())),
                None => {
                    if line != "OK" {
                        key_values.push((line, String::new()))
                    }
                }
            }
        }
        Ok(key_values)
    }

    pub fn add_onion(
        &mut self,
        key: Option<&Ed25519PrivateKey>,
        flags: &AddOnionFlags,
        max_streams: Option<u16>,
        virt_port: u16,
        target: Option<SocketAddr>,
        client_auth: Option<&[X25519PublicKey]>,
    ) -> Result<(Option<Ed25519PrivateKey>, V3OnionServiceId), Error> {
        let reply = self.add_onion_cmd(key, flags, max_streams, virt_port, target, client_auth).and_then(reply_ok)?;

        let mut private_key: Option<Ed25519PrivateKey> = None;
        let mut service_id: Option<V3OnionServiceId> = None;

        for line in reply.reply_lines {
            if let Some(mut index) = line.find("ServiceID=") {
                if service_id.is_some() {
                    return Err(Error::CommandReplyParseFailed(
                        "received duplicate ServiceID entries".to_string(),
                    ));
                }
                index += "ServiceId=".len();
                let service_id_string = &line[index..];
                service_id = match V3OnionServiceId::from_string(service_id_string) {
                    Ok(service_id) => Some(service_id),
                    Err(_) => {
                        return Err(Error::CommandReplyParseFailed(format!(
                            "could not parse '{}' as V3OnionServiceId",
                            service_id_string
                        )))
                    }
                }
            } else if let Some(mut index) = line.find("PrivateKey=") {
                if private_key.is_some() {
                    return Err(Error::CommandReplyParseFailed(
                        "received duplicate PrivateKey entries".to_string(),
                    ));
                }
                index += "PrivateKey=".len();
                let key_blob_string = &line[index..];
                private_key = match Ed25519PrivateKey::from_key_blob_legacy(key_blob_string)
                {
                    Ok(private_key) => Some(private_key),
                    Err(_) => {
                        return Err(Error::CommandReplyParseFailed(format!(
                            "could not parse {} as Ed25519PrivateKey",
                            key_blob_string
                        )))
                    }
                };
            } else if line.contains("ClientAuthV3=") {
                if client_auth.unwrap_or_default().is_empty() {
                    return Err(Error::CommandReplyParseFailed(
                        "recieved unexpected ClientAuthV3 keys".to_string(),
                    ));
                }
            } else if !line.contains("OK") {
                return Err(Error::CommandReplyParseFailed(format!(
                    "received unexpected reply line '{}'",
                    line
                )));
            }
        }

        if flags.discard_pk {
            if private_key.is_some() {
                return Err(Error::CommandReplyParseFailed(
                    "PrivateKey response should have been discard".to_string(),
                ));
            }
        } else if private_key.is_none() {
            return Err(Error::CommandReplyParseFailed(
                "did not receive a PrivateKey".to_string(),
            ));
        }

        match service_id {
            Some(service_id) => Ok((private_key, service_id)),
            None => Err(Error::CommandReplyParseFailed(
                "did not receive a ServiceID".to_string(),
            )),
        }
    }

    pub fn del_onion(&mut self, service_id: &V3OnionServiceId) -> Result<(), Error> {
        self.del_onion_cmd(service_id).and_then(reply_ok).map(|_| ())
    }

    // more specific encapulsation of specific command invocations

    pub fn getinfo_net_listeners_socks(&mut self) -> Result<Vec<SocketAddr>, Error> {
        let response = self.getinfo(&["net/listeners/socks"])?;
        for (key, value) in response.iter() {
            if key.as_str() == "net/listeners/socks" {
                if value.is_empty() {
                    return Ok(Default::default());
                }
                // get our list of double-quoted strings
                let listeners: Vec<&str> = value.split(' ').collect();
                let mut result: Vec<SocketAddr> = Default::default();
                for socket_addr in listeners.iter() {
                    if !socket_addr.starts_with('\"') || !socket_addr.ends_with('\"') {
                        return Err(Error::CommandReplyParseFailed(format!(
                            "could not parse '{}' as socket address",
                            socket_addr
                        )));
                    }

                    // remove leading/trailing double quote
                    let stripped = &socket_addr[1..socket_addr.len() - 1];
                    result.push(match SocketAddr::from_str(stripped) {
                        Ok(result) => result,
                        Err(_) => {
                            return Err(Error::CommandReplyParseFailed(format!(
                                "could not parse '{}' as socket address",
                                socket_addr
                            )))
                        }
                    });
                }
                return Ok(result);
            }
        }
        Err(Error::CommandReplyParseFailed(
            "reply did not find a 'net/listeners/socks' key/value".to_string(),
        ))
    }

    pub fn getinfo_version(&mut self) -> Result<LegacyTorVersion, Error> {
        let response = self.getinfo(&["version"])?;
        for (key, value) in response.iter() {
            if key.as_str() == "version" {
                return LegacyTorVersion::from_str(value).map_err(Error::TorVersionParseFailed);
            }
        }
        Err(Error::CommandReplyParseFailed(
            "did not find a 'version' key/value".to_string(),
        ))
    }

    pub fn onion_client_auth_add(
        &mut self,
        service_id: &V3OnionServiceId,
        private_key: &X25519PrivateKey,
        client_name: Option<String>,
        flags: &OnionClientAuthAddFlags,
    ) -> Result<(), Error> {
        let reply = self.onion_client_auth_add_cmd(service_id, private_key, client_name, flags)?;

        match reply.status_code {
            250u32..=252u32 => Ok(()),
            code => Err(Error::CommandFailed(code, reply.reply_lines)),
        }
    }

    #[allow(dead_code)]
    pub fn onion_client_auth_remove(&mut self, service_id: &V3OnionServiceId) -> Result<(), Error> {
        let reply = self.onion_client_auth_remove_cmd(service_id)?;

        match reply.status_code {
            250u32..=251u32 => Ok(()),
            code => Err(Error::CommandFailed(code, reply.reply_lines)),
        }
    }
}

#[test]
#[serial]
fn test_tor_controller() -> anyhow::Result<()> {
    let tor_path = which::which(format!("tor{}", std::env::consts::EXE_SUFFIX))?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_tor_controller");
    let tor_process = LegacyTorProcess::new(&tor_path, &data_path)?;

    // create a scope to ensure tor_controller is dropped
    {
        let control_stream =
            LegacyControlStream::new(tor_process.get_control_addr(), Duration::from_millis(16))?;

        // create a tor controller and send authentication command
        let mut tor_controller = LegacyTorController::new(control_stream)?;
        tor_controller.authenticate_cmd(tor_process.get_password())?;
        assert!(
            tor_controller
                .authenticate_cmd("invalid password")?
                .status_code
                == 515u32
        );

        // tor controller should have shutdown the connection after failed authentication
        assert!(
            tor_controller
                .authenticate_cmd(tor_process.get_password())
                .is_err(),
            "expected failure due to closed connection"
        );
        assert!(tor_controller.control_stream.closed_by_remote());
    }
    // now create a second controller
    {
        let control_stream =
            LegacyControlStream::new(tor_process.get_control_addr(), Duration::from_millis(16))?;

        // create a tor controller and send authentication command
        // all async events are just printed to stdout
        let mut tor_controller = LegacyTorController::new(control_stream)?;
        tor_controller.authenticate(tor_process.get_password())?;

        // ensure everything is matching our default_torrc settings
        let vals = tor_controller.getconf(&["SocksPort", "AvoidDiskWrites", "DisableNetwork"])?;
        for (key, value) in vals.iter() {
            let expected = match key.as_str() {
                "SocksPort" => "auto",
                "AvoidDiskWrites" => "1",
                "DisableNetwork" => "1",
                _ => panic!("unexpected returned key: {}", key),
            };
            assert!(value == expected);
        }

        let vals = tor_controller.getinfo(&["version", "config-file", "config-text"])?;
        let mut expected_torrc_path = data_path.clone();
        expected_torrc_path.push("torrc");
        let mut expected_control_port_path = data_path.clone();
        expected_control_port_path.push("control_port");
        for (key, value) in vals.iter() {
            match key.as_str() {
                "version" => assert!(Regex::new(r"\d+\.\d+\.\d+\.\d+")?.is_match(&value)),
                "config-file" => assert!(Path::new(&value) == expected_torrc_path),
                "config-text" => assert!(
                    value.to_string()
                        == format!(
                            "\nControlPort auto\nControlPortWriteToFile {}\nDataDirectory {}",
                            expected_control_port_path.display(),
                            data_path.display()
                        )
                ),
                _ => panic!("unexpected returned key: {}", key),
            }
        }

        tor_controller.setevents(&["STATUS_CLIENT"])?;
        // begin bootstrap
        tor_controller.setconf(&[("DisableNetwork", "0".to_string())])?;

        // add an onoin service
        let (private_key, service_id) =
            match tor_controller.add_onion(None, &Default::default(), None, 22, None, None)? {
                (Some(private_key), service_id) => (private_key, service_id),
                _ => panic!("add_onion did not return expected values"),
            };
        println!("private_key: {}", private_key.to_key_blob());
        println!("service_id: {}", service_id.to_string());

        assert!(
            tor_controller
                .del_onion(&V3OnionServiceId::from_string(
                    "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd"
                )?)
                .is_err(),
            "deleting unknown onion should have failed"
        );

        // delete our new onion
        tor_controller.del_onion(&service_id)?;

        println!("listeners: ");
        for sock_addr in tor_controller.getinfo_net_listeners_socks()?.iter() {
            println!(" {}", sock_addr);
        }

        // print our event names available to tor
        for (key, value) in tor_controller.getinfo(&["events/names"])?.iter() {
            println!("{} : {}", key, value);
        }

        let stop_time = Instant::now() + std::time::Duration::from_secs(5);
        while stop_time > Instant::now() {
            for async_event in tor_controller.wait_async_events()?.iter() {
                match async_event {
                    AsyncEvent::Unknown { lines } => {
                        println!("Unknown: {}", lines.join("\n"));
                    }
                    AsyncEvent::StatusClient {
                        severity,
                        action,
                        arguments,
                    } => {
                        println!("STATUS_CLIENT severity={}, action={}", severity, action);
                        for (key, value) in arguments.iter() {
                            println!(" {}='{}'", key, value);
                        }
                    }
                    AsyncEvent::HsDesc { action, hs_address } => {
                        println!(
                            "HS_DESC action={}, hsaddress={}",
                            action,
                            hs_address.to_string()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
