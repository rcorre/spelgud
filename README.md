# Spelgud Language Server

`spelgud` is a spell-checking [Language Server](https://microsoft.github.io/language-server-protocol/).
`spelgud` exposes spelling errors as diagnostics, and provides actions to correct spelling or add words to a dictionary.

# Prerequisites

Install [aspell](http://aspell.net/) or [hunspell](http://hunspell.github.io/) and ensure the executable is on your `$PATH`.

# Installation

```
cargo install --git https://git.sr.ht/~rrc/spelgud
```

Ensure the cargo binary path (usually `~/.cargo/bin`) is on `$PATH`.
Finally, [configure spelgud in your editor](#editor-setup).

# Logging

Set the environment variable `RUST_LOG` to one of ERROR, WARN, INFO, DEBUG, or TRACE.
See [env_logger](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for more details.

# Editor Setup

## Helix

```toml
# ~/.config/helix/languages.toml

[language-server.spelgud]
command = "spelgud"

[[language]]
name = "text"
language-servers = ['spelgud']
```

# Similar Projects

- [ltex](https://valentjn.github.io/ltex/)
