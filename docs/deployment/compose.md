# Compose

Keeps frater and the proxy in front of it in one file, started together. This
example uses Caddy, because it gets a certificate on its own; any reverse proxy
works.

## `compose.yaml`

```yaml
services:
  frater:
    image: ghcr.io/dvjn/frater:latest
    restart: unless-stopped
    environment:
      PUBLIC_URL: https://frater.example.com
    volumes:
      - frater-data:/data
    expose:
      - "3000"

  caddy:
    image: docker.io/library/caddy:2
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy-data:/data
      - caddy-config:/config

volumes:
  frater-data:
  caddy-config:
  caddy-data:
```

## `Caddyfile`

```
frater.example.com {
	reverse_proxy frater:3000
}
```

## Start

```sh
docker compose up -d
docker compose logs -f frater
```

## First account

```sh
docker compose exec frater /usr/local/bin/frater bootstrap-superuser --email you@example.com
```

More: [Configuration](../configuration/README.md).
