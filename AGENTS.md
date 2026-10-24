# Bork

For implementation, read `.agents/SPEC.md`; its assets are the visual reference. `README.md` owns build/run commands, host requirements, and operational caveats.

## Working style

- Keep changes focused and hand them back promptly for user testing. Run full verification matrices only when the user explicitly requests them.
- Keep builds within 50% of available CPU capacity.
- Keep temporary logs, screenshots, and build outputs in ignored directories; do not create progress journals or verification handoffs by default.

## Safety

Do not change persistent system configuration or enable disruptive effects on the active desktop without approval. Use isolated sessions for display tests.
