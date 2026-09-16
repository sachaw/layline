//! Declarations more than one test file reads.
#![allow(dead_code)]

use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 8, endian = le)]
pub struct Sealed {
    #[magic(b"SEAL")]
    pub signature: [u8; 4],
    pub file_size: u32,
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 80, view)]
pub struct NavState {
    pub week: u32,
    pub time_of_week: f64,
    pub nav_status: u32,
    pub hw_status: u32,
    #[at(byte = 20)]
    pub theta: [f32; 3],
    pub uvw: [f32; 3],
    #[at(byte = 44)]
    pub lla: [f64; 3],
    pub ned: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2, endian = be)]
pub struct Ping {
    pub seq: u16,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
pub struct Pong {
    pub seq: u32,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed, endian = be)]
pub enum Body {
    #[value(0)]
    #[bytes(2)]
    Ping(Ping),
    #[value(1)]
    #[bytes(4)]
    Pong(Pong),
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub struct Switched {
    pub kind: u8,
    #[switch(kind)]
    pub body: Body,
    pub crc: u16,
}
