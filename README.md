> License status: Source available.
> 
> While you are welcome to read and run the code in this project for your own personal enjoyment, I am not accepting issues or contributions at this time. I will close any issues or contributions opened. I would, however, love to chat about this project with you. Feel free to reach out!

# guide to home.isaac.sh

- Apps are located in `apps/*`.
- Crates are located in `crates/*`.

# 2025-12-10

DNS: 

- There is an `A` record pointing from `home.isaac.sh` to `95.217.200.240`
- There is an `A` record pointing from `*.home.isaac.sh` to `95.217.200.240`

Current system setup:

- Configuration is in `etc/nixos/configuration.nix`
  - We use Caddy to serve everything
    - Caddy configuration files are in `var/lib/caddy/*`
- All site files are in `/var/www/home.isaac.sh/*`
  - These files are currently served statically
- All project files are in `/home/marge/isocore/*`

Planned setup:

- Relative paths are relative to `isocore`
- I will have a deployment script in `scripts/deploy.sh`
  - This script will:
    - Make a zfs snapshot of the sqlite database(s) and put them in a site files backups folder.
    - Compile the Rust project in release mode.
    - Using systemd sockets deploy a new binary.
- (unfinished)

Dependencies:

- You will need: `cargo install cargo-component`
