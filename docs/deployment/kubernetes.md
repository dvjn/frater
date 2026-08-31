# Kubernetes

The database is SQLite on one file, so the Deployment runs **one** replica with
`Recreate` as its strategy. Two replicas would write to one file and corrupt it.

## Manifests

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: frater
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: frater-data
  namespace: frater
spec:
  accessModes: [ReadWriteOnce]
  resources:
    requests:
      storage: 2Gi
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: frater
  namespace: frater
spec:
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels: { app: frater }
  template:
    metadata:
      labels: { app: frater }
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        fsGroup: 65532
      containers:
        - name: frater
          image: ghcr.io/dvjn/frater:latest
          ports:
            - containerPort: 3210
          env:
            - name: PUBLIC_URL
              value: https://frater.example.com
          volumeMounts:
            - name: data
              mountPath: /data
          livenessProbe:
            httpGet: { path: /healthz, port: 3210 }
            initialDelaySeconds: 5
          readinessProbe:
            httpGet: { path: /healthz, port: 3210 }
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: [ALL]
      volumes:
        - name: data
          persistentVolumeClaim:
            claimName: frater-data
---
apiVersion: v1
kind: Service
metadata:
  name: frater
  namespace: frater
spec:
  selector: { app: frater }
  ports:
    - port: 3210
      targetPort: 3210
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: frater
  namespace: frater
spec:
  rules:
    - host: frater.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: frater
                port:
                  number: 3210
  tls:
    - hosts: [frater.example.com]
      secretName: frater-tls
```

## First account

```sh
kubectl -n frater exec -it deploy/frater -- \
  /usr/local/bin/frater bootstrap-superuser --email you@example.com
```

`kubectl exec` can only run `/usr/local/bin/frater`, because the image has no
shell.

## Logs

```sh
kubectl -n frater logs deploy/frater -f
```

