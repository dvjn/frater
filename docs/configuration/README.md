# Configuration

Everything is set through environment variables.

## Server

| Variable | Default | Sets |
| --- | --- | --- |
| `PUBLIC_URL` | taken from request headers | the address people use |
| `HTTP_ADDR` | `127.0.0.1:3000` (image: `0.0.0.0:3000`) | where it listens |
| `REGISTRATION_ENABLED` | `false` | whether sign-up is open |

Set `PUBLIC_URL` to the address people type: `https://frater.example.com`.
Every deployment needs it — it fixes the address the OAuth sign-in flow uses,
and an `https` value marks session cookies `Secure`. A value with a path is
rejected, so frater needs a host of its own.

## Mail

| Variable | Default | Sets |
| --- | --- | --- |
| `SMTP_HOST` | unset | the mail server, for password reset |
| `SMTP_PORT` | `587` | the mail server port |
| `SMTP_FROM` | unset | the sender; required with `SMTP_HOST` |
| `SMTP_USERNAME`, `SMTP_PASSWORD` | unset | credentials; both or neither |

Without `SMTP_HOST` a password reset still works, but the reset link is written
to the log instead of an inbox.

## Data

| Variable | Default | Sets |
| --- | --- | --- |
| `DATABASE_URL` | `sqlite://data/frater.db?mode=rwc` (image: in `/data`) | where the database lives |
| `SECRET_KEY` | `@data/root.key` (image: in `/data`) | the root secret |

`SECRET_KEY` takes two forms: `@` and a path to a file to keep the key in
(created on the first start), or the key itself as 32 base64url bytes for a
secret manager:

```sh
head -c 32 /dev/urandom | basenc --base64url | tr -d '='
```

Back the key up together with the database. Restoring a database next to a
different key locks out every account.

## Logs

| Variable | Default | Sets |
| --- | --- | --- |
| `RUST_LOG` | `frater=info,frater::access_log::assets=off,frater::access_log::healthz=off` | what gets logged |

`RUST_LOG` filters by target, and the most specific directive wins:

```
frater                     everything the server logs
└── access_log             the one-line-per-request logs
    ├── auth               browser pages: sign in, registration, reset, account
    ├── oauth              OAuth metadata, registration, device flow, authorize, token
    ├── mcp                the MCP transport
    ├── assets             the stylesheet and the font
    └── healthz            /healthz
```

```sh
RUST_LOG=frater=info                                    # every log, request logs included
RUST_LOG=frater=info,frater::access_log::healthz=off    # all except the healthcheck
RUST_LOG=frater::access_log=off                         # no request log, other logs unchanged
RUST_LOG=frater=warn,frater::access_log::oauth=info     # the OAuth requests only
```

The full syntax: [`tracing_subscriber::EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html).
