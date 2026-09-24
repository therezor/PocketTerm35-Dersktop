# Changelog

## 0.3.2 (2026-09-24)

### Fixed

- An app's context menu no longer covers the launcher. Opening the menu over
  a right-click menu left the popup on top, holding the keyboard. The popup
  now closes when the menu opens.
- The DarkWire Default agent stops reaching for sudo. It tried
  `sudo journalctl`, the sudo rule refused it, and it gave up. The prompt now
  says logs need no sudo and shows the command for recent errors. Run
  `pt35-ai agents --reset` to pick up the new prompt.

## 0.3.1 (2026-09-24)

### Added

- A local AI agent: `pt35-ai` installs llama.cpp, llama-swap and pinned
  models, with darkwire as the chat client and five agents. The DarkWire
  tile opens it.

### Changed

- The menu is the desktop when nothing is open on the workspace in front,
  so an app that exits leaves you on the launcher.

### Fixed

- Lock was a white screen: swaylock with no config paints white. It now
  uses the theme's colours, unless you have a swaylock config of your own.
- Screen off had no way back but ssh. Any key or touch now turns the
  panel on again. This needs `swayidle`, now a dependency.
- The power key no longer opens the power menu under the lock, where it
  waited after the unlock with Screen off selected.

## 0.3.0 (2026-09-23)

First release.
