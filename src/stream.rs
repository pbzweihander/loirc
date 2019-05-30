#[cfg(feature = "std")]
use std::{io, net::TcpStream};
use std::{
    io::{Read, Write},
    net::Shutdown,
};

pub trait Stream: Read + Write + Sized + Send {
    type Error: std::error::Error + Send + 'static;

    fn connect<A: AsRef<str>>(addr: A) -> Result<Self, Self::Error>;
    fn shutdown(&self, how: Shutdown) -> Result<(), Self::Error>;
    fn try_clone(&self) -> Result<Self, Self::Error>;
}

#[cfg(feature = "std")]
impl Stream for TcpStream {
    type Error = io::Error;

    fn connect<A: AsRef<str>>(addr: A) -> Result<Self, Self::Error> {
        TcpStream::connect(addr.as_ref())
    }

    fn shutdown(&self, how: Shutdown) -> Result<(), Self::Error> {
        TcpStream::shutdown(self, how)
    }

    fn try_clone(&self) -> Result<Self, Self::Error> {
        TcpStream::try_clone(self)
    }
}
