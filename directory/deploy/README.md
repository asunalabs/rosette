# Directory service: AWS EC2 deploy (T18)

One EC2 instance running `directory` + Postgres + Caddy (automatic TLS) via
docker compose. No ALB/RDS/ACM to provision first — those are real
upgrades, not requirements to get a working HTTPS endpoint.

## 1. Launch the instance

- AMI: Amazon Linux 2023 or Ubuntu 22.04/24.04 (either works; steps below
  assume `dnf`/Amazon Linux, swap for `apt` on Ubuntu).
- Instance type: `t3.small` is enough to start (directory + a small
  Postgres + Caddy). Move to `t3.medium` if Postgres becomes the
  bottleneck — nothing here assumes a fixed size.
- Storage: 20GB gp3 is plenty until Postgres data grows meaningfully.
- Security group: inbound 22 (SSH, restrict to your IP), 80 and 443
  (0.0.0.0/0 — Caddy needs 80 for the ACME HTTP-01 challenge). No other
  inbound ports: Postgres is never exposed outside the compose network.
- Elastic IP: allocate one and associate it, then point `DIRECTORY_DOMAIN`'s
  DNS A record at it *before* starting Caddy (ACME will fail and retry with
  backoff otherwise, so it's recoverable, just slower to first boot).

## 2. Install Docker

```bash
sudo dnf install -y docker
sudo systemctl enable --now docker
sudo usermod -aG docker $(whoami)   # log out/in to pick this up
# docker compose (the plugin, not docker-compose) ships with recent
# Docker Engine packages; if `docker compose version` fails, install the
# compose-plugin package for your distro.
```

## 3. Check out the repo and configure

```bash
sudo mkdir -p /opt/chat && sudo chown $(whoami) /opt/chat
git clone <this repo> /opt/chat
cd /opt/chat/directory/deploy
cp .env.example .env
# edit .env: DIRECTORY_DOMAIN, POSTGRES_PASSWORD, DIRECTORY_PEPPER
# (from a secrets manager, never reused from dev), and either the three
# TWILIO_* values or DIRECTORY_ALLOW_DEV_OTP_VENDOR=1 for a non-production
# trial run.
```

## 4. Start it, and keep it up across reboots

```bash
sudo cp directory-stack.service /etc/systemd/system/
sudo systemctl enable --now directory-stack
```

## 5. Verify

```bash
curl https://$DIRECTORY_DOMAIN/health   # expect: ok
```

## What's deliberately not here

- **Managed Postgres (RDS)** — self-hosted-on-the-same-box for v1; revisit
  once uptime/backup requirements exceed a single EBS volume with a `pg_dump`
  cron.
- **ALB / autoscaling** — single instance for v1; the compose stack doesn't
  assume it's the only way to run `directory`, so moving to ECS/EKS later
  doesn't require redesigning the app, just the deploy topology.
- **Automated backups** — add a `pg_dump`-to-S3 cron once there's account
  data worth losing sleep over; not wired up yet.
