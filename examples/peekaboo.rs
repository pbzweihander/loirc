#![allow(clippy::single_match)]

use {
    encoding::all::UTF_8,
    failure::Fallible,
    ploirc::{connect, Code, Event, Prefix, ReconnectionSettings},
    std::{env, net::TcpStream},
};

/// Say "peekaboo" in a channel and then quit.
/// target/debug/examples/peekaboo "irc.freenode.net:6667" "#mychannel"
fn main() -> Fallible<()> {
    let args: Vec<String> = env::args().collect();
    let server = args.get(1);
    let channel = args.get(2);

    if server.is_none() || channel.is_none() {
        eprintln!("Usage: {} <SERVER> <CHANNEL>", args[0]);
        std::process::exit(1)
    }
    let (server, channel) = (server.unwrap(), channel.unwrap());

    // Connect to server and use no not reconnect.
    let (writer, reader) =
        connect::<TcpStream>(server, ReconnectionSettings::DoNotReconnect, UTF_8).unwrap();
    writer.raw(format!("USER {} 8 * :{}\n", "peekaboo", "peekaboo"))?;
    writer.raw(format!("NICK {}\n", "peekaboo"))?;

    // Receive events.
    for event in reader.iter() {
        println!("{:?}", event);
        match event {
            Event::Message(msg) => {
                if msg.code == Code::RplWelcome {
                    // join channel, no password
                    writer.raw(format!("JOIN {}\n", channel))?;
                }
                // JOIN is sent when you join a channel.
                if msg.code == Code::Join {
                    // If there is a prefix...
                    if let Some(prefix) = msg.prefix {
                        match prefix {
                            // And the prefix is a user...
                            Prefix::User(user) => {
                                // And that user's nick is peekaboo, we've joined the channel!
                                if user.nickname == "peekaboo" {
                                    writer.raw(format!("PRIVMSG {} :{}\n", channel, "peekaboo"))?;
                                    // Note that if the reconnection settings said to reconnect,
                                    // it would. Close would "really" stop it.
                                    writer.raw(format!("QUIT :{}\n", "peekaboo"))?;
                                    // writer.close();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}
