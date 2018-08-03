use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use native_tls::TlsStream;
use std::net::{Shutdown, TcpStream};

use encoding::{DecoderTrap, EncoderTrap, EncodingRef};
use std::time::Duration;

use message::Message;

use failure::{Error, Fail};

/// This is the comprehensive set of events that can occur.
#[derive(Debug)]
pub enum Event {
    /// Connection was manually closed. The string is the reason.
    Closed(&'static str),
    /// Connection has dropped.
    Disconnected,
    /// Message from the IRC server.
    Message(Message),
    /// Error parsing a message from the server.
    ///
    /// This can probably be ignored, and it shouldn't ever happen, really.
    /// If you catch this you should probably open an issue on GitHub.
    ParseError(Error),
    /// Connection was sucessfully restored.
    Reconnected,
    /// Attempting to restore connection.
    Reconnecting,
    /// An error occured trying to restore the connection.
    ///
    /// This is normal in poor network conditions. It might take
    /// a few attempts before the connection can be restored.
    ReconnectionError(Error),
}

/// This the receiving end of a `mpsc` channel.
///
/// If is closed/dropped, the connection will also be dropped,
/// as there isn't anyone listening to the events anymore.
pub type Reader = Receiver<Event>;

/// Errors produced by the Writer.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Fail)]
pub enum ConnectionError {
    /// Connection is already closed.
    #[fail(display = "Connection is already closed.")]
    AlreadyClosed,
    /// Connection is already disconnected.
    #[fail(display = "Connection is already disconnected.")]
    AlreadyDisconnected,
    /// Connection was manually closed.
    #[fail(display = "Connection was manually closed.")]
    Closed,
    /// Connection was dropped.
    ///
    /// A reconnection might be in process.
    #[fail(display = "Connection was dropped.")]
    Disconnected,
}

enum StreamInner {
    Tcp(TcpStream),
    Tls(TlsStream<TcpStream>),
}

impl StreamInner {
    fn shutdown(&mut self, how: Shutdown) -> Result<(), Error> {
        match self {
            StreamInner::Tcp(s) => s.shutdown(how).map_err(Into::into),
            StreamInner::Tls(s) => s.shutdown().map_err(Into::into),
        }
    }
}

impl Read for StreamInner {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            StreamInner::Tcp(s) => s.read(buf),
            StreamInner::Tls(s) => s.read(buf),
        }
    }
}

impl Write for StreamInner {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            StreamInner::Tcp(s) => s.write(buf),
            StreamInner::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            StreamInner::Tcp(s) => s.flush(),
            StreamInner::Tls(s) => s.flush(),
        }
    }
}

enum StreamStatus {
    // The stream was closed manually.
    Closed,
    // The stream is connected.
    Connected(StreamInner),
    // The stream is disconnected, an attempt to reconnect will be made.
    Disconnected,
}

#[derive(Clone)]
struct Stream {
    inner: Arc<Mutex<StreamStatus>>,
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self.inner.lock().unwrap() {
            StreamStatus::Connected(ref mut s) => match s {
                StreamInner::Tcp(s) => s.read(buf),
                StreamInner::Tls(s) => s.read(buf),
            },
            StreamStatus::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                ConnectionError::Closed.compat(),
            )),
            StreamStatus::Disconnected => Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                ConnectionError::Disconnected.compat(),
            )),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match *self.inner.lock().unwrap() {
            StreamStatus::Connected(ref mut s) => match s {
                StreamInner::Tcp(s) => s.write(buf),
                StreamInner::Tls(s) => s.write(buf),
            },
            StreamStatus::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                ConnectionError::Closed.compat(),
            )),
            StreamStatus::Disconnected => Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                ConnectionError::Disconnected.compat(),
            )),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match *self.inner.lock().unwrap() {
            StreamStatus::Connected(ref mut s) => match s {
                StreamInner::Tcp(s) => s.flush(),
                StreamInner::Tls(s) => s.flush(),
            },
            StreamStatus::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                ConnectionError::Closed.compat(),
            )),
            StreamStatus::Disconnected => Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                ConnectionError::Disconnected.compat(),
            )),
        }
    }
}

impl Stream {
    fn from_tcp(stream: TcpStream) -> Self {
        Stream {
            inner: Arc::new(Mutex::new(StreamStatus::Connected(StreamInner::Tcp(
                stream,
            )))),
        }
    }

    fn from_tls(stream: TlsStream<TcpStream>) -> Self {
        Stream {
            inner: Arc::new(Mutex::new(StreamStatus::Connected(StreamInner::Tls(
                stream,
            )))),
        }
    }

    fn set_disconnected(&self) {
        *self.inner.lock().unwrap() = StreamStatus::Disconnected;
    }

    fn disconnect(&self) -> Result<(), Error> {
        let mut status = self.inner.lock().unwrap();

        match *status {
            StreamStatus::Closed => return Err(ConnectionError::Closed.into()),
            StreamStatus::Connected(ref mut stream) => {
                let _ = stream.shutdown(Shutdown::Both);
            }
            StreamStatus::Disconnected => return Err(ConnectionError::AlreadyDisconnected.into()),
        }

        *status = StreamStatus::Disconnected;
        Ok(())
    }

    fn is_closed(&self) -> bool {
        match *self.inner.lock().unwrap() {
            StreamStatus::Closed => true,
            _ => false,
        }
    }

    fn close(&self) -> Result<(), Error> {
        let mut status = self.inner.lock().unwrap();

        match *status {
            StreamStatus::Closed => return Err(ConnectionError::AlreadyClosed.into()),
            StreamStatus::Connected(ref mut stream) => {
                let _ = stream.shutdown(Shutdown::Both);
            }
            _ => {}
        }

        *status = StreamStatus::Closed;
        Ok(())
    }

    fn write<S: AsRef<str>>(&self, data: S, encoding: EncodingRef) -> Result<(), Error> {
        let mut status = self.inner.lock().unwrap();
        let mut failed = false;

        match *status {
            StreamStatus::Closed => {
                return Err(ConnectionError::Closed.into());
            }
            StreamStatus::Connected(ref mut stream) => {
                // Try to write to the stream.
                let bytes = encoding.encode(data.as_ref(), EncoderTrap::Ignore).unwrap();
                if stream.write(&bytes).is_err() {
                    // The write failed, shutdown the connection.
                    let _ = stream.shutdown(Shutdown::Both);
                    failed = true;
                }
            }
            StreamStatus::Disconnected => {
                return Err(ConnectionError::Disconnected.into());
            }
        }

        if failed {
            // The write failed, change the status.
            *status = StreamStatus::Disconnected;
            Err(ConnectionError::Disconnected.into())
        } else {
            // The write did not fail.
            Ok(())
        }
    }
}

/// Used to send messages to the IRC server.
///
/// This object is thread safe. You can clone it and send the clones to other
/// threads. You can write from multiple threads without any issue. Internally,
/// it uses `Arc` and `Mutex`.
#[derive(Clone)]
pub struct Writer {
    stream: Stream,
    encoding: EncodingRef,
}

impl Writer {
    fn new(stream: Stream, encoding: EncodingRef) -> Writer {
        Writer { stream, encoding }
    }

    fn set_connected(&mut self, stream: Stream) {
        self.stream = stream;
    }

    fn set_disconnected(&self) {
        self.stream.set_disconnected()
    }

    /// Drop the connection and trigger the reconnection process.
    ///
    /// There might be a reconnection attempt, based on your settings.
    /// This should be used if you want the connection to be re-created.
    /// This is not the preferred way of shutting down the connection
    /// for good. Use `close` for this.
    pub fn disconnect(&self) -> Result<(), Error> {
        self.stream.disconnect()
    }

    /// Check if the connection was manually closed.
    pub fn is_closed(&self) -> bool {
        self.stream.is_closed()
    }

    /// Close the connection and stop listening for messages.
    ///
    /// There will not be any reconnection attempt.
    /// An error will be returned if the connection is already closed.
    pub fn close(&self) -> Result<(), Error> {
        self.stream.close()
    }

    /// Send a raw string to the IRC server.
    ///
    /// A new line will be not be added, so make sure that you include it.
    /// An error will be returned if the client is disconnected.
    pub fn raw<S: AsRef<str>>(&self, data: S) -> Result<(), Error> {
        self.stream.write(data, self.encoding)
    }
}

impl Into<Event> for Result<Message, Error> {
    fn into(self) -> Event {
        match self {
            Ok(msg) => Event::Message(msg),
            Err(err) => Event::ParseError(err),
        }
    }
}

/// These settings tell the reconnection process how to behave.
///
/// Default is implemented for this type, with fairly sensible settings.
/// See the Default trait implementation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ReconnectionSettings {
    /// Don't try to reconnect after failure.
    DoNotReconnect,
    /// Reconnect
    Reconnect {
        /// After trying this amount of times, it will stop trying.
        ///
        /// A value of 0 means infinite attempts.
        max_attempts: u32,
        /// Wait time between two attempts to reconnect in milliseconds.
        ///
        /// Note that if the computer's network is still unavailable, the connect
        /// call might block for about a minute until it fails. Somtimes, it fails
        /// instantly because it cannot resolve the hostname. You should probably
        /// leave at least a second of delay, so that it doesn't loop really fast
        /// while getting hostname resolution errors. You can watch the stream of
        /// errors via the ReconnectionError event.
        delay_between_attempts: Duration,
        /// Wait time after disconnection, before trying to reconnect.
        delay_after_disconnect: Duration,
    },
}

/// Default settings are provided for this enum.
///
/// They are:
///
/// `max_attempts` = 10
///
/// `delay_between_attempts` = 5 seconds
///
/// `delay_after_disconnect` = 60 seconds
impl Default for ReconnectionSettings {
    fn default() -> ReconnectionSettings {
        ReconnectionSettings::Reconnect {
            max_attempts: 10,
            delay_between_attempts: Duration::from_secs(5),
            delay_after_disconnect: Duration::from_secs(60),
        }
    }
}

fn reconnect(setting: &ConnectionBuilder, handle: &mut Writer) -> Result<BufReader<Stream>, Error> {
    let tcp_stream = TcpStream::connect((setting.address.as_ref(), setting.port))?;
    let stream = if setting.use_tls {
        let connector =
            ::native_tls::TlsConnector::new().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let stream = connector
            .connect(&setting.address, tcp_stream)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Stream::from_tls(stream)
    } else {
        Stream::from_tcp(tcp_stream)
    };

    let reader = BufReader::new(stream.clone());

    handle.set_connected(stream);
    Ok(reader)
}

struct ReaderThread {
    setting: ConnectionBuilder,
    reader: BufReader<Stream>,
    event_sender: Sender<Event>,
    handle: Writer,
}

impl ReaderThread {
    fn new(setting: ConnectionBuilder, stream: Stream, handle: Writer) -> (Self, Reader) {
        let (event_sender, event_reader) = mpsc::channel::<Event>();

        (
            ReaderThread {
                setting,
                reader: BufReader::new(stream),
                event_sender,
                handle,
            },
            event_reader,
        )
    }

    fn run(&mut self) {
        'read: loop {
            let mut buff = Vec::new();
            let res = self.reader.read_until(b'\n', &mut buff);

            // If there's an error or a zero length read, we should check to reconnect or exit.
            // If the size is 0, it means that the socket was shutdown.
            if res.is_err() || res.unwrap() == 0 {
                // If the stream has the closed status, the stream was manually closed.
                if self.handle.is_closed() {
                    let _ = self.event_sender.send(Event::Closed("manually closed"));
                    break;
                } else {
                    // The stream was not closed manually, see what we should do.

                    // Set the disconnected status on the writer.
                    self.handle.set_disconnected();

                    if self.event_sender.send(Event::Disconnected).is_err() {
                        break;
                    }

                    // Grab the reconnection settings or break the loop if no reconnection is desired.
                    let (max_attempts, delay_between_attempts, delay_after_disconnect) =
                        match self.setting.reco_settings {
                            ReconnectionSettings::DoNotReconnect => {
                                let _ = self.handle.close();
                                let _ = self.event_sender.send(Event::Closed("do not reconnect"));
                                break;
                            }
                            ReconnectionSettings::Reconnect {
                                max_attempts,
                                delay_between_attempts,
                                delay_after_disconnect,
                            } => (max_attempts, delay_between_attempts, delay_after_disconnect),
                        };

                    thread::sleep(delay_after_disconnect);

                    let mut attempts = 0u32;

                    // Loop until reconnection is successful.
                    'reconnect: loop {
                        // If max_attempts is zero, it means an infinite amount of attempts.
                        if max_attempts > 0 {
                            attempts += 1;
                            if attempts > max_attempts {
                                let _ = self.handle.close();
                                let _ = self
                                    .event_sender
                                    .send(Event::Closed("max attempts reached"));
                                break 'read;
                            }
                        }

                        if self.event_sender.send(Event::Reconnecting).is_err() {
                            break 'read;
                        }

                        // Try to reconnect.
                        match reconnect(&self.setting, &mut self.handle) {
                            // Sucess, send event, and update reader.
                            Ok(new_reader) => {
                                self.reader = new_reader;
                                if self.event_sender.send(Event::Reconnected).is_err() {
                                    break 'read;
                                }

                                break 'reconnect;
                            }
                            // Error, send event.
                            Err(err) => {
                                if self
                                    .event_sender
                                    .send(Event::ReconnectionError(err))
                                    .is_err()
                                {
                                    break 'read;
                                }
                            }
                        }
                        // sleep until we try to reconnect again
                        thread::sleep(delay_between_attempts);
                    }
                }
            } else {
                // decode the message
                let line = self
                    .setting
                    .encoding
                    .decode(&buff, DecoderTrap::Ignore)
                    .unwrap();
                // Size is bigger than 0, try to parse the message. Send the result in the channel.
                if self
                    .event_sender
                    .send(Message::parse(&line).into())
                    .is_err()
                {
                    break;
                }
            }
        }

        // If we exited from a break (failed to send message through channel), we might not
        // have closed the stream cleanly. Do so if necessary.
        if !self.handle.is_closed() {
            let _ = self.handle.close();
        }
    }
}

/// A builder to construct connection.
pub struct ConnectionBuilder {
    address: String,
    port: u16,
    reco_settings: ReconnectionSettings,
    encoding: EncodingRef,
    use_tls: bool,
}

impl ConnectionBuilder {
    /// Create a new builder with default settings.
    pub fn new(address: &str) -> Self {
        use encoding::all::UTF_8;
        ConnectionBuilder {
            address: address.to_string(),
            port: 6667,
            reco_settings: ReconnectionSettings::default(),
            encoding: UTF_8,
            use_tls: false,
        }
    }

    /// Set port
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set reconnection settings
    pub fn reconnect(mut self, reco_settings: ReconnectionSettings) -> Self {
        self.reco_settings = reco_settings;
        self
    }

    /// Set encoding
    pub fn encoding(mut self, encoding: EncodingRef) -> Self {
        self.encoding = encoding;
        self
    }

    /// Set use of tls
    pub fn use_tls(mut self, use_tls: bool) -> Self {
        self.use_tls = use_tls;
        self
    }

    /// Create the connection
    pub fn connect(self) -> Result<(Writer, Reader), Error> {
        let tcp_stream = TcpStream::connect((self.address.as_ref(), self.port))?;
        let stream = if self.use_tls {
            let connector = ::native_tls::TlsConnector::new()?;
            let stream = connector.connect(&self.address, tcp_stream)?;

            Stream::from_tls(stream)
        } else {
            Stream::from_tcp(tcp_stream)
        };

        let writer = Writer::new(stream.clone(), self.encoding);

        let (mut reader, event_reader) = ReaderThread::new(self, stream, writer.clone());

        thread::spawn(move || reader.run());

        Ok((writer, event_reader))
    }
}

/// Create a connection to the given address.
///
/// A `Writer`/`Reader` pair is returned. If the connection fails,
/// an error is returned.
///
/// If you don't want to reconnect, use `ReconnectionSettings::DoNotReconnect`.
pub fn connect(
    address: &str,
    port: u16,
    reco_settings: ReconnectionSettings,
    encoding: EncodingRef,
    use_tls: bool,
) -> Result<(Writer, Reader), Error> {
    ConnectionBuilder {
        address: address.to_string(),
        port,
        reco_settings,
        encoding,
        use_tls,
    }.connect()
}
