# REA connection plan — approved and applied

Client: Codex desktop. REA npm package: 6.8.0; project skill: version 49,
installed with the official skill-only command. No analysis engine installed.
Initial state: no REA MCP tool was connected in this chat. A healthy CLI diagnostic would not
prove a live MCP connection.

The initial system Node 22.22.1 failed REA's minimum-version check. An existing
bundled Node 24.22.1 is compatible; no runtime installation is needed.

Read-only setup plan: `evidence/rea-compatible-setup-plan.json`.

The user explicitly approved this plan. Setup and the runtime correction below have been applied:

1. Run the installed REA setup under the existing Node 24.22.1, scoped to
   `--client codex --yes`. Setup proposes updating only
   `/home/papan/.codex/config.toml`, with recovery backup
   `/home/papan/.codex/config.toml.rea.backup` (or a new recovery path if the
   current file changes), and installing its bundled skill at
   `/home/papan/.agents/skills/reverse-engineer-anything`.
2. The generated runner uses an `env node` shebang that would select the older
   system Node. Preserve unrelated TOML and set the REA registration to the
   exact, already-present runtime and installed CLI:

   ```toml
   [mcp_servers.rea]
   command = "/home/papan/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin/node"
   args = ["/home/papan/.npm/_npx/af752b90495482d7/node_modules/rea-agents/scripts/rea.mjs", "mcp"]
   ```

   The npm cache path must remain present; a future cache purge will require
   an independently reviewed durable registration repair. Setup may use a
   different REA table name; inspect its actual output before editing.
3. Rerun scoped diagnostics. Reconnect/restart Codex, then inspect the actual
   tool list and perform a read-only REA call before claiming it operational.
4. Install no Ghidra, Hopper, Java, or other engines under this approval.
   Select engines only after an authorized Windows artifact establishes its
   format. Source repository analysis needs no native decompiler.

Approval requirement: prompt.md §2.6, and the installed REA skill's connection
guide: “Obtain approval for writes when that concrete scope is not already
authorized”. Applied evidence: `evidence/rea-applied.json` and `evidence/rea-doctor-after-setup.json`. Scoped doctor reports healthy; this does not establish an active chat MCP connection.

Codex configuration fields checked against [official MCP documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli). Unrelated parsed TOML entries were compared before/after the runtime correction and preserved.

Standalone MCP verification completed: REA 6.8.0, 229 tools, successful `binary_session`. Actual SDK PE/managed metadata/member calls are retained in `evidence/rea-hp-sdk-*.json` and `rea-xtu-service-members.json`.

Direct chat connection is now verified too. A read-only `binary_session` call
through the registered REA tool returned server 6.8.0, the Codex MCP client,
catalog count 229, no active target and no pending cleanup. Its complete response
is retained in `evidence/rea-chat-session.json`. No configuration repair or engine
installation was performed for this verification. Optional native decompilers
remain unavailable; this connection check does not remove that limitation.
