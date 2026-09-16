# slack

Slack from your terminal, as yourself.
One Rust binary, ~10ms startup, session stored in the macOS keychain.

## Install

```sh
cargo install --path .
slack completions zsh > ~/.zfunc/_slack   # optional
```

## Login

```sh
slack login acme          # opens your browser on acme.slack.com, log in there, done
slack whoami
```

The CLI drives a throwaway Brave/Chrome profile over the DevTools protocol.
Once you are logged in it reads the web client's session (`xoxc` token + `d` cookie), stores it in the login keychain under the `slack-cli` service, closes the browser and wipes the profile.
Nothing ever touches disk unencrypted.

The keychain item is written through `/usr/bin/security`, so rebuilding or upgrading the binary never triggers an "allow access" prompt.

Escape hatches: `--browser chrome`, `--profile <dir>` to reuse an existing browser profile, `--cookie -` to paste a `d` cookie from stdin instead of using a browser.
`slack logout` forgets everything.

## Everyday

```sh
slack send #general "Deploy **v2.3** is out, see [notes](https://…) cc @bob"
slack send @bob - < message.md                       # stdin, markdown
slack send #ops --blocks payload.json                # Block Kit, validated first
slack send <permalink> "replying in that thread"
slack send #ops "reply" --thread 1694700000.000100 --broadcast
slack send #ops "…" --dry-run                        # print the payload only

slack messages #general -n 50 --since 2d --threads
slack messages #general --follow                     # tail live
slack thread <permalink>
slack thread #general 1694700000.000100
slack search "deploy failed" --in ops --from vivien --after 2026-09-01
slack react <permalink> :tada:
slack channels [filter] [--all]
slack users [filter]
slack api conversations.info channel=C0123          # any Web API method
slack inbox                                          # unread DMs, mentions, thread replies
slack tui
```

`slack inbox` is a modal: `→` marks read, `←` snoozes (1h, 3h, tomorrow, monday), `r` replies in place, `enter` jumps into the conversation, `a` clears everything.
Piped or with `--list`/`--json` it prints the list instead. `i` opens it from the TUI too.

Every command takes `--json` for scripts and agents, and `-w <workspace>` to pick a workspace.

## Markdown

Messages are markdown by default and become Block Kit `rich_text`:
`**bold**`, `_italic_`, `~~strike~~`, `` `code` ``, fenced code, `# heading`, `-`/`1.` lists, `> quotes`, `---`, `[label](url)`, `@user`, `#channel`, `:emoji:`.
Use `--raw` to send Slack mrkdwn untouched.

## TUI

`slack tui` opens channels, messages and thread panes.
`j/k` move, `enter` opens, `r` replies, `t` replies in thread, `e` reacts, `o` opens in Slack, `y` copies the permalink, `s` searches, `/` filters channels, `?` shows every key.
`ctrl-k` (or `⌘k` on terminals that forward it: Ghostty, Kitty, WezTerm, iTerm2 with the kitty keyboard protocol enabled and ⌘K unbound) opens a fuzzy jump box over channels, people and the threads you follow (`vvt` finds `#vivien-vault`); a leading `>` sends the query to Slack search instead.
New messages, edits, deletions and reactions arrive live over Slack's RTM websocket; other channels light up with `●`.
If the workspace refuses RTM the open conversation is polled every 10 seconds instead (`↻` in the status bar).

## Settings

`~/.config/slack-cli/config.toml`:

```toml
[tui]
highlight = "#2a2a2a"   # selected row: a name, #rrggbb, or a 0-255 index

[links]
show_url = false        # true prints `label (url)`; false keeps the label, clickable on OSC 8 terminals
```

## Development

```sh
cargo test                       # unit + end-to-end against a mock Slack API
cargo test -- --ignored          # also the real keychain round trip
SLACK_CLI_DEBUG=1 slack login …  # trace the browser capture
```

Environment overrides: `SLACK_TOKEN` / `SLACK_COOKIE` bypass the keychain, `SLACK_CLI_API_URL`, `SLACK_CLI_CONFIG_DIR`, `SLACK_CLI_CACHE_DIR`, `NO_COLOR`, `COLUMNS`.
