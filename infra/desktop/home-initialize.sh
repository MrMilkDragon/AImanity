#!/bin/sh
set -eu
config=${XDG_CONFIG_HOME:-"$HOME/.config"}
pcmanfm="$config/pcmanfm"
desktop_style="$pcmanfm/default/desktop-items-0.conf"
mkdir -p "$HOME/Files" "$HOME/Desktop" "$HOME/Templates" "$config/libfm" "$pcmanfm/default" "$config/tint2"
if [ ! -e "$config/user-dirs.dirs" ]; then
  printf '%s\n' 'XDG_DESKTOP_DIR="$HOME/Desktop"' 'XDG_DOCUMENTS_DIR="$HOME/Files"' 'XDG_TEMPLATES_DIR="$HOME/Templates"' > "$config/user-dirs.dirs"
fi
if [ ! -e "$config/gtk-3.0/bookmarks" ]; then
  mkdir -p "$config/gtk-3.0"
  printf 'file:///home/ai/Files\n' > "$config/gtk-3.0/bookmarks"
fi

if [ -f "$pcmanfm/.ai-phone-style-v1" ]; then
  if cmp -s "$desktop_style" - <<'STYLE'
[*]
wallpaper_mode=color
desktop_bg=#e7ebef
desktop_fg=#26313c
desktop_shadow=#ffffff
show_wm_menu=1
show_documents=0
show_trash=0
show_mounts=0
STYLE
  then
    cp /etc/xdg/pcmanfm/default/desktop-items-0.conf "$desktop_style"
  fi
  while IFS='|' read -r filename label command icon; do
    shortcut="$HOME/Desktop/$filename.desktop"
    if printf '[Desktop Entry]\nType=Application\nName=%s\nExec=%s\nIcon=%s\nTerminal=false\n' "$label" "$command" "$icon" | cmp -s "$shortcut" -; then
      rm "$shortcut"
    fi
  done <<'SHORTCUTS'
Files|工作文件|pcmanfm /home/ai/Files|folder
Browser|浏览器|/usr/local/bin/ai-phone-browser|chromium
Writer|文档|libreoffice --writer|libreoffice-writer
Calc|表格|libreoffice --calc|libreoffice-calc
Impress|演示|libreoffice --impress|libreoffice-impress
Draw|绘图|libreoffice --draw|libreoffice-draw
SHORTCUTS
  rm "$pcmanfm/.ai-phone-style-v1"
fi

if [ ! -f "$desktop_style" ]; then
  cp /etc/xdg/pcmanfm/default/desktop-items-0.conf "$desktop_style"
fi
if [ ! -f "$config/tint2/tint2rc" ] || cmp -s "$config/tint2/tint2rc" /etc/xdg/tint2/tint2rc; then
  cp /usr/local/share/aimanity/tint2rc "$config/tint2/tint2rc"
fi
if [ ! -f "$config/libfm/libfm.conf" ]; then
  printf '[config]\nquick_exec=1\n' > "$config/libfm/libfm.conf"
fi
if [ ! -f "$config/.ai-phone-office-v1" ]; then
  if [ -d "$config/libreoffice" ]; then mv "$config/libreoffice" "$config/libreoffice-before-desktop-fix"; fi
  touch "$config/.ai-phone-office-v1"
fi
rm -f "$config/libreoffice/4/.lock"
