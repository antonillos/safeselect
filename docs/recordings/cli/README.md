# SafeSelect CLI captures

These PNGs are static terminal captures for the shared CLI gallery
(docs/cli-gallery.json). They were prepared from real SafeSelect CLI invocations
against disposable PostgreSQL/MongoDB fixtures and synthetic import inputs.
Configuration, credentials and personal paths are redacted. Connectivity and
query captures show real lifecycle results and bounded synthetic rows; guardrails
remain fail-closed where applicable.

The renderer follows the VHS recordings: JetBrains Mono, a colorful window bar
and semantic terminal colors for prompts, success, warnings and errors.

The MCP initialize response is line-wrapped in its thumbnail for readability; the payload is unchanged. The captures are intentionally evidence-sized thumbnails rather than videos.
Run python3 -B tools/validate_cli_gallery.py from the repository root to verify
that every catalog entry has one capture, that the files stay within the asset
budget and that no private markers are present in the catalog.
