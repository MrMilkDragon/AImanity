# Roadmap

See [release status](release-status.md) for downloads and platform requirements.

## Fluorine 0.0.6

Fluorine starts with an empty workspace backed by an embedded Rust database. Add virtual Android phones and Linux desktops as needed, or pair a device. The shared Agent selects from the devices you authorize. Disconnecting stops an owned guest and keeps its disk. Deleting it also removes its local instance data.

| Host | Planned installer |
| --- | --- |
| Apple Silicon Mac | DMG |
| Windows 11 x64 | EXE |
| Linux x86_64 | DEB for Debian and Ubuntu, RPM for Fedora and Enterprise Linux |
| Android phone and tablet | One signed landscape APK for both screen sizes |

Install Fluorine as a fresh client. The Android package includes its guest templates. Creating local guests on Android requires an ARM64 host with root access and Android Virtualization Framework (AVF) custom VM support. The empty workspace opens before any guest or microphone permission is requested.

## 0.0.7 · iPhone and iPad

Plan one universal iOS and iPadOS app with a shared landscape workbench. Qualify local execution and device permissions on both form factors before TestFlight distribution.

The installation contract is described in [distribution requirements](distribution-requirements.md), [runtime bundles](runtime-bundles.md) and [self-contained hosts](integrated-hosts.md).

## Responsiveness and sustained work

Reduce cold-start time, control latency and input-to-response delay. Measure cold and warm operation separately across device capture, transport, model scheduling and playback. New user input should adjust the current task without waiting for a long-running foreground operation or recreating its conversation.

Device actions retain their result identity across cancellation and recovery. Unknown outcomes are observed before retrying. Long-lived background tasks keep their checkpoints while notifications and user instructions are admitted through the same durable conversation path.

## Models and connections

Save model/API settings, application connections and memory per profile. All devices in the workspace share the selected profile. Skills, knowledge and work files are shared.

Extend model discovery with manual-ID fallback, configuration-bound capability checks, provider-specific protocol tests, OAuth and local-model integration. Supported protocols and providers are listed in [model setup](model-and-channel-setup.md) and [compatibility](compatibility.md).

## Copilot voice

Copilot uses an offline speech recognizer and the system voice. Spoken corrections enter the same conversation as typed instructions. Guest microphone inputs and speaker outputs are routed per device. Qualification covers first-use permission, simultaneous audio, interruption and the time from a pause to the first device action.

## Calls and extensions

Calls need a session, line provider and dial, answer, mute and hang-up controls. Extensions can integrate SIP, user-configured telephone services or physical devices. Show the recipient, cost and recording scope before placing a call.

Optional [companion applications](companion-extension.md), additional devices and system templates can extend the core workspace independently. Existing user state must remain compatible across extensions and program updates.

## Updates and distribution

Use signed versioned artifacts, compatibility/capacity checks, consistent backup, explicit migration, health checks and activation/rollback. See [application and user-data boundaries](distribution.en.md) and [backup/recovery](backup-restore.en.md).

Original code remains Apache-2.0. Third-party software, images and assets retain their own licences and source obligations.
