#![feature(phase)]
extern crate regex;
#[phase(plugin)] extern crate regex_macros;
extern crate serialize;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufferedReader, InvalidInput, IoError, IoResult};
use std::vec::Vec;
use conn::{Connection, connect, send};
use data::{Config, Message};

pub mod conn;
pub mod data;

pub struct Bot<'a> {
    pub conn: Connection,
    pub config: Config,
    process: RefCell<|&Bot, &str, &str, &[&str]|:'a -> IoResult<()>>,
    pub chanlists: HashMap<String, Vec<String>>,
}

impl<'a> Bot<'a> {
    pub fn new(process: |&Bot, &str, &str, &[&str]|:'a -> IoResult<()>) -> IoResult<Bot<'a>> {
        let config = try!(Config::load());
        let conn = try!(connect(config.server.as_slice(), config.port));
        Ok(Bot {
            conn: conn,
            config: config,
            process: RefCell::new(process),
            chanlists: HashMap::new(),
        })
    }

    pub fn send_nick(&self, nick: &str) -> IoResult<()> {
        send(&self.conn, Message::new(None, "NICK", [nick]))
    }

    pub fn send_user(&self, username: &str, real_name: &str) -> IoResult<()> {
        send(&self.conn, Message::new(None, "USER", [username, "0", "*", real_name]))
    }

    pub fn send_join(&self, chan: &str) -> IoResult<()> {
        send(&self.conn, Message::new(None, "JOIN", [chan.as_slice()]))
    }

    pub fn send_topic(&self, chan: &str, topic: &str) -> IoResult<()> {
        send(&self.conn, Message::new(None, "TOPIC", [chan.as_slice(), topic.as_slice()]))
    }

    pub fn send_invite(&self, person: &str, chan: &str) -> IoResult<()> {
        send(&self.conn, Message::new(None, "INVITE", [person.as_slice(), chan.as_slice()]))
    }

    pub fn send_privmsg(&self, chan: &str, msg: &str) -> IoResult<()> {
        send(&self.conn, Message::new(None, "PRIVMSG", [chan.as_slice(), msg.as_slice()]))
    }

    pub fn identify(&self) -> IoResult<()> {
        self.send_nick(self.config.nickname.as_slice());
        self.send_user(self.config.username.as_slice(), self.config.realname.as_slice())
    }

    pub fn output(&mut self) {
        let mut reader = { let Connection(ref tcp) = self.conn; BufferedReader::new(tcp.clone()) };
        for line in reader.lines() {
            match line {
                Ok(ln) => {
                    let (source, command, args) = process(ln.as_slice()).unwrap();
                    self.handle_command(source, command, args.as_slice());
                    println!("{}", ln)
                },
                Err(e) => println!("Shit, you're fucked! {}", e),
            }
        }
    }

    fn handle_command(&mut self, source: &str, command: &str, args: &[&str]) -> IoResult<()> {
        match (command, args) {
            ("PING", [msg]) => {
                try!(send(&self.conn, Message::new(None, "PONG", [msg])));
            },
            ("376", _) => { // End of MOTD
                for chan in self.config.channels.iter() {
                    try!(self.send_join(chan.as_slice()));
                }
            },
            ("422", _) => { // Missing MOTD
                for chan in self.config.channels.iter() {
                    try!(self.send_join(chan.as_slice()));
                }
            },
            ("353", [_, _, chan, users]) => { // /NAMES
                for user in users.split_str(" ") {
                    if !match self.chanlists.find_mut(&String::from_str(chan)) {
                        Some(vec) => {
                            vec.push(String::from_str(user));
                            true
                        },
                        None => false,
                    } {
                        self.chanlists.insert(String::from_str(chan), vec!(String::from_str(user)));
                    }
                }
            },
            ("JOIN", [chan]) => {
                match self.chanlists.find_mut(&String::from_str(chan)) {
                    Some(vec) => {
                        match source.find('!') {
                            Some(i) => vec.push(String::from_str(source.slice_to(i))),
                            None => (),
                        };
                    },
                    None => (),
                }
            },
            ("PART", [chan, _]) => {
                match self.chanlists.find_mut(&String::from_str(chan)) {
                    Some(vec) => {
                        match source.find('!') {
                            Some(i) => {
                                match vec.as_slice().position_elem(&String::from_str(source.slice_to(i))) {
                                    Some(n) => {
                                        vec.swap_remove(n);
                                    },
                                    None => (),
                                };
                            },
                            None => (),
                        };
                    },
                    None => (),
                }
            },
            _ => {
                (*self.process.borrow_mut().deref_mut())(self, source, command, args);
            },
        };
        Ok(())
    }
}

fn process(msg: &str) -> IoResult<(&str, &str, Vec<&str>)> {
    let reg = regex!(r"^(?::([^ ]+) )?([^ ]+)(.*)");
    let cap = match reg.captures(msg) {
        Some(x) => x,
        None => return Err(IoError {
            kind: InvalidInput,
            desc: "Failed to parse line",
            detail: None,
        }),
    };
    let source = cap.at(1);
    let command = cap.at(2);
    let args = parse_args(cap.at(3));
    Ok((source, command, args))
}

fn parse_args(line: &str) -> Vec<&str> {
    let reg = regex!(r" ([^: ]+)| :([^\r\n]*)[\r\n]*$");
    reg.captures_iter(line).map(|cap| {
        match cap.at(1) {
            "" => cap.at(2),
            x => x,
        }
    }).collect()
}

#[test]
fn process_line_test() {
    let res = process(":flare.to.ca.fyrechat.net 353 pickles = #pickles :pickles awe\r\n").unwrap();
    let (source, command, args) = res;
    assert_eq!(source, "flare.to.ca.fyrechat.net");
    assert_eq!(command, "353");
    assert_eq!(args, vec!["pickles", "=", "#pickles", "pickles awe"]);

    let res = process("PING :flare.to.ca.fyrechat.net\r\n").unwrap();
    let (source, command, args) = res;
    assert_eq!(source, "");
    assert_eq!(command, "PING");
    assert_eq!(args, vec!["flare.to.ca.fyrechat.net"]);
}

#[test]
fn process_args_test() {
    let res = parse_args("PRIVMSG #vana :hi");
    assert_eq!(res, vec!["#vana", "hi"])
}