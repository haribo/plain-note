# Deterministic GTK render environment for headless screenshot tests.
# Arch base to match the app's GTK 4 runtime; Xvfb provides a virtual display.
FROM archlinux:latest
RUN pacman -Syu --noconfirm \
      gtk4 libadwaita xorg-server-xvfb imagemagick xdotool \
      ttf-dejavu ttf-liberation \
  && pacman -Scc --noconfirm
