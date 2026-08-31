# Container

One host, one container. Substitute `docker` for `podman`; the arguments are the
same.

## 1. Prepare the volume

It must be writable by UID 65532:

```sh
install -d -o 65532 -g 65532 -m 0700 /srv/frater
```

## 2. Run

```sh
podman run -d --name frater \
  --restart unless-stopped \
  -p 3210:3210 \
  -v /srv/frater:/data \
  -e PUBLIC_URL=https://frater.example.com \
  ghcr.io/dvjn/frater:latest
```

## 3. Create the first account

```sh
podman exec -it frater /usr/local/bin/frater bootstrap-superuser --email you@example.com
```

## Operating

```sh
podman logs -f frater                        # logs
curl -fsS http://127.0.0.1:3210/healthz      # health
```

More: [Configuration](../configuration/README.md).
