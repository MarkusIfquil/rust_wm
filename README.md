<div align="center"><h1>hematite: a simple, fast, opinionated tiling window manager.</h1></div>

# why hematite?
hematite is designed to be as simple as possible while still having the functions of a modern tiling manager.

it is **simple**, only containing ~1500 lines of code, and is only concerned about tiling windows and showing a bar.

it is **fast** and **efficient**, refreshing only when necessary and contains as few moving parts as possible.

it is **opinionated**, contains no support for external scripting, with a minimal config file for appearance. it also only contains one tiling layout, which is master-stack.
# installation
## 1. Build from source (recommended installation)
### clone the repository
```sh
git clone git@github.com:MarkusIfquil/hematite.git
```

### build from source
```sh
sudo cargo install --path . --root /usr
```

### add to .xinitrc (or any script that runs on startup)
```sh
exec hematite &
```

# next steps
## status bar
included in the repository is a bar script. adding it to your startup script shows various status information on the bar.

### add to .xinitrc
```sh
bash bar.sh &
```
## notifications
`dunst` is recommended for showing notifications as it is also simple and lightweight.
## install dunst
### Arch linux:
```sh
sudo pacman -S dunst
```
### add to .xinitrc
```sh
dunst &
```
## background image
`feh` is recommended for setting the wallpaper.
### install feh
### Arch linux:
```sh
sudo pacman -S feh
```
### add to .xinitrc
```sh
~/.fehbg &
```
# configuration
configuration is set using the `config.toml` file located in your `.config/hematite` folder. A default one is provided when hematite is run for the first time.
## font
for now, fonts use the base x fonts found in your font directories. For TTF fonts this is usually `/usr/share/fonts/TTF`. 

if a font is not recognized make sure that you're using the correct name format (e.g. `-misc-jetbrainsmononl nfp medium-medium-r-normal--20-0-0-0-p-0-iso8859-16`), and that X sees your font directory by containing a `fonts.dir` file.
## hotkeys
not all keys are supported by default. If you want to use a non-character key then you will have to add it manually in the code.

# default hotkeys
| Keybinding           | Description                                                            |
| -------------------- | ---------------------------------------------------------------------- |
| Mod + (1-9)          | Switch to a desktop/tag                                                |
| Shift + Mod + (1-9)  | Move window to a desktop/tag                                           |
| Mod + q              | Close window                                                           |
| Shift + Mod + q      | Exit hematite                                                          |
| Mod + h              | Decrease master area ratio                                             |
| Mod + j              | Increase stack area ratio                                              |
| Mod + k              | Focus previous window                                                  |
| Mod + l              | Focus next window                                                      |
| Mod + Left           | Switch to previous desktop/tag                                         |
| Mod + Right          | Switch to next desktop/tag                                             |
| Mod + Enter          | Swap focused window with master window                                 |
| Mod + c              | Application launcher (default: rofi drun)                              |
| Control + Mod + Enter| Open terminal (default: alacritty)                                     |
| Control + Mod + l    | Open browser (default: librewolf)                                      |
| Mod + u              | Take screenshot (default: maim)                                        |
