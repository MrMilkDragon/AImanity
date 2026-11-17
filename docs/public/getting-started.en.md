# Local setup and configuration

[简体中文](getting-started.zh-CN.md) · Contributor source setup

## Choose an installation path

Check [release status](release-status.md) for installer availability and platform qualification before downloading. The intended product setup is to install the client, choose language and device resources, and configure a model; see the [integrated-host contract](integrated-hosts.md).

The instructions below build the project from source on Apple Silicon. For the Windows package, see the [Windows factory](windows-client-factory.md).

## Update the installed client

When an update is available, the workbench shows an update icon at the top right. Open it to read the version, release name and notes, then select **Install Update**. AI Phone verifies the package, installs it and reopens using your existing devices, model settings and conversations. Windows may show a system approval prompt.

## Source alternative: prerequisites

Use an Apple Silicon Mac with Rust 1.96.0, Xcode Command Line Tools, and the Android 35 platform and native build tools. Prepare the offline Android and Debian/Openbox bundles using the [macOS factory](engineering/macos-full-runtime.md). Reserve 35 GiB for a fresh source build and 16 GiB of RAM for building and running both guests.

## First run

1. Run `./ai-phone doctor`.
2. Run `./ai-phone install --resources /path/to/host` and choose a language and device resources. Use the factory's generated `host` directory.
3. Open the workbench and select a saved profile in Configuration → Agent. The phone and computer use the same selected profile.

## Resources and networking

`configure` saves a private 0600 host profile in `data/deployment/profile.json`. Creation presets do not resize existing devices. Desktop CPU/memory is separately configured in its creation form; its host-backed directory has no enforced independent disk quota.

New deployments connect directly. Import and enable a subscription in the [workbench proxy](network.md) to route supported device traffic.

Use `start`, `open` and `stop` for normal operation. Repeated start preserves existing services; partial-service recovery still needs diagnostics. LAN address discovery uses private interfaces; the address is saved in private `data/deployment.json`. LAN HTTP is unencrypted: use trusted TLS or a user-managed VPN on untrusted networks. No public tunnel is created.

## Review bundles

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- source package dist
```

The command writes a source archive with an embedded file manifest under `dist`. See [distribution and updates](distribution.en.md).

## Personal entry and guest apps

The default local single-owner profile opens the workbench directly and preserves existing credentials/sessions. Clients able to reach this trusted-LAN deployment can enter as its owner. This is not multi-user authentication. Set `local_entry` to false in the private deployment profile to disable this entry endpoint; existing authentication APIs remain available. Configure the first Agent model in Configuration.

The Android home screen starts with the browser, Settings and Files. Apps you install are added to the home screen.

Installation stores private durable per-phase checkpoints and uses a process lock. On retry, install verifies source inputs and artifacts before reusing completed work; start resumes missing owned services and preserves running tasks. See [task recovery and retry behavior](task-recovery.md).

## Liberty (development runtime)

Enable Liberty in the conversation of a configured device. The Agent can choose and revise its own goals. Reply and installation settings still apply. A paused owner task keeps its own history. The Agent can save reusable methods after checking the evidence. See [Liberty](autonomous-exploration.md) and [notification-driven work](notification-workflows.md).
