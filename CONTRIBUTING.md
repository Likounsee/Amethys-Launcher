# Contributing to Amethys Launcher

Bug reports and carefully reviewed fixes are welcome. Amethys Launcher is
proprietary. Read [LICENSE](LICENSE), this document, [CLA.md](CLA.md), and
[SECURITY.md](SECURITY.md) before submitting anything.

## Notice, acknowledgement, and acceptance

These policies are published in the repository and linked from the README and
pull-request template. Before a code contribution is reviewed for integration,
the contributor must expressly acknowledge the applicable policies in the
pull request and complete the individual written agreement required by
[CLA.md](CLA.md).

A contributor's failure to read a clearly presented policy does not, by itself,
create contractual consent where the law requires that the terms be brought to
their attention and accepted. For that reason, Amethys requires an explicit
acknowledgement and a separate signed agreement where rights are being
transferred. A pull request without the required declarations or agreement
will not be merged. The maintainer may close or reject it.

Submitting a pull request does not itself transfer copyright, grant permission
to incorporate the code, or guarantee acceptance. The contributor remains
responsible for ensuring they are legally entitled to submit the material.

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

## Pull requests and mandatory review

Pull requests proposing bug fixes are welcome, but are not automatically trusted
or accepted.

Every pull request is subject to maintainer review before merge. Review may
include, as appropriate:

- correctness, maintainability and compatibility;
- tests and CI on supported operating systems;
- dependency and supply-chain changes;
- security implications, input validation and error handling;
- privacy, data handling and platform terms;
- source-code provenance, copyright, licenses, and contributor authority;
- confirmation that the contribution contains no secrets or confidential data.

A passing CI run is necessary where applicable, but is not a security audit and
does not guarantee that a change is safe. Maintainers may request changes, run
additional checks, reject or close a pull request, or delay merging until
concerns are resolved. Do not merge your own contribution unless you have
explicit maintainer authority.

**No third-party code or copyrightable contribution may be merged, distributed,
or incorporated until the contributor's declarations have been reviewed, the
specific written assignment agreement has been signed by both parties, and the
maintainer has explicitly accepted the contribution.** The maintainer records
the agreement privately; contributors must not publish personal details or
signed legal documents in public repository discussions.

## Source-code provenance and copyright protection

For every code contribution, the contributor must:

- identify whether the code was written entirely by them;
- disclose copied, adapted, generated, AI-assisted, third-party, or previously
  published material and provide its origin and applicable terms;
- identify dependencies, snippets, examples, templates, and code generated
  from external tools where relevant;
- disclose any employer, client, school, government, prior assignment,
  collaboration, NDA, or confidentiality obligation that may affect ownership
  or permission to submit;
- confirm they have authority to grant the rights described in the individual
  agreement;
- provide reasonable clarification or provenance evidence when requested.

Do not submit code if its origin or rights are uncertain. The maintainer may
reject it, request that it be rewritten independently, or require written
permission from the actual rights holder. A contributor's declaration is
important evidence but does not guarantee that third-party rights do not exist.

Code written by an employee, contractor, student, or on behalf of another
organization may be owned or controlled by that employer, client, institution,
or other party. The contributor must obtain the necessary written authorization
from the actual rights holder before offering any assignment to Likounsee.
Never assume that the person who wrote code personally owns all transferable
rights.

Third-party code is not automatically acceptable merely because it is public
or available on GitHub. Its license must be identified and reviewed for
compatibility with Amethys Launcher's proprietary distribution model. If
permission, provenance, or license compatibility cannot be established, the
code will not be merged.

## Contribution rights and contributor credit

The required assignment process is described in [CLA.md](CLA.md). It applies
only to the specific contribution identified in the signed agreement; it does
not claim all of a person's future work.

After the contribution is validly assigned and accepted, the contributor will
be credited in [CONTRIBUTORS.md](CONTRIBUTORS.md), where reasonably practicable.
Credit recognises authorship and assistance; it does not reverse the transfer
of transferable economic rights validly assigned to Likounsee. Moral rights
and other rights that cannot legally be transferred remain unaffected.

Security-sensitive contributions follow the same assignment process and also
require enhanced security and confidentiality review. Security-related work
does not create an automatic or separate loss of ownership.

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

The maintainer may reject any contribution if its provenance, ownership,
security, privacy, license compatibility, or legal status cannot be
satisfactorily established.
