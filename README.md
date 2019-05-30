# ploirc

[![CircleCI badge]][CircleCI]
[![crates.io badge]][crates.io]
[![docs.rs badge]][docs.rs]

**ploirc** is a **p**bzweihander's **lo**w level **irc** client library.
This is a fork of [loirc].

Automatic reconnections are built into the core, and [utilities] are available
to increase reliability. It's the perfect library to use on fragile network
connections such as Wi-Fi, but it's also very useful for any type of clients,
such as bots and loggers, that require high availability.

[Events] are read from a channel, and communications are sent via [Writers].
Event processing can be a bit tedious, hence why this is considered low level.

The [documentation] is pretty good, please refer to it for more information.
Examples are also available in the `examples` folder.

------

_ploirc_ is distributed under the terms of both [MIT License] and
[Apache Licence 2.0]. See [COPYRIGHT] for detail.

[CircleCI]: https://circleci.com/gh/pbzweihander/ploirc
[CircleCI badge]: https://circleci.com/gh/pbzweihander/ploirc.svg?style=shield
[crates.io]: https://crates.io/crates/ploirc
[crates.io badge]: https://badgen.net/crates/v/ploirc
[docs.rs]: https://docs.rs/ploirc
[docs.rs badge]: https://docs.rs/ploirc/badge.svg
[loirc]: https://github.com/sbstp/loirc
[utilities]: https://docs.rs/ploirc/*/ploirc/struct.ActivityMonitor.html
[Events]: https://docs.rs/ploirc/*/ploirc/enum.Event.html
[Writers]: https://docs.rs/ploirc/*/ploirc/struct.Writer.html
[documentation]: https://docs.rs/ploirc/
[MIT License]: LICENSE-MIT
[Apache License 2.0]: LICENSE-APACHE
[COPYRIGHT]: COPYRIGHT
