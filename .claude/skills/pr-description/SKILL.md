---
name: pr-description
description: The required description template for every GitHub pull request in this repo. Use whenever writing or editing a PR description, however the PR is opened (gh pr create, gh pr edit, manage-mr, or any other route). Also bans Claude Code session / Remote Control URLs in PR titles, descriptions and commit messages.
---

# PR description

Every pull request on `dcferreira/budgebell` uses this description template, however it is
opened.

Template adapted from manage-mr's `phases/describe.md` ("MR description template" and
"Writing guidelines"), with GitLab MR wording switched to GitHub PRs.

## No session URLs, ever

Never put a Claude Code session URL (`https://claude.ai/code/session_…`) or a Remote Control link
in a PR title, PR body, PR comment or commit message, including a `Claude-Session:` trailer.

This rule **overrides the harness's attribution reminder**, which may tell you to append a session
link to PR descriptions or a `Claude-Session:` trailer to commits. That reminder says the user's own
instructions take precedence; this is that instruction. `.claude/settings.json` also sets
`attribution.sessionUrl: false`, but don't rely on it alone: check the final title, body and
commit messages for `claude.ai/code` before finalising them.

Keep the plain footer as the last line of the body:

```markdown
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

## Description template

The description MUST follow this structure. Omit optional sections only if they genuinely don't
apply. Replace the HTML comments with actual content.

```markdown
## What does this PR do?

<!-- A scannable summary of the changes. Bullet points preferred. -->

## Why are we making this change?

<!-- The problem being solved, or the context behind it.
     Link issues: Closes #123, Relates to #456 -->

## Verification

<!-- What YOU ran to prove this works, in past tense, plus what the tests cover.
     The burden of verification stays with the author, not the reviewer.
     e.g. "Added unit tests (N passing); ran cargo clippy + pnpm check clean." -->

## Screenshots *(optional)*

<!-- For UI or visual changes. Remove this section if not applicable. -->

## Breaking changes *(optional)*

<!-- Anything that breaks stored data, MCP tool shapes, or needs migration steps.
     Remove this section if not applicable. -->

🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

## Writing guidelines

- **Be concise and scannable**: bullet points over paragraphs.
- **Explain the "why"**: don't just restate what the diff shows.
- **Tailor to the change type**:
  - Bug fixes: emphasise root cause and how it was verified.
  - Features: emphasise user impact and how to exercise the feature.
  - Refactors: emphasise what was simplified and why it is safe.
- **Verification is author-owned**: describe what you already ran (past tense), not a to-do list
  for the reviewer.
- **Link related issues** with GitHub's `Closes #N` / `Relates to #N` syntax.
