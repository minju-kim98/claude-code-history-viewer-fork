#!/usr/bin/env python3
"""Backfill `tokens: null` in DITCodeAgent local session logs.

DITCodeAgent writes per-message token usage as `null` in its session files
(`~/.ditcodeagent/tmp/<project>/chats/session-*.json`), so the history viewer
shows 0 tokens / 0 cost for this provider. The real usage was never recorded
and cannot be recovered. This script *reconstructs an approximation* using
Anthropic's `POST /v1/messages/count_tokens` endpoint and writes the estimated
values back into each `tokens` object.

What gets filled, per assistant ("gemini") message (cache-aware model so the
`input` bucket does not explode quadratically across a long session):

    input   ≈ tokens of the NEW user/tool turns since the previous assistant
              reply (i.e. the freshly-sent prompt for this turn)
    cached  ≈ tokens of all earlier context the model had already seen
    output  ≈ tokens of this assistant message's own generated content
              (text + thinking + tool-call args)
    thoughts≈ tokens of just the thinking content (informational subset of output)
    tool    = 0  (cannot be reconstructed)
    total   = input + cached + output

These are ESTIMATES. They omit the system prompt and tool definitions (not in
the log) and cannot know real cache hit/miss behavior, so treat them as
"good enough for charts", not as billing-accurate numbers.

Usage:
    # 1. Scan only — no API key needed. Reports files and how many nulls exist.
    python scripts/ditcodeagent_backfill_tokens.py --scan

    # 2. Real backfill — needs an API key.
    set ANTHROPIC_API_KEY=sk-ant-...        # Windows (PowerShell: $env:ANTHROPIC_API_KEY="...")
    python scripts/ditcodeagent_backfill_tokens.py --backup

    # Preview without writing:
    python scripts/ditcodeagent_backfill_tokens.py --dry-run

Flags:
    --scan        Count nulls only; never calls the API or writes (no key needed).
    --dry-run     Compute estimates (calls API) but do not write files.
    --force       Recompute even messages whose tokens are already filled.
    --backup      Keep a <file>.bak copy before overwriting.
    --root PATH   Override the DITCodeAgent home (default: $DITCODEAGENT_HOME or ~/.ditcodeagent).
    --limit N     Process at most N session files (handy for a trial run).
    --verbose     Per-file logging.

Safety: run this when DITCodeAgent is NOT actively writing those sessions.
Writes are atomic (temp file + os.replace); messages other than `tokens` are
left untouched.
"""
from __future__ import annotations

import argparse
import glob
import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.request

# Optional: load ANTHROPIC_API_KEY (and friends) from a .env file in the repo
# root / cwd. Requires `pip install python-dotenv`. If the package or the file
# is absent, a directly-exported env var still works.
try:
    from dotenv import load_dotenv

    load_dotenv()
except ImportError:
    pass

API_URL = "https://api.anthropic.com/v1/messages/count_tokens"
API_VERSION = "2023-06-01"
DEFAULT_MODEL = "claude-sonnet-4-6"  # tokenizer is shared; used as fallback


# --------------------------------------------------------------------------
# Path discovery
# --------------------------------------------------------------------------
def get_root(cli_root: str | None) -> str:
    if cli_root:
        return os.path.expanduser(cli_root)
    env = os.environ.get("DITCODEAGENT_HOME")
    if env and os.path.isdir(env):
        return env
    return os.path.join(os.path.expanduser("~"), ".ditcodeagent")


def find_session_files(root: str) -> list[str]:
    pattern = os.path.join(root, "tmp", "*", "chats", "session-*.json")
    return sorted(glob.glob(pattern))


# --------------------------------------------------------------------------
# Content flattening (for token counting only — approximate)
# --------------------------------------------------------------------------
def _part_to_text(part) -> str:
    if isinstance(part, str):
        return part
    if not isinstance(part, dict):
        return ""
    if "text" in part and isinstance(part["text"], str):
        return part["text"]
    if "functionCall" in part:
        fc = part["functionCall"] or {}
        return f'{fc.get("name", "")} {json.dumps(fc.get("args", {}), ensure_ascii=False)}'
    if "functionResponse" in part:
        fr = part["functionResponse"] or {}
        resp = fr.get("response") or {}
        return str(resp.get("output", "")) if isinstance(resp, dict) else str(resp)
    if "executableCode" in part:
        return str((part["executableCode"] or {}).get("code", ""))
    if "codeExecutionResult" in part:
        return str((part["codeExecutionResult"] or {}).get("output", ""))
    # inlineData / fileData (binary) contribute little reconstructable text
    return ""


def flatten_content(content) -> str:
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(t for t in (_part_to_text(p) for p in content) if t)
    return ""


def flatten_thoughts(msg: dict) -> str:
    thoughts = msg.get("thoughts")
    if not isinstance(thoughts, list):
        return ""
    chunks = []
    for t in thoughts:
        if not isinstance(t, dict):
            continue
        subject = t.get("subject") or ""
        desc = t.get("description") or ""
        chunks.append(f"{subject}\n{desc}".strip())
    return "\n".join(c for c in chunks if c)


def message_own_text(msg: dict) -> str:
    """All text the message itself contributes (content + thinking)."""
    return "\n".join(t for t in (flatten_thoughts(msg), flatten_content(msg.get("content"))) if t)


def is_assistant(msg: dict) -> bool:
    return msg.get("type") in ("gemini", "ditcodeagent")


def map_model(model: str | None, bad_models: set[str]) -> str:
    if not model:
        return DEFAULT_MODEL
    mapped = model.replace(".", "-")  # claude-opus-4.7 -> claude-opus-4-7
    return DEFAULT_MODEL if mapped in bad_models else mapped


# --------------------------------------------------------------------------
# Token counting (Anthropic API)
# --------------------------------------------------------------------------
class TokenCounter:
    def __init__(self, api_key: str, verbose: bool = False, rpm: int = 0):
        self.api_key = api_key
        self.verbose = verbose
        self.cache: dict[str, int] = {}
        self.bad_models: set[str] = set()
        self.calls = 0
        # Proactive throttle: keep at least `min_interval` seconds between
        # calls to stay under the account's count_tokens RPM limit
        # (Tier 1 = 100, Tier 2 = 2000, Tier 3 = 4000, Tier 4 = 8000 RPM).
        self.min_interval = 60.0 / rpm if rpm > 0 else 0.0
        self._last_call = 0.0

    def count(self, text: str, model: str) -> int:
        if not text or not text.strip():
            return 0
        key = hashlib.sha1(f"{model}\x00{text}".encode("utf-8")).hexdigest()
        if key in self.cache:
            return self.cache[key]
        n = self._request(text, model)
        self.cache[key] = n
        return n

    def _request(self, text: str, model: str) -> int:
        body = json.dumps(
            {"model": model, "messages": [{"role": "user", "content": text}]}
        ).encode("utf-8")
        req = urllib.request.Request(
            API_URL,
            data=body,
            headers={
                "x-api-key": self.api_key,
                "anthropic-version": API_VERSION,
                "content-type": "application/json",
            },
            method="POST",
        )
        if self.min_interval > 0.0:
            wait = self.min_interval - (time.monotonic() - self._last_call)
            if wait > 0:
                time.sleep(wait)
        backoff = 2.0
        for attempt in range(6):
            try:
                self.calls += 1
                self._last_call = time.monotonic()
                with urllib.request.urlopen(req, timeout=60) as resp:
                    data = json.loads(resp.read().decode("utf-8"))
                    return int(data.get("input_tokens", 0))
            except urllib.error.HTTPError as e:
                detail = e.read().decode("utf-8", "replace")
                # Unknown model -> fall back to default and retry once with it.
                if e.code in (400, 404) and model not in self.bad_models and model != DEFAULT_MODEL:
                    self.bad_models.add(model)
                    if self.verbose:
                        print(f"  model '{model}' rejected; falling back to {DEFAULT_MODEL}",
                              file=sys.stderr)
                    return self._request(text, DEFAULT_MODEL)
                if e.code in (429, 500, 503, 529):
                    if self.verbose:
                        print(f"  HTTP {e.code}; retrying in {backoff:.0f}s", file=sys.stderr)
                    time.sleep(backoff)
                    backoff = min(backoff * 2, 60)
                    continue
                raise RuntimeError(f"count_tokens failed (HTTP {e.code}): {detail}") from e
            except urllib.error.URLError as e:
                if self.verbose:
                    print(f"  network error: {e}; retrying in {backoff:.0f}s", file=sys.stderr)
                time.sleep(backoff)
                backoff = min(backoff * 2, 60)
        raise RuntimeError("count_tokens failed after retries")


# --------------------------------------------------------------------------
# Per-file processing
# --------------------------------------------------------------------------
def count_nulls(messages: list) -> int:
    return sum(1 for m in messages if isinstance(m, dict) and is_assistant(m) and m.get("tokens") is None)


def backfill_file(
    path: str,
    counter: TokenCounter | None,
    *,
    force: bool,
    dry_run: bool,
    backup: bool,
    verbose: bool,
) -> tuple[int, int]:
    """Return (filled_count, assistant_count). counter=None means scan only."""
    try:
        with open(path, "r", encoding="utf-8") as f:
            doc = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        print(f"  skip (cannot read/parse): {path} ({e})", file=sys.stderr)
        return (0, 0)

    messages = doc.get("messages")
    if not isinstance(messages, list):
        return (0, 0)

    nulls = count_nulls(messages)
    assistants = sum(1 for m in messages if isinstance(m, dict) and is_assistant(m))

    if counter is None:  # --scan
        return (nulls, assistants)

    if nulls == 0 and not force:
        if verbose:
            print(f"  already complete: {os.path.basename(path)}")
        return (0, assistants)

    total_prior = 0   # tokens of all messages before the current index
    window_new = 0    # tokens of user/tool turns since the last assistant reply
    filled = 0

    for msg in messages:
        if not isinstance(msg, dict):
            continue
        model = map_model(msg.get("model"), counter.bad_models)
        own = message_own_text(msg)
        own_tokens = counter.count(own, model)

        if is_assistant(msg):
            needs = force or msg.get("tokens") is None
            if needs:
                inp = window_new
                cached = max(total_prior - window_new, 0)
                out = own_tokens  # includes generated thinking (counted within content)
                msg["tokens"] = {
                    "input": inp,
                    "output": out,
                    "cached": cached,
                    "thoughts": 0,  # generated thinking already folded into `output`
                    "tool": 0,
                    "total": inp + cached + out,
                }
                filled += 1
            total_prior += own_tokens
            window_new = 0
        else:
            total_prior += own_tokens
            window_new += own_tokens

    if filled == 0:
        return (0, assistants)

    if dry_run:
        print(f"  [dry-run] would fill {filled} message(s): {os.path.basename(path)}")
        return (filled, assistants)

    if backup:
        try:
            with open(path + ".bak", "w", encoding="utf-8") as bf, open(path, "r", encoding="utf-8") as orig:
                bf.write(orig.read())
        except OSError as e:
            print(f"  warning: backup failed for {path} ({e})", file=sys.stderr)

    tmp = f"{path}.tmp.{os.getpid()}"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(doc, f, ensure_ascii=False, indent=2)
    os.replace(tmp, path)
    if verbose:
        print(f"  filled {filled} message(s): {os.path.basename(path)}")
    return (filled, assistants)


# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------
def main() -> int:
    ap = argparse.ArgumentParser(description="Backfill null token counts in DITCodeAgent session logs.")
    ap.add_argument("--scan", action="store_true", help="Count nulls only; no API calls, no writes.")
    ap.add_argument("--dry-run", action="store_true", help="Compute estimates but do not write.")
    ap.add_argument("--force", action="store_true", help="Recompute already-filled messages too.")
    ap.add_argument("--backup", action="store_true", help="Keep a .bak copy before overwriting.")
    ap.add_argument("--root", default=None, help="DITCodeAgent home override.")
    ap.add_argument("--limit", type=int, default=0, help="Process at most N files (0 = all).")
    ap.add_argument("--rpm", type=int, default=0,
                    help="Throttle to at most N count_tokens calls/min (0 = no throttle, "
                         "rely on 429 backoff). Tier 1 limit is 100; use e.g. --rpm 90.")
    ap.add_argument("--verbose", action="store_true", help="Per-file logging.")
    args = ap.parse_args()

    root = get_root(args.root)
    files = find_session_files(root)
    if args.limit > 0:
        files = files[: args.limit]

    if not files:
        print(f"No session files found under {os.path.join(root, 'tmp')}", file=sys.stderr)
        return 1

    print(f"Found {len(files)} session file(s) under {root}")

    counter: TokenCounter | None = None
    if not args.scan:
        api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not api_key:
            print("ERROR: ANTHROPIC_API_KEY is not set (required unless --scan).", file=sys.stderr)
            return 2
        counter = TokenCounter(api_key, verbose=args.verbose, rpm=args.rpm)

    total_nulls = total_filled = total_assistant = 0
    for path in files:
        filled, assistants = backfill_file(
            path, counter,
            force=args.force, dry_run=args.dry_run, backup=args.backup, verbose=args.verbose,
        )
        total_assistant += assistants
        if args.scan:
            total_nulls += filled  # in scan mode `filled` carries the null count
        else:
            total_filled += filled

    print("-" * 60)
    if args.scan:
        print(f"assistant messages: {total_assistant}")
        print(f"with tokens == null: {total_nulls}")
        print("Run without --scan (and with ANTHROPIC_API_KEY) to backfill.")
    else:
        verb = "would fill" if args.dry_run else "filled"
        print(f"assistant messages: {total_assistant}")
        print(f"{verb}: {total_filled}")
        if counter is not None:
            print(f"count_tokens API calls: {counter.calls}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
