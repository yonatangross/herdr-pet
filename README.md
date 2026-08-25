# herdr-pet

A Codex-style pet that lives on your [herdr](https://herdr.dev) panes and acts
out what your agent is doing. One small Rust binary. Uses the same pet format
as Codex, so the 750+ community pets work as-is.

```
agent working   → typing
needs input     → waiting (waves first)
done            → jumps, then reviews
idle            → naps
crashed         → sad
dragged         → walks along, facing the way you pull
```

## Requirements

herdr ≥ 0.7.4 and a terminal that can draw images: Ghostty, kitty, or WezTerm.

## Install

```bash
herdr plugin install <owner>/herdr-pet
```

That downloads a prebuilt binary (checksum-verified) and puts `herdr-pet` on
your PATH. For a dev checkout:

```bash
cargo build --release
herdr plugin link /path/to/herdr-pet
ln -s "$PWD/target/release/herdr-pet" ~/.local/bin/herdr-pet
```

## Setup

Add to your herdr config (`~/.config/herdr/config.toml`):

```toml
[experimental]
kitty_graphics = true

[[keys.command]]
key = "prefix+shift+p"
type = "plugin_action"
command = "pet.toggle"        # wake / tuck away
description = "pet: toggle"

[[keys.command]]
key = "prefix+shift+o"
type = "plugin_action"
command = "pet.settings"      # popover: pick a pet, size, speed, mode…
description = "pet: settings"
```

Then `herdr server reload-config` and detach/reattach each client (`prefix+q`,
then `herdr`) — the graphics flag only takes effect on a fresh client. Finally,
install a pet (below).

## Use

Everything in the popover applies immediately. The same knobs exist on the CLI
for scripting: `herdr-pet use|move|status|list|bigger|smaller|faster|slower|start|stop|restart`.

The pet survives herdr restarts on its own. Tucking it away keeps the daemon
running so waking is instant; `herdr-pet stop` shuts it down entirely.

**Moving it:** on macOS, hold **⌃⌥** and drag — the pet runs along with you and
stays where you drop it. First time, macOS will ask for Accessibility
permission for your terminal app. Dragging only works at the machine herdr
runs on; over SSH (or on Linux) use `herdr-pet move <col> <row>` /
`herdr-pet move default`.

## Config

`$(herdr plugin config-dir pet)/pet.toml` — the popover edits it live; hand
edits apply on `herdr-pet restart`.

```toml
enabled = true
pet = "desk-otter"          # a pet name, or a path to one
mode = "all"                # all: follows the focused pane · agents: one on every agent pane
size = 6                    # height in terminal rows
transitions = true          # wave/jump on status changes (off = calmer, steadier size)
speed = 1.0                 # animation speed, 0.25–4
quantize = true             # smaller frames, looks the same for sprite art
drag = "control+option"     # drag keys, or false
warm_panes = 4              # recently used panes that keep their pet ready
# position = [24, 10]       # written when you drag; delete to go back to the corner
```

## Pets

None are bundled. A pet is a folder with `pet.json` + `spritesheet.webp` in
Codex's format (made with OpenAI's `hatch-pet` skill). Put it in
`~/.codex/pets/` — the same place Codex keeps its pets — and switch with
`herdr-pet use <name>` or the popover. 

Browse more: [petdex.dev](https://petdex.dev) (`npx petdex install <slug>`),
[awesome-codex-pet](https://github.com/legeling/awesome-codex-pet),
[codex-pokepets](https://github.com/dnnyngyen/codex-pokepets). Mind each pet's
own licence.

## Good to know

- The pet draws over the pane's text in its corner. Plain clicks go straight
  through to the pane — only the ⌃⌥-drag is intercepted.
- Only the pet on your focused pane animates; others hold still. Cost is
  small (a few percent of one core) and hidden panes cost nothing.
- Built on herdr's experimental graphics API, which may change.
