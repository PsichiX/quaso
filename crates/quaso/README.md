# Quaso

Toolset for making Micro Games quickly.

Quaso bundles rendering, GUI, input, audio, assets, animation, scripting and
multiplayer behind one game loop, so a small game needs little wiring.

```toml
[dependencies]
quaso = "0.60"
```

## Features

- `editor` (default) - the in game editor.
- `agent` - opens a loopback control port so an outside process can pause the
  game, inject input, step the fixed update and capture frames. It is not on by
  default, and it refuses to start in a release build.

## Project templates and tools

Ready made game templates, the `quaso-mcp` adapter and the examples live in the
repository: <https://github.com/PsichiX/quaso>

Run an example from the repository root, because the examples read `./resources`:

```bash
cargo run -p quaso --example hello_world
```
