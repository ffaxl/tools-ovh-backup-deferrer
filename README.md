# ovh-autobackup-deferrer

Keeps the OVHcloud VPS *automated backup* schedule permanently in the recent past, so the daily
backup — which stops the guest for up to a minute — never reaches its trigger time.

At startup and then ten seconds after every full UTC hour, it sets every configured VPS's backup
schedule to the start of the previous hour minus that VPS's own offset, whatever the schedule was.
The schedule therefore sits one to two hours plus the offset in the past, and the provider, which
does not run a schedule that has already elapsed today, keeps postponing it to tomorrow.

Two provider behaviours this rests on are observed, not documented by OVHcloud: the API's
`schedule` is UTC (the control panel labels it so), and a schedule set to a time already past today
does not run until the next day (confirmed on real VPS). If either changes, the daemon triggers
backups instead of deferring them.

Deferring the backup also means the provider keeps no restore point for these VPS. Have another
backup of whatever they hold in place before deploying.

## Behaviour in short

- Per VPS and cycle: one `POST /vps/{service}/automatedBackup/reschedule`, nothing read first.
- A failing VPS is logged and the cycle moves on; the next hour retries. The schedule sits at least
  21 hours from firing, so the daemon can miss about twenty cycles before a backup runs.
- Offsets must differ between VPS: a backup pauses its guest, and two etcd members paused together
  lose quorum. Startup refuses a shared offset; keep them well apart, e.g. 0, 20 and 40.
- Output is JSON logs on stdout, one line per VPS per cycle. There is no listener and no probe.

## Configuration

Environment variables only.

| Variable | Default | Meaning |
| -------- | ------- | ------- |
| `OVH_ENDPOINT` | `ovh-ca` | API region: `ovh-eu`, `ovh-ca` or `ovh-us` |
| `OVH_APPLICATION_KEY` | — | required |
| `OVH_APPLICATION_SECRET` | — | required |
| `OVH_CONSUMER_KEY` | — | required |
| `DEFERRER_SERVICES` | — | required; `service:offset` pairs, offset in minutes 0–59, e.g. `vps-aaaa.vps.ovh.net:0,vps-bbbb.vps.ovh.net:20` |
| `DEFERRER_DRY_RUN` | `false` | debugging: `true` logs the targets without writing them |
| `RUST_LOG` | `info` | log filter |

## Credentials

1. Create an application for your region, e.g. `https://ca.api.ovh.com/createApp/`. It yields the
   application key and secret.
2. Request a consumer key limited to the one call the daemon makes:

   ```sh
   curl -s -X POST https://ca.api.ovh.com/1.0/auth/credential \
     -H "X-Ovh-Application: $OVH_APPLICATION_KEY" \
     -H 'Content-Type: application/json' \
     -d '{"accessRules":[{"method":"POST","path":"/vps/*/automatedBackup/reschedule"}]}'
   ```

   Open the returned `validationUrl`, log in and confirm; the returned `consumerKey` is then valid.
3. VPS service names, if you do not have them at hand, are listed in the control panel or by
   `GET /vps`; the daemon's key cannot read them.

## Running locally

```sh
export OVH_APPLICATION_KEY=... OVH_APPLICATION_SECRET=... OVH_CONSUMER_KEY=...
export DEFERRER_SERVICES=vps-aaaa.vps.ovh.net:0,vps-bbbb.vps.ovh.net:20
DEFERRER_DRY_RUN=true cargo run
```

Without `DEFERRER_DRY_RUN=true` this writes the schedules.

## Deploying

CI publishes the image to `ghcr.io/ffaxl/tools-ovh-backup-deferrer`: `sha-<commit>` and `latest`
for every push to `main`, and `<version>` for every `v<version>` tag. The Helm chart in
[helm/](helm/) runs it as a single-replica Deployment; [helm/values.yaml](helm/values.yaml)
lists every setting.

1. Note the current backup time of every VPS from the control panel, somewhere private; rollback
   writes these back.
2. Write a values file, kept out of this repository, listing a single VPS at first:

   ```yaml
   image:
     tag: latest              # or digest: sha256:...; defaults to the chart's appVersion
   services:
     - name: vps-aaaa.vps.ovh.net
       offset: 0
   credentials:
     existingSecret: ovh-api  # holds OVH_APPLICATION_KEY, OVH_APPLICATION_SECRET, OVH_CONSUMER_KEY
   ```

   Without `existingSecret`, set `credentials.applicationKey`, `applicationSecret` and
   `consumerKey` instead and the chart creates the secret.
3. Install: `helm upgrade --install ovh-autobackup-deferrer ./helm -n <namespace> -f <values>`.
   The pod needs outbound HTTPS to the API and DNS, nothing else.
4. Check the logs: one `written` line for the VPS, and the new time in the control panel a few
   minutes later.
5. Watch it for 72 hours: no new restore point in the control panel and no guest pause at its old
   backup time. Then add the rest to `services` and upgrade again.

Rollback: `helm uninstall ovh-autobackup-deferrer -n <namespace>`, then set the noted backup times
back in the control panel, keeping them at least 20 minutes apart.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```
