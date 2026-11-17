# Game compatibility and platform adaptation strategy

> **Version 0.1 scope:** Amethys currently targets native game discovery, import, verification and launch on Windows and Linux only. macOS work is paused until physical Mac testing is available. Compatibility runtimes (Proton/Wine), game adaptation and cross-OS porting are future work and are not promised by 0.1.


Amethys is a cross-platform game launcher. Windows, Linux and macOS are first-class host platforms; support must not be designed around Windows first and retrofitted later.

This document defines the long-term compatibility direction. It does not claim that Amethys can make every game compatible or that a launcher can automatically port a game binary.

## Product goals

- Long-term: discover, import, organize, verify and launch games across desktop operating systems; the current 0.1 release is limited to Windows and Linux.
- Clearly distinguish native games from games launched through a compatibility runtime.
- Help users identify which compatibility options are available for a particular game and host.
- Prefer safe, reversible, per-game configuration over global system changes.
- Never bypass DRM, store ownership checks, anti-cheat, platform authentication or other access controls.
- Never modify game files during discovery or verification. Any future game adaptation or patching feature requires explicit opt-in, a clear explanation, backups and a per-game rollback path.

## Three compatibility levels

### 1. Native support

A game has a build intended for the host operating system and architecture. Amethys should detect the native executable and launch it using the platform's normal process APIs.

### 2. Compatibility runtime

A game built for another operating system may run through an existing compatibility environment. Amethys should detect installed runtimes and expose supported launch profiles rather than silently installing or changing system components.

- Linux: Proton (normally through Steam), Wine and compatible community runtimes.
- macOS: CrossOver, Whisky/Wine-based environments and Apple's Game Porting Toolkit where the user has installed and configured them.
- Windows: native execution first; compatibility tools only where a real use case exists.

Runtime availability is not a guarantee that a specific game works. Compatibility can depend on graphics APIs, architecture, runtime version, launch options, store client, DRM and anti-cheat support.

### 3. Native port or game-specific adaptation

A true port changes the game so it can run natively on another operating system. This may require source code, engine support, platform SDKs, graphics/audio/input changes, signing/notarization and publisher authorization.

Amethys must not claim it can automatically port closed-source games. Any future adaptation tooling should be limited to legitimate, documented, user-authorized workflows and should clearly identify what it changes. Community compatibility recipes may describe settings and known workarounds, but must not distribute copyrighted game assets or circumvent protections.

## Platform roadmap

### Windows
- Preserve reliable native game discovery and launch.
- Add a runtime abstraction so future compatibility providers do not leak platform-specific behavior into the core.
- Harden named-pipe access controls and validate peer identity before treating local IPC as trusted.

### Linux
- Keep Steam native and Flatpak layouts covered.
- Expand read-only discovery for Heroic/Legendary and Lutris, then detect Proton/Wine installations and prefixes.
- Support per-game launch profiles without changing global Wine/Proton configuration.
- Report missing runtime dependencies and unsupported anti-cheat cases honestly.

### macOS
- Keep native Steam and Heroic discovery covered.
- Detect Apple Silicon versus Intel and the available execution architecture.
- Detect user-installed CrossOver/Whisky/Wine environments and their game prefixes.
- Build a compatibility assessment for DirectX-to-Metal translation and other prerequisites, without promising universal Windows-game support.
- Investigate native porting workflows only where source code, SDK access and authorization exist.

## Shared architecture requirements

- A platform-neutral game model records host OS, game architecture, native/compatibility status, runtime provider and launch profile separately.
- Runtime providers implement a common interface for detection, capability reporting, profile inspection and launch.
- Detection is read-only. Launch is a separate explicit user action.
- Runtime installation, prefix creation, dependency installation, registry edits and system configuration are potentially destructive or security-sensitive; they must never happen silently.
- Compatibility status should be evidence-based: native, runtime_available, known_working, known_limited, unsupported or unknown. A runtime being installed is not proof that a game works.
- CI must continue testing the shared core on Windows, Linux and macOS. Platform-specific runtime integration tests should use isolated fixtures or controlled runners, not the developer's personal game libraries.

## Initial implementation order

1. Finish shared game launch abstractions and IPC security.
2. Add read-only runtime inventory on Linux and macOS, alongside Windows core work.
3. Add per-game launch profiles and capability reporting.
4. Add safe launch adapters for detected Proton/Wine/CrossOver/Whisky installations.
5. Add compatibility diagnostics and curated, evidence-backed game status.
6. Evaluate native port/adaptation tooling on a game-by-game basis, only with appropriate rights and technical access.

This roadmap is intentionally incremental: Amethys should first become a reliable launcher across all three desktop platforms, then become a compatibility manager, and only later explore legitimate game-specific porting or adaptation.
