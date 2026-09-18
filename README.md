# slack-tui

Slack in your terminal, as yourself: a full TUI plus scriptable commands.
One Rust binary called `slack`, ~10 ms startup.

> [!IMPORTANT]
> **Unofficial.** It logs in with your browser session, not a Slack app, so it needs no admin approval.
> Check that your workspace rules allow this before you use it.
>
> **macOS only** for now (keychain, `open`, `pbcopy`).
>
> **Vibe Coded** for personal usage but use at your own risks.

## Install

```sh
cargo install --git https://github.com/vmeyet/slack-tui
```

That is all: the `slack` command is now on your path.

| You need | Get it |
|---|---|
| macOS | |
| Rust 1.88+ (`cargo`) | `curl -sSf https://sh.rustup.rs \| sh` |
| Brave, Chrome, Chromium or Edge | Only used once, to log in |

Optional shell completions: `slack completions zsh > ~/.zfunc/_slack` (also `bash`, `fish`).
Update with `slack update` (a no-op when you already run the latest commit, `-f` to rebuild anyway); remove with `cargo uninstall slack`.
`slack --version` prints the version and the commit it was built from.

## Log in

```sh
slack login acme    # opens acme.slack.com in your browser; log in there, done
slack whoami
slack logout        # forgets the keychain entry, config and cache
```

| Option | Use |
|---|---|
| `--browser chrome` | Pick the browser: `brave`, `chrome`, `chromium`, `edge`, or a path. |
| `--profile <dir>` | Reuse a browser profile that is already logged in. |
| `--cookie -` | Skip the browser and paste a `d` cookie on stdin. |

## TUI

```sh
slack tui
```

Three panes: channels, messages, thread.
Messages, edits, deletions and reactions arrive live; other channels light up with `●`.

| Key | Action |
|---|---|
| `j` `k` / arrows | Move |
| `enter` / `l` | Open the channel or the thread |
| `h` / `esc` | Go back |
| `r` / `t` | Reply / reply in thread |
| `e` | React |
| `o` / `u` | Open the message in Slack / open its first link |
| `y` | Copy the permalink |
| `/` | Filter channels |
| `s` | Search |
| `ctrl-k` | Fuzzy jump to a channel, a person or a followed thread; start with `>` to search Slack |
| `i` | Inbox |
| `f` | Firehose: every conversation as one live wall |
| `z` | Reading mode: one centered column, nothing else |
| `:` | Command line |
| `?` | Every key |

`⌘k` also works on terminals that forward it (Ghostty, Kitty, WezTerm, iTerm2 with the kitty keyboard protocol).

**Command line.**
`:join #ops`, `:leave`, `:go @bob`, `:msg @bob on my way`, `:react rocket`, `:search deploy failed`, `:export md`, `:read`, `:snooze 1h`, `:set theme=nord`, `:help`, `:quit`.
Tab completes verbs, channels, people and emoji; `↑` recalls history.

**Inbox.**
Unread DMs, mentions and thread replies in one list.
`→` marks read, `←` snoozes (1h, 3h, tomorrow, monday), `r` replies in place, `enter` opens the conversation, `a` clears everything.

**No live feed?**
If the workspace refuses the RTM websocket, the open conversation is polled every 10 seconds (`↻` in the status bar).

## Commands

Every command takes `--json` (for scripts and agents) and `-w <workspace>`.

```sh
slack send '#general' "Deploy **v2.3** is out, see [notes](https://…) cc @bob"
slack send @bob - < message.md                  # markdown from stdin
slack send '#ops' --blocks payload.json         # Block Kit
slack send <permalink> "replying in that thread"
slack send '#ops' "…" --dry-run                 # print the payload, send nothing

slack messages '#general' -n 50 --since 2d --threads
slack messages '#general' --follow              # tail live
slack thread <permalink>
slack search "deploy failed" --in ops --from bob --after 2026-09-01
slack react <permalink> :tada:
slack channels [filter] [--all]
slack users [filter]
slack inbox                                     # a list when piped or with --list
slack firehose -H 'error|failed' -H prod        # live ticker, -H highlights a regex
slack api conversations.info channel=C0123      # any Web API method
```

Messages are markdown by default and are sent as Block Kit `rich_text`:
`**bold**`, `_italic_`, `~~strike~~`, `` `code` ``, fenced code, `# heading`, lists, `> quotes`, `---`, `[label](url)`, `@user`, `#channel`, `:emoji:`.
Use `--raw` to send Slack mrkdwn untouched.

## Settings

`~/.config/slack-cli/config.toml`, every key optional:

```toml
[tui]
theme = "catppuccin"    # dracula, catppuccin, catppuccin-latte, rosepine, rosepine-dawn, nord, tokyonight, monokai
highlight = "#2a2a2a"   # fill under the selected row; by default only the ▎ bar marks it
images = true           # inline thumbnails on Kitty, Ghostty, WezTerm and iTerm2

[links]
show_url = false        # true prints `label (url)`; false keeps a clickable label

[firehose]
highlight = ["prod", "error|failed", "@bob"]   # case-insensitive regexes
```

`:set theme=…`, `:set highlight=…` and `:set images=on|off` write this file from the TUI.

## How the login works

1. `slack login` starts a throwaway browser profile and drives it over the DevTools protocol.
2. Once you are logged in, it reads the web client's session: the `xoxc` token and the `d` cookie.
3. It stores both in the macOS login keychain (service `slack-cli`), closes the browser and deletes the profile.

The session never touches disk unencrypted.
It acts as you, with everything you can do in Slack: treat it like a password.
The keychain is accessed through `/usr/bin/security`, so rebuilding the binary never triggers an "allow access" prompt.

Unread badges, the inbox and sidebar sections use undocumented web-client endpoints (`client.counts`, `subscriptions.thread.*`, `users.channelSections.list`).
Slack can change them without notice.

## Development

```sh
cargo test                       # unit + end-to-end against a mock Slack API
cargo test -- --ignored          # also the real keychain round trip
SLACK_CLI_DEBUG=1 slack login …  # trace the browser capture
```

| Variable | Effect |
|---|---|
| `SLACK_TOKEN`, `SLACK_COOKIE` | Bypass the keychain |
| `SLACK_WORKSPACE` | Default workspace |
| `SLACK_CLI_API_URL` | Point at another API host (tests use a mock) |
| `SLACK_CLI_CONFIG_DIR`, `SLACK_CLI_CACHE_DIR` | Move the config and the cache |
| `NO_COLOR`, `COLUMNS` | Plain output, fixed width |

## License

[MIT](LICENSE)
