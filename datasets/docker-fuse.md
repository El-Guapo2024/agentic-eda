# Running the FUSE mount in the `agentic-eda` Docker image

`mount.sh` needs a FUSE kernel driver. On this Mac that means macFUSE, which
is a kernel extension requiring manual approval in System Settings and a
reboot — not something this repo can or should install for you. Checked on
2026-09-10: `/Library/Filesystems/macfuse.fs` is **not present**, so the
mount was NOT tested on macOS (mount.sh detects this and refuses rather than
silently falling back).

The supported path is Linux, inside the `agentic-eda` Docker image
(`docker/Dockerfile`), where FUSE is a standard kernel feature exposed to
containers via two flags.

## docker run

```bash
docker run --rm -it \
  --cap-add SYS_ADMIN \
  --device /dev/fuse \
  --security-opt apparmor:unconfined \
  -v "$(pwd)/datasets:/workspace/datasets" \
  agentic-eda \
  bash -c "pip install -q huggingface_hub fsspec fusepy && \
           mkdir -p /mnt/hf && \
           python3 /workspace/datasets/mount.sh bshada/open-schematics /mnt/hf"
```

- `--cap-add SYS_ADMIN` — lets the container create a FUSE mount.
- `--device /dev/fuse` — exposes the host's `/dev/fuse` char device.
- `--security-opt apparmor:unconfined` — needed on AppArmor-enforcing hosts
  (not needed on macOS/Docker Desktop's Linux VM, harmless to include).

## docker-compose snippet

```yaml
services:
  eda-fuse:
    image: agentic-eda
    cap_add:
      - SYS_ADMIN
    devices:
      - /dev/fuse
    security_opt:
      - apparmor:unconfined
    volumes:
      - ./datasets:/workspace/datasets
    command: >
      bash -c "pip install -q huggingface_hub fsspec fusepy &&
               mkdir -p /mnt/hf &&
               python3 /workspace/datasets/mount.sh bshada/open-schematics /mnt/hf &&
               sleep infinity"
```

## Verifying the mount

From another shell into the same container:

```bash
docker exec -it <container> ls -la /mnt/hf
docker exec -it <container> cat /mnt/hf/README.md
```

Files should list instantly; `cat`-ing a large file pulls bytes lazily from
the Hub on first read and caches them under `datasets/cache/` on the host
(bind-mounted), so a second read is local.

## Unmounting

```bash
docker exec <container> umount /mnt/hf
```

## If you want to test on macOS anyway

Install macFUSE yourself (https://macfuse.github.io/), approve the kernel
extension in System Settings > Privacy & Security, reboot, then re-run
`./mount.sh <repo> <mountpoint>` directly on the Mac — it will detect
`/Library/Filesystems/macfuse.fs` and proceed instead of refusing.
