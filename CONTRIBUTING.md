# Contributing to Amethys Launcher

Bug reports and carefully reviewed fixes are welcome. The project is proprietary:
opening an issue or submitting a pull request does not grant a license to reuse
the project or its code. See [LICENSE](LICENSE).

## Reporting a bug

1. Search existing issues before opening a new one.
2. Describe the affected operating system, Amethys version/commit and exact steps
   to reproduce the problem.
3. Include expected and actual behavior, relevant sanitized logs and test results.
4. Use a minimal reproduction where possible.

Do not include passwords, access tokens, API keys, session cookies, personal
information, private platform data, partner-only documentation, proprietary
responses, or confidential information from Steam, Epic Games or any other
platform. Redact local usernames, machine-specific paths and identifiers where
they are not necessary. If a report may contain a security vulnerability or
confidential information, do not publish it in an issue; follow SECURITY.md.

## Pull requests

Pull requests proposing bug fixes are welcome, but are not automatically trusted
or accepted.

Every pull request is subject to maintainer review before merge. Review may
include, as appropriate:

- correctness, maintainability and compatibility;
- tests and CI on supported operating systems;
- dependency and supply-chain changes;
- security implications, input validation and error handling;
- privacy, data handling and platform terms;
- confirmation that the contribution contains no secrets or confidential data.

A passing CI run is necessary where applicable, but is not a security audit and
does not guarantee that a change is safe. Maintainers may request changes, run
additional checks, reject or close a pull request, or delay merging until
concerns are resolved. Do not merge your own contribution unless you have
explicit maintainer authority.

## Security and platform boundaries

- Keep platform discovery read-only unless a separately reviewed feature
  explicitly requires otherwise.
- Never change, move, rename, repair, delete or overwrite a user's game files
  or launcher metadata as part of detection.
- Do not bypass authentication, DRM, licensing, anti-cheat, access controls,
  rate limits or platform restrictions.
- Use only public, documented and permitted interfaces, or interfaces for
  which the project has explicit authorization.
- Never commit credentials, tokens, private keys, private API responses,
  partner material, personal data or confidential platform information.
- Do not add telemetry or transmit user/platform data without explicit,
  documented design approval.

## Contribution rights and required written agreement

Submitting a pull request does not transfer ownership of the submitted code.
The contributor keeps their rights while the proposal is being reviewed.

**No third-party code or copyrightable contribution may be merged, distributed,
or incorporated into Amethys Launcher until the contributor has signed a
separate written assignment agreement for that specific accepted contribution.**
The agreement must identify the contribution and specify the transferable
economic rights, scope, territory, duration, purpose and any compensation in
accordance with applicable law. A general statement in a pull request or this
document is not a substitute for that signed agreement.

Once the agreement is validly signed and the contribution is accepted, the
rights expressly assigned under that agreement belong to Likounsee to the
extent permitted by applicable law. The contributor will be credited in
[CONTRIBUTORS.md](CONTRIBUTORS.md). Credit recognises the contributor's work;
it does not give them ownership or control over the rights assigned to
Likounsee. Non-transferable moral rights remain unaffected.

Security-sensitive contributions follow the exact same assignment process.
They also require enhanced security and confidentiality review. Writing
security-related code does not automatically remove or transfer any additional
rights: only the specific signed agreement can do that.

Contributors must only submit work they are legally entitled to contribute.
Third-party code, generated code, employer-owned work, code subject to another
license, and code created under confidentiality obligations must be disclosed
before submission and may be rejected.

The maintainers decide whether a contribution is accepted. Submission alone
does not create a right to merge or distribute it.
