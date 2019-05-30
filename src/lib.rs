//! This library's goal is to offer a highly available IRC client with an easy to use API.
//! Automatic reconnection is built into the design. It uses a channel-like design, where
//! events are received from a `Reader` and commands are sent from one or many `Writer`.
//!
//! The `Writer` are thread safe and can be cheaply cloned.
//!
//! Here's a canonical example.
//!
//! ```no_run
//! use {
//!     encoding::all::UTF_8,
//!     ploirc::{connect, Code, Event},
//!     std::net::TcpStream,
//! };
//!
//! fn main() {
//!     // connect to freenode and use the default reconnection settings.
//!     let (writer, reader) =
//!         connect::<TcpStream>("irc.freenode.net:6667", Default::default(), UTF_8).unwrap();
//!     writer.raw(format!("USER {} 8 * :{}\n", "username", "realname"));
//!     writer.raw(format!("NICK {}\n", "nickname"));
//!     // Block until something happens.
//!     for event in reader.iter() {
//!         match event {
//!             // Handle messages
//!             Event::Message(msg) => {
//!                 if msg.code == Code::RplWelcome {
//!                     writer.raw(format!("JOIN {}\n", "#channel"));
//!                 }
//!             }
//!             // Handle other events, such as disconnects.
//!             _ => {}
//!         }
//!     }
//! }
//! ```
#![deny(missing_docs)]

mod activity_monitor;
mod code;
mod connection;
mod message;
mod stream;

pub use {
    activity_monitor::{ActivityMonitor, MonitorSettings},
    code::Code,
    connection::{connect, Error, Event, Reader, ReconnectionSettings, Writer},
    message::{Message, ParseError, Prefix, PrefixUser},
};
