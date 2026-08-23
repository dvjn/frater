# frater

> a self-hosted fitness tracker


## Development

The project uses [mise](https://mise.jdx.dev) for the toolchain and the tasks:

```sh
mise install
mise run dev     # live-reload server
mise run check   # format check, Clippy, tests, dependency audit
```

## Logging

`RUST_LOG` controls the logs. Each request surface writes its access logs under
its own target, so one directive can turn one surface on or off:

| Target | Requests |
| --- | --- |
| `frater::access_log::auth` | browser pages: sign in, registration, reset, account |
| `frater::access_log::oauth` | OAuth metadata, registration, device flow, authorize, token |
| `frater::access_log::mcp` | the MCP transport |
| `frater::access_log::assets` | the stylesheet and the font |
| `frater::access_log::healthz` | `/healthz` |

The targets stay under the crate path, and `EnvFilter` obeys the most specific
directive. Thus a parent target sets the rule and a child target overrides it:

```sh
RUST_LOG=frater=info                                    # every log, request logs included
RUST_LOG=frater=info,frater::access_log::healthz=off    # all except the healthcheck
RUST_LOG=frater::access_log=off                         # no request log, other logs unchanged
RUST_LOG=frater=warn,frater::access_log::oauth=info     # the OAuth requests only
```

Without `RUST_LOG` the default is
`frater=info,frater::access_log::assets=off,frater::access_log::healthz=off`.
A failing healthcheck still logs an error, because the handler writes under the
`frater::web` target, not under an access log target.

## License

MIT. See [LICENSE](LICENSE).

