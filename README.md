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
runs on; over SSH (or on Linux) use the picker or `herdr-pet move`.

No drag available? The settings popover has a **Position…** row: press enter
and tap (or drag) a spot on a little map of your pane — the pet moves there
live. Arrows nudge it, `d` puts it back in the corner.

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

None are bundled — grab one from a gallery (each lands in `~/.codex/pets`,
where the plugin looks; it shows up in the popover right away):

- [petdex.dev](https://petdex.dev) — `npx petdex install <slug>`
- [codexpet.top](https://codexpet.top) — copy-paste install command on each pet's page
- [codex-pokepets](https://github.com/dnnyngyen/codex-pokepets) — every Pokémon, one curl
- or make your own with `hatch-pet` skill in Codex

The 8 official Codex pets (`codex`, `dewey`, `fireball`, `rocky`, `seedy`,
`stacky`, `bsod`, `null-signal`) aren't licensed for bundling, but you can
fetch one for yourself from Codex's CDN — the same download its CLI does:

```bash
id=dewey; d=~/.codex/pets/$id; mkdir -p $d
curl -o $d/spritesheet.webp "https://persistent.oaistatic.com/codex/pets/v1/$id-spritesheet-v4.webp"
printf '{"id":"%s"}' $id > $d/pet.json
```

Then switch with the popover or `herdr-pet use <name>`.

A pet is just a folder with `pet.json` + `spritesheet.webp` in Codex's format;
the plugin also reads its own `pets/` folder and plain paths
(`herdr-pet use ./my-pet`).

## Good to know

- The pet draws over the pane's text in its corner. Plain clicks go straight
  through to the pane — only the ⌃⌥-drag is intercepted.
- Only the pet on your focused pane animates; others hold still. Cost is
  small (a few percent of one core) and hidden panes cost nothing.
- Built on herdr's experimental graphics API, which may change.
