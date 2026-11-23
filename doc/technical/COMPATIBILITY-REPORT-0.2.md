# Amethys Launcher 0.2 — compatibility and validation report

**Snapshot date:** 23 November 2026  
**Status:** experimental development preview; compatibility is intentionally not presented as complete.

This report describes what the 0.2 code can establish today, what remains uncertain, and where official publisher/platform cooperation would materially improve compatibility. It is an engineering status report, not a certification or a claim that every detected game can launch.

## Evidence levels — do not conflate them

Amethys must report these as separate facts:

1. **Detected** — trusted local launcher metadata identifies an installation.
2. **Imported** — the user has explicitly added that installation to the Amethys library.
3. **Directory verified** — the saved installation path exists and is accessible.
4. **Executable identified** — Amethys has selected a candidate executable using platform metadata, file metadata and bounded path analysis.
5. **Process started** — the operating system accepted a process-creation request for the configured target.
6. **Game verified in-session** — a reproducible manual test confirms that the game reached a usable state, such as its main menu.

A successful scan proves only detection. The current `games verify` command checks directory and configured executable paths; its `healthy` and `launch_ready` values do **not** prove file integrity, anti-cheat readiness, successful process startup or successful gameplay. A path can exist while still pointing to a bootstrapper, helper or platform stub.

## What 0.2 currently provides

- Read-only discovery adapters for supported launcher metadata.
- Explicit import into a local Amethys library.
- Local installation-path and configured-executable verification.
- A bounded, deterministic executable-candidate scan.
- Static Windows PE version-resource inspection to improve candidate ranking without executing candidate files.
- Direct launch of a configured executable through normal operating-system process APIs.
- Path validation that rejects unsafe relative paths and executable targets resolving outside the saved installation directory.
- Automated CI checks and fixture-based tests for metadata parsing and filesystem-safety cases.

Discovery does not start launchers, rewrite launcher databases, modify game files, install drivers, or change system security settings.

## Real-install findings and unresolved cases

Real Windows installations have demonstrated several patterns that a generic filename-only resolver cannot safely solve:

- A game directory can contain a small storefront stub, one or more large engine binaries, crash handlers and prerequisite installers. The largest `.exe` is not automatically the correct launch target.
- A root-level executable can be a publisher's bootstrapper rather than the game binary. Its product name and original filename may describe the launcher, while the actual game binary is nested several directories below.
- Some installations ship multiple binaries with the same product/version metadata but different hashes or build variants. Selecting one arbitrarily is unsafe.
- Anti-cheat launchers may sit beside the game binary, while the actual anti-cheat also depends on a Windows service or driver. Merely starting the game executable may not satisfy those requirements.
- A platform's official launcher may perform authentication, entitlement checks, updates, launch arguments or anti-cheat preparation that a direct process start does not reproduce.

When evidence is ambiguous, Amethys should leave the executable unconfigured and explain why rather than silently selecting a random helper or secondary binary. A publisher-matched bootstrapper may be used as a hand-off only when the evidence is unambiguous; it still requires a real launch test.

## Anti-cheat and security boundary

Version 0.2 does not claim to detect, install, update, start, stop, configure or repair anti-cheat services and drivers. It does not inject into game processes, bypass anti-cheat checks, disable security controls, circumvent DRM, or alter platform authentication/licensing.

Some titles may require an official launcher, a signed service or a kernel driver. Those requirements must be respected. Amethys must not try to imitate or bypass them.

The intended integration path is to use publisher- or platform-documented launch mechanisms and permissioned metadata. Any future anti-cheat integration must be reviewed with the relevant publisher/security team and must not weaken the anti-cheat's guarantees.

## Current coverage limitations

- Adapter coverage varies by launcher and operating system; fixture-tested is not the same as validated against a real installation.
- Battle.net and Xbox GDK real-install validation remain incomplete.
- Linux has automated CI coverage but still needs validation on a physical Linux machine.
- macOS is not supported by the 0.2 release.
- Executable selection is generic and evidence-based, but it cannot infer every publisher-specific launch contract.
- Version 0.2 has no publisher-provided per-game launch profiles, compatibility patch channel, commercial Store, game download or game installation manager.
- A configured executable path is not a record of a successful in-game session.

See [launcher discovery coverage](../discovery/LAUNCHER-COVERAGE.md) and [game discovery](game-discovery.md) for adapter details and verification semantics.

## What collaboration with publishers would enable

Amethys is not asking publishers to waive their security requirements or provide unrestricted access to protected game files. Useful, narrowly scoped cooperation could include:

- Documented launch entry points, supported command-line arguments and required working-directory rules.
- Official installation manifests or metadata interfaces that identify the correct executable and build variant.
- Documented dependencies and launch ordering for official launchers, anti-cheat services and drivers.
- A permissioned SDK/API or test environment for verifying installation identity and launch readiness without reading private or protected game data.
- A process for reviewing compatibility profiles and distributing publisher-approved updates when a game changes.
- A security contact or review process for validating that Amethys' read-only discovery and launch integration do not weaken platform protections.

With this information, Amethys could maintain versioned, per-game profiles as data rather than accumulating hard-coded exceptions. Profiles would describe approved launch methods and dependencies; they would not disable or replace publisher security components.

## How compatibility should be presented to partners

Use precise labels:

| Label | Meaning |
| --- | --- |
| Detected | Installation metadata was recognized. |
| Imported | Installation is present in the Amethys library. |
| Path verified | Installation directory and configured executable path passed filesystem checks. |
| Launch attempted | Amethys requested process creation; this alone is not proof that the game loaded. |
| In-game verified | A documented manual test reached the agreed usable game state. |
| Unresolved | Available evidence is insufficient to select a safe launch method. |
| Unsupported | A known platform, policy or technical restriction prevents the current integration. |

Every compatibility claim should record the game/build, operating system, launcher version, test date, evidence level and any required publisher components. Unknown and unresolved cases must remain visible in reports.

## Position of the 0.2 release

Amethys 0.2 is a working foundation for a local library, read-only discovery, executable analysis and direct launch. It is not a universal compatibility layer and is not a replacement for official stores or launchers.

The objective of publisher collaboration is to close the gaps that cannot responsibly be solved through guesswork: authoritative metadata, documented launch flows and security-approved integration. This is a deliberate engineering boundary, not a promise that every game already works.
