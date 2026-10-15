# LogiBat Tray

Small Windows tray app that shows the battery status of:

- **Logitech G522 LIGHTSPEED** headset (LIGHTSPEED dongle, PID `0x0B18`)
- **Logitech G PRO X SUPERLIGHT 2** mouse (LIGHTSPEED receiver, PID `0xC54D` / `0xC547`)

## Tray icon

Two battery bars: **left = headset**, **right = mouse**.

| Look | Meaning |
|------|---------|
| green fill | 50 % or more |
| amber fill | 20–49 % |
| red fill | less than 20 % |
| blue fill | charging |
| gray, empty | not connected, off, or no answer |

The fill height shows the level. Move the mouse over the icon to see the exact
values and the time of the last read-out. The outline color follows the
Windows light / dark taskbar theme.

Tray menu: **Refresh now**, **Start with Windows**, **Open log folder**, **Quit**.

## When the app reads the batteries

- At app start.
- **Device answers:** every 15 minutes.
- **Dongle present, device does not answer** (headset off, mouse asleep):
  every 60 seconds. This query only goes to the dongle, so it costs no device
  battery. Each dongle has its own schedule.
- **Dongle not present** (for example KVM switched away): no timed read.
- **Device arrival** (USB/HID arrival event, for example KVM switched back):
  read 3 s after the last event, then 2 fast retries 5 s apart if the device
  does not answer yet.
- **Device removal:** the device is shown as "not connected" at once.

## Install

Download `LogiBatTray-Setup-<version>.exe` from the
[releases](https://github.com/allgoewer/logibat-tray/releases) and run it.
It installs per user (no admin rights). The task **Start with Windows** writes
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run\LogiBatTray`. The tray menu
toggle uses the same value.

Note: close Logitech G HUB if the values do not update — it can hold the
devices.

## Logs

`%LOCALAPPDATA%\logibat-tray\logibat-tray.log` (cleared at each start).
Start with `--debug` to log all HID packets as hex dumps.

## Development

```sh
cargo test                                        # protocol, schedule, icon tests (any OS)
cargo clippy --target x86_64-pc-windows-gnu -- -D warnings   # check the Windows code on Linux
cargo build --release                             # on Windows
ICON_PREVIEW_DIR=out cargo test dump_preview -- --ignored    # dump all icon variants as raw RGBA
```

The protocol code is a port of the Python prototype: see `src/devices/`.

## Commits and releases

This repository uses [Conventional Commits](https://www.conventionalcommits.org/)
(`feat: …`, `fix: …`, `docs: …`, `ci: …`, `chore: …`). The release notes are
made from the commit messages by [git-cliff](https://git-cliff.org/) (`cliff.toml`).

- Each push and pull request runs the **CI** workflow (format, clippy, tests).
- A release is built **only** when a tag `v*` is pushed:

  ```sh
  # 1. set the same version in Cargo.toml, commit: "chore(release): v0.2.0"
  git tag v0.2.0
  git push origin v0.2.0
  ```

  The **Release** workflow checks that the tag matches `Cargo.toml`, builds
  the exe and the Inno Setup installer, and creates a GitHub release with the
  git-cliff notes and the installer.

## License

GPL-3.0-only. See [LICENSE](LICENSE).
