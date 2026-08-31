# Quick start

Runs a server on your own machine and connects your AI agent to it.

## Requirements

- Podman or Docker.

## 1. Start the server

```sh
mkdir -p ./frater-data
podman run -d --name frater \
  -p 3210:3210 \
  -v ./frater-data:/data \
  ghcr.io/dvjn/frater:latest
```

Check the log:

```sh
podman logs frater
```

A healthy start ends with `server listening`.

## 2. Create the first account

Sign-up is off by default, so create the first account from the command line:

```sh
podman exec -it frater /usr/local/bin/frater bootstrap-superuser --email you@example.com
```

## 3. Connect your agent

The address is `http://localhost:3210/mcp`. Each client opens a browser once to
sign in with the account above and to approve the permissions it asks for.

### Claude Code

```sh
claude mcp add --transport http frater http://localhost:3210/mcp
```

Then run `/mcp` inside Claude Code and sign in.

### Codex

```sh
codex mcp add frater --url http://localhost:3210/mcp
codex mcp login frater
```

### Any other client

```json
{
  "mcpServers": {
    "frater": {
      "type": "http",
      "url": "http://localhost:3210/mcp"
    }
  }
}
```

## 4. Log a workout

Talk to your agent:

> Log today's workout: bench press, three sets of eight at 60 kg.

> What did I lift last Tuesday?

> Correct yesterday's squat session: the top set was 62.5 kg, not 60 kg.

To find out what else you can ask for:

> What can I do with Frater?

## Next steps

- [Deployment](deployment/README.md) — running it for other people to use.
- [Configuration](configuration/README.md) — the environment variables.