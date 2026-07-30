# Game-neutral shared assets

Each child folder is owned by the plug-in with the same package-folder name.
Other plug-ins reuse these assets through the host resource registry, leaving
one canonical copy on disk. Game-specific shared assets instead live under
`assets/<game-id>/shared/`.
