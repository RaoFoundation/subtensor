#!/usr/bin/env python3
"""
Post a persona's review to a PR.

Input: a JSON document produced by Codex against `codex-output-schema.json`.
Behaviour:

  1. Post a fresh sticky comment with the new verdict and a findings table.
     Each finding gets a stable ID = sha1(path:line:title)[:8], embedded in
     the row as `<!-- fid:xxxxxxxx -->` so future runs can match.
  2. Open a PR review with each inline finding as a review comment (with
     ```suggestion blocks where applicable, giving the one-click 'Apply'
     button on GitHub).
  3. If a previous sticky comment exists (matched by the persona marker),
     EDIT it in place to:
       - prepend a 'Superseded by <new comment URL>' header
       - render its findings table with strikethrough on every prior finding,
         annotated with status from the new comment's prior_reconciliation:
           addressed         -> ✅ Addressed
           no_longer_applies -> ⏭️ No longer applies
           not_addressed     -> ➡️ Carried forward to <new comment URL>
       - remove the old prior-reconciliation and conclusion sections
     The OLD comment thus becomes a compact historical record; the NEW comment
     is the live status.

Usage:
  GH_TOKEN=... python3 post_review.py \
    --persona skeptic --pr 2668 --repo RaoFoundation/subtensor \
    --commit-sha <sha> --input-file skeptic-output.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from typing import Any


SEVERITY_RANK = {"CRITICAL": 0, "HIGH": 1, "MEDIUM": 2, "LOW": 3}


def gh_api(
    method: str,
    path: str,
    body: dict | None = None,
    paginate: bool = False,
) -> dict | list:
    """
    Call gh api; raise on non-zero. Returns parsed JSON, or {} for empty.

    paginate=True is for GET list endpoints — uses `--paginate --slurp` so
    multi-page responses come back as [[page1], [page2], ...], then we flatten
    page-of-arrays into a single array in Python. (gh rejects --slurp together
    with --jq, so we do the flatten here instead of via `--jq add`.)
    """
    cmd = ["gh", "api"]
    if paginate:
        cmd += ["--paginate", "--slurp"]
    cmd += ["-X", method, path]
    if body is not None:
        cmd += ["--input", "-"]
    proc = subprocess.run(
        cmd,
        input=json.dumps(body) if body is not None else None,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"gh api {method} {path} failed:\n  stdout={proc.stdout}\n  stderr={proc.stderr}"
        )
    parsed = json.loads(proc.stdout) if proc.stdout.strip() else {}
    if paginate and isinstance(parsed, list):
        # Slurp gives us a list of pages. If each page is itself a list (the
        # usual case for list endpoints), flatten into a single array. Object
        # endpoints would yield a list of objects, which is also fine to return.
        if all(isinstance(p, list) for p in parsed):
            flat: list = []
            for page in parsed:
                flat.extend(page)
            return flat
    return parsed


def finding_id(path: str, line: int | str, title: str) -> str:
    """Stable 8-char ID derived from a finding's location + title."""
    key = f"{path}:{line}:{title.strip().lower()}"
    return hashlib.sha1(key.encode()).hexdigest()[:8]


def render_inline_comment_body(f: dict) -> str:
    """Build the markdown body of an inline review comment (incl. fid marker + suggestion fence)."""
    severity = f["severity"]
    title = f["title"].strip()
    body = (f.get("body_markdown") or "").strip()
    fid = finding_id(f["path"], f["line"], title)
    parts = [
        f"**[{severity}] {title}**",
        "",
        body,
    ]
    suggestion = f.get("suggestion")
    if suggestion is not None and suggestion != "":
        parts += ["", "```suggestion", suggestion.rstrip("\n"), "```"]
    parts += ["", f"<!-- fid:{fid} -->"]
    return "\n".join(parts).strip() + "\n"


def to_review_comment(f: dict) -> dict:
    """Translate our inline-finding schema to GitHub's review-comment schema."""
    side = (f.get("side") or "RIGHT").upper()
    c: dict[str, Any] = {
        "path": f["path"],
        "line": int(f["line"]),
        "side": side,
        "body": render_inline_comment_body(f),
    }
    if f.get("start_line") is not None:
        c["start_line"] = int(f["start_line"])
        c["start_side"] = side
    return c


def post_review(
    repo: str, pr: int, commit_sha: str, comments: list[dict]
) -> tuple[int, list[dict]]:
    if not comments:
        return (0, [])
    review = gh_api(
        "POST",
        f"repos/{repo}/pulls/{pr}/reviews",
        {
            "commit_id": commit_sha,
            "event": "COMMENT",
            "body": "AI review — see the sticky summary comment for the verdict and the inline comments below for specific findings.",
            "comments": comments,
        },
    )
    review_id = int(review.get("id", 0))
    # A single review can technically exceed 100 comments; paginate to be safe.
    posted = gh_api(
        "GET",
        f"repos/{repo}/pulls/{pr}/reviews/{review_id}/comments?per_page=100",
        paginate=True,
    )
    return (review_id, posted if isinstance(posted, list) else [])


def render_findings_table(
    inline: list[dict], off_diff: list[dict], inline_urls: dict[str, str]
) -> str:
    """Build the live findings table for the new sticky comment."""
    rows: list[tuple[str, str, str, str, str, str]] = []  # (sev_rank, sev, file, title, link, fid)
    for f in inline:
        fid = finding_id(f["path"], f["line"], f["title"])
        sev = f["severity"].upper()
        link = inline_urls.get(fid, "")
        link_md = f"[inline]({link})" if link else "_(post-failed)_"
        rows.append(
            (
                str(SEVERITY_RANK.get(sev, 99)),
                sev,
                f"`{f['path']}:{f['line']}`",
                f["title"].strip().replace("|", "\\|"),
                link_md,
                fid,
            )
        )
    for f in off_diff:
        title = f["title"].strip().replace("|", "\\|")
        sev = f["severity"].upper()
        loc = f.get("approximate_location") or "—"
        fid = finding_id(loc, 0, title)
        rows.append(
            (str(SEVERITY_RANK.get(sev, 99)), sev, f"_{loc}_", title, "_(off-diff)_", fid),
        )
    if not rows:
        return "_No findings._"
    rows.sort()
    lines = ["| Sev | File | Finding |  |", "| --- | --- | --- | --- |"]
    for _, sev, fileloc, title, link, fid in rows:
        lines.append(f"| **{sev}** | {fileloc} | {title} <!-- fid:{fid} --> | {link} |")
    return "\n".join(lines)


def parse_prior_findings(prior_body: str) -> list[dict]:
    """
    Parse rows out of the prior sticky comment's findings table.
    Returns list of {fid, sev, fileloc, title, link_md}.
    """
    rows: list[dict] = []
    pattern = re.compile(
        r"^\|\s*\*\*(?P<sev>[A-Z]+)\*\*\s*\|\s*(?P<fileloc>[^|]+?)\s*\|\s*(?P<title>.+?)<!--\s*fid:(?P<fid>[A-Za-z0-9]+)\s*-->\s*\|\s*(?P<link>[^|]+?)\s*\|",
        re.MULTILINE,
    )
    for m in pattern.finditer(prior_body):
        rows.append(
            {
                "fid": m.group("fid"),
                "sev": m.group("sev"),
                "fileloc": m.group("fileloc").strip(),
                "title": m.group("title").strip().rstrip(),
                "link_md": m.group("link").strip(),
            }
        )
    return rows


_PERSONA_HEADER = {
    "skeptic": "# 🛡️ AI Review — **Skeptic** (security review)",
    "auditor": "# 🔍 AI Review — **Auditor** (domain review)",
}


def render_new_sticky(
    persona: str,
    verdict: str,
    scrutiny_note: str,
    summary_markdown: str,
    conclusion_markdown: str,
    findings_table: str,
    off_diff: list[dict],
    reconciliation: list[dict],
    prior_url: str | None,
) -> str:
    """Build the body of the new sticky comment."""
    parts = [
        _PERSONA_HEADER.get(persona, f"# AI Review — {persona}"),
        "",
        f"**VERDICT:** {verdict}",
        "",
        scrutiny_note.strip(),
    ]
    if prior_url:
        parts += ["", f"_Supersedes [previous review]({prior_url})._"]
    if summary_markdown.strip():
        parts += ["", summary_markdown.strip()]
    parts += ["", "## Findings", "", findings_table.strip()]
    if off_diff:
        parts += ["", "## Other findings", ""]
        for f in off_diff:
            sev = f["severity"].upper()
            title = f["title"].strip()
            body = f.get("body_markdown", "").strip()
            loc = f.get("approximate_location")
            loc_md = f" _({loc})_" if loc else ""
            parts.append(f"- **[{sev}]** {title}{loc_md} — {body}")
    if reconciliation:
        parts += ["", "## Prior-comment reconciliation", ""]
        for r in reconciliation:
            status = r["status"].replace("_", " ")
            note = r.get("note_markdown")
            line = f"- `{r['prior_finding_id']}`: **{status}**"
            if note:
                line += f" — {note.strip()}"
            parts.append(line)
    parts += ["", "## Conclusion", "", conclusion_markdown.strip(),
              "", f"<!-- ai-review:{persona} -->"]
    return "\n".join(parts).strip() + "\n"


def _post_error_sticky(repo: str, pr: int, persona: str, message: str, raw: str) -> None:
    """
    Surface a Codex-output failure in the persona's section of the unified
    sticky. On the next run, the agent reads `prior-<persona>-comment.md`
    (which contains this section), giving it direct feedback to self-correct.
    """
    marker = f"<!-- ai-review:{persona} -->"
    header = _PERSONA_HEADER.get(persona, f"# AI Review — {persona}")
    # Truncate raw output so the comment isn't enormous.
    raw_trim = raw if len(raw) <= 4000 else raw[:2000] + "\n\n[... truncated ...]\n\n" + raw[-2000:]
    body = (
        f"{header}\n\n"
        f"**VERDICT:** ERROR\n\n"
        f"⚠️ **Codex output failed validation.** {message}\n\n"
        f"<details><summary>Raw model output ({len(raw)} chars)</summary>\n\n"
        f"```\n{raw_trim}\n```\n\n</details>\n\n"
        f"{marker}\n"
    )
    try:
        # Error path emits no reconciliation; archive of prior findings is
        # preserved as-is by render_section_archive (the prior section's
        # findings show "❔ Status unknown in current run").
        upsert_persona_section(repo, pr, persona, body, reconciliation=[])
    except Exception as e:  # last-resort: surface in logs
        print(f"::error::Failed to post error sticky: {e}", file=sys.stderr)
        print(f"::error::Original Codex output ({len(raw)} chars):", file=sys.stderr)
        print(raw_trim, file=sys.stderr)


_PR_BODY_TRIVIAL_MAX_CHARS = 150


def _pr_body_is_trivial(body: str) -> bool:
    """
    A PR body is considered 'trivial' if (after stripping the GitHub PR template
    boilerplate, checkbox lines, and headings) less than ~150 chars of real
    prose remain. Used to decide whether the auditor's proposed_pr_body should
    auto-apply.
    """
    if body is None:
        return True
    # Strip lines that are just headers, checkboxes, comments, or empty.
    keep_lines: list[str] = []
    for line in body.splitlines():
        s = line.strip()
        if not s:
            continue
        if s.startswith("#"):
            continue
        if s.startswith("<!--") and s.endswith("-->"):
            continue
        if re.match(r"^[-*]\s*\[[ xX]\]", s):  # markdown checkbox
            continue
        if re.match(r"^[-*]\s+(N/A|TBD|—|-)\s*$", s, re.IGNORECASE):
            continue
        if s in {"## Description", "## Related Issue(s)", "## Type of Change",
                 "## Breaking Change", "## Checklist", "## Screenshots (if applicable)",
                 "## Additional Notes"}:
            continue
        keep_lines.append(s)
    substance = " ".join(keep_lines)
    return len(substance) < _PR_BODY_TRIVIAL_MAX_CHARS


def maybe_patch_pr_body(
    repo: str, pr: int, proposed: str | None
) -> str | None:
    """
    If the auditor proposed a body AND the current body is trivial, PATCH it.
    Returns a short note for the sticky summary, or None if no action taken.
    """
    if not proposed or not proposed.strip():
        return None
    try:
        pr_obj = gh_api("GET", f"repos/{repo}/pulls/{pr}")
    except RuntimeError as e:
        print(f"::warning::Could not read PR for body check: {e}", file=sys.stderr)
        return None
    if not isinstance(pr_obj, dict):
        return None
    current = pr_obj.get("body") or ""
    if not _pr_body_is_trivial(current):
        return (
            "_The Auditor proposed a replacement PR description, but the "
            "current body is non-trivial; not overwriting. Maintainers: ask "
            "the Auditor to regenerate if you want it._"
        )
    try:
        gh_api("PATCH", f"repos/{repo}/pulls/{pr}", {"body": proposed})
        print("Patched PR body with auditor's proposal.", file=sys.stderr)
        return "_PR body was empty/trivial; the Auditor has auto-filled it. Please review._"
    except RuntimeError as e:
        print(f"::warning::Failed to patch PR body: {e}", file=sys.stderr)
        return f"_Auditor proposed a PR body but the PATCH failed: {e}_"


UNIFIED_MARKER = "<!-- ai-review:unified -->"
_ARCHIVE_BEGIN_RE = re.compile(
    r"<details>\s*<summary>[^<]*Previous run \(superseded\)[^<]*</summary>.*?</details>",
    re.DOTALL,
)
_STATUS_LABEL = {
    "addressed": "✅ Addressed",
    "no_longer_applies": "⏭️ No longer applies",
    "not_addressed": "➡️ Carried forward to current findings",
}


def _section_markers(persona: str) -> tuple[str, str]:
    return (f"<!-- ai-review:{persona}:begin -->",
            f"<!-- ai-review:{persona}:end -->")


def render_persona_section(persona: str, body: str) -> str:
    begin, end = _section_markers(persona)
    return f"{begin}\n\n{body.strip()}\n\n{end}"


def render_placeholder_section(persona: str) -> str:
    label = persona.capitalize()
    return render_persona_section(
        persona,
        f"_{_PERSONA_HEADER.get(persona, label)} has not yet run on this PR._",
    )


def render_unified_comment(skeptic_section: str, auditor_section: str) -> str:
    """Compose the unified sticky body. Both sections always present."""
    return (
        f"{UNIFIED_MARKER}\n\n"
        f"{skeptic_section}\n\n"
        f"---\n\n"
        f"{auditor_section}\n"
    )


def extract_section_body(unified_body: str, persona: str) -> str:
    """Pull out the inner content between this persona's begin/end markers."""
    begin, end = _section_markers(persona)
    pattern = re.compile(
        re.escape(begin) + r"\s*(.*?)\s*" + re.escape(end), re.DOTALL
    )
    m = pattern.search(unified_body)
    return m.group(1).strip() if m else ""


def replace_persona_section(body: str, persona: str, new_section: str) -> str:
    """
    Replace the persona's existing section in the unified comment body. If
    absent (e.g. the comment was created with just the other persona's section),
    append it after a horizontal rule.
    """
    begin, end = _section_markers(persona)
    pattern = re.compile(re.escape(begin) + r".*?" + re.escape(end), re.DOTALL)
    # re.sub treats backslashes in `repl` as escape sequences; pass a lambda
    # to insert new_section literally.
    if pattern.search(body):
        return pattern.sub(lambda _m: new_section, body)
    return body.rstrip() + "\n\n---\n\n" + new_section + "\n"


def find_unified_sticky(
    repo: str, pr: int
) -> tuple[int | None, str, str]:
    """Find the single unified ai-review sticky on the PR, if it exists."""
    comments = gh_api(
        "GET",
        f"repos/{repo}/issues/{pr}/comments?per_page=100",
        paginate=True,
    )
    if not isinstance(comments, list):
        return (None, "", "")
    for c in comments:
        if UNIFIED_MARKER in c.get("body", ""):
            return (int(c["id"]), c.get("body", ""), c.get("html_url", ""))
    return (None, "", "")


def post_new_sticky(repo: str, pr: int, body: str) -> dict:
    return gh_api("POST", f"repos/{repo}/issues/{pr}/comments", {"body": body})


def edit_comment(repo: str, comment_id: int, body: str) -> None:
    gh_api("PATCH", f"repos/{repo}/issues/comments/{comment_id}", {"body": body})


def render_section_archive(
    prior_section_body: str, reconciliation: list[dict]
) -> str:
    """
    Build a collapsed <details> block showing the just-superseded findings
    with strikethrough + addressed/not-addressed/no-longer-applies status from
    the new run's reconciliation. Each rerun replaces the prior archive (we
    don't chain history — comment would grow forever; GitHub's comment 'edited'
    tab preserves the full trail anyway).
    """
    # Strip any pre-existing archive from the prior section before parsing, so
    # we only annotate the LAST live findings, not older archives.
    section_no_archive = _ARCHIVE_BEGIN_RE.sub("", prior_section_body)
    rows = parse_prior_findings(section_no_archive)
    if not rows:
        return ""
    status_by_fid: dict[str, dict] = {
        r["prior_finding_id"]: r
        for r in reconciliation
        if r.get("prior_finding_id")
    }
    table_lines = [
        "| ~~Sev~~ | ~~File~~ | ~~Finding~~ | Status |",
        "| --- | --- | --- | --- |",
    ]
    for r in rows:
        rec = status_by_fid.get(r["fid"])
        if rec is None:
            status_md = "❔ Status unknown in current run"
        else:
            status_md = _STATUS_LABEL.get(rec["status"], rec["status"])
            note = rec.get("note_markdown")
            if note:
                status_md += f"<br/>_{note.strip()}_"
        table_lines.append(
            f"| ~~**{r['sev']}**~~ | ~~{r['fileloc']}~~ | ~~{r['title']}~~ | {status_md} |"
        )
    return (
        "<details>\n"
        "<summary>📜 Previous run (superseded)</summary>\n\n"
        + "\n".join(table_lines)
        + "\n\n</details>"
    )


def upsert_persona_section(
    repo: str,
    pr: int,
    persona: str,
    new_inner: str,
    reconciliation: list[dict],
) -> str:
    """
    Find or create the unified sticky and replace this persona's section with
    `new_inner` plus an archive of the prior section's findings. Returns the
    html_url of the (created or updated) unified comment.
    """
    existing_id, existing_body, existing_url = find_unified_sticky(repo, pr)

    if existing_id is None:
        # First run on this PR — initialize the unified sticky. No prior to
        # archive.
        full_section = render_persona_section(persona, new_inner)
        other = "auditor" if persona == "skeptic" else "skeptic"
        placeholder = render_placeholder_section(other)
        unified = (
            render_unified_comment(full_section, placeholder)
            if persona == "skeptic"
            else render_unified_comment(placeholder, full_section)
        )
        created = post_new_sticky(repo, pr, unified)
        return created.get("html_url", "")

    # Sticky exists. Extract this persona's prior section content (if any) and
    # build an archive of its findings annotated with reconciliation status.
    prior_inner = extract_section_body(existing_body, persona)
    archive = render_section_archive(prior_inner, reconciliation) if prior_inner else ""
    new_inner_full = new_inner.rstrip()
    if archive:
        new_inner_full += "\n\n---\n\n" + archive
    full_section = render_persona_section(persona, new_inner_full)
    new_body = replace_persona_section(existing_body, persona, full_section)
    edit_comment(repo, existing_id, new_body)
    return existing_url


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--persona", required=True, choices=["skeptic", "auditor"])
    p.add_argument("--pr", required=True, type=int)
    p.add_argument("--repo", required=True)
    p.add_argument("--commit-sha", required=True)
    p.add_argument("--input-file", required=True,
                   help="JSON file produced by Codex against codex-output-schema.json")
    args = p.parse_args()

    if not os.environ.get("GH_TOKEN"):
        print("::error::GH_TOKEN must be set", file=sys.stderr)
        return 1

    with open(args.input_file) as f:
        raw = f.read().strip()
    if not raw:
        # Even an empty Codex output should produce a sticky so the next run's
        # `prior-*-comment.md` makes the failure visible to the agent.
        _post_error_sticky(
            args.repo, args.pr, args.persona,
            "Codex produced no output. Check the workflow logs for the model error.",
            raw="(empty)",
        )
        return 1
    try:
        doc = json.loads(raw)
    except json.JSONDecodeError as e:
        # Pass the error back to the agent via the next run's prior-comment.md.
        _post_error_sticky(
            args.repo, args.pr, args.persona,
            f"Codex emitted output that did not parse as JSON: {e}. "
            "On the next run, you (the agent) will see this comment as your "
            "prior verdict — please re-emit the output strictly per "
            "`codex-output-schema.json` (valid JSON, all required fields).",
            raw=raw,
        )
        return 1

    # Validate required top-level fields. If anything is missing, post an
    # error sticky so the agent sees the schema mismatch on the next run.
    required = {
        "verdict": (str,),
        "scrutiny_note": (str,),
        "summary_markdown": (str,),
        "conclusion_markdown": (str,),
        "inline_findings": (list,),
        "off_diff_findings": (list,),
        "prior_reconciliation": (list,),
        "proposed_pr_body": (str, type(None)),
    }
    problems: list[str] = []
    for key, typs in required.items():
        if key not in doc:
            problems.append(f"missing required field `{key}`")
        elif not isinstance(doc[key], typs):
            names = "|".join(t.__name__ for t in typs)
            problems.append(f"`{key}` must be {names}, got {type(doc[key]).__name__}")
    if problems:
        _post_error_sticky(
            args.repo, args.pr, args.persona,
            "Codex output parsed as JSON but does not match the schema: "
            + "; ".join(problems)
            + ". Re-emit strictly per `codex-output-schema.json`.",
            raw=raw,
        )
        return 1

    verdict = (doc.get("verdict") or "").strip()
    inline = doc.get("inline_findings") or []
    off_diff = doc.get("off_diff_findings") or []
    reconciliation = doc.get("prior_reconciliation") or []

    # Auditor-only: maybe PATCH the PR body. Prepend the resulting note to
    # summary_markdown so the sticky reflects the action taken.
    if args.persona == "auditor":
        note = maybe_patch_pr_body(args.repo, args.pr, doc.get("proposed_pr_body"))
        if note:
            existing = doc.get("summary_markdown") or ""
            doc["summary_markdown"] = note + ("\n\n" + existing if existing.strip() else "")

    # 1. Post the inline review (if any findings have a pinnable line).
    inline_urls: dict[str, str] = {}
    posted: list[dict] = []
    if inline:
        try:
            review_comments = [to_review_comment(f) for f in inline]
            _, posted = post_review(args.repo, args.pr, args.commit_sha, review_comments)
        except RuntimeError as e:
            print(f"::warning::review post failed; rendering without inline links: {e}",
                  file=sys.stderr)
        # Match returned comments back to our findings by fid embedded in the body.
        for c in posted:
            body = c.get("body", "")
            m = re.search(r"<!--\s*fid:([A-Za-z0-9]+)\s*-->", body)
            if m:
                inline_urls[m.group(1)] = c.get("html_url", "")

    # 2. Build this persona's section body and upsert into the unified sticky.
    findings_table = render_findings_table(inline, off_diff, inline_urls)
    section_body = render_new_sticky(
        persona=args.persona,
        verdict=verdict,
        scrutiny_note=doc.get("scrutiny_note", ""),
        summary_markdown=doc.get("summary_markdown", ""),
        conclusion_markdown=doc.get("conclusion_markdown", ""),
        findings_table=findings_table,
        off_diff=off_diff,
        reconciliation=reconciliation,
        prior_url=None,
    )
    url = upsert_persona_section(
        args.repo, args.pr, args.persona, section_body, reconciliation
    )
    print(f"Updated unified sticky ({args.persona} section): {url}", file=sys.stderr)
    # If running inside a GitHub Actions step, surface the URL + verdict as
    # step outputs so a downstream notify job can post a single "review
    # updated" pointer comment at the bottom of the PR.
    gh_output = os.environ.get("GITHUB_OUTPUT")
    if gh_output and url:
        with open(gh_output, "a") as f:
            f.write(f"posted_url={url}\n")
            f.write(f"verdict={verdict}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
