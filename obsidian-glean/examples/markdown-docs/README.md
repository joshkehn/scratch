# Widget

[![CI](https://img.shields.io/badge/ci-passing-green)](https://example.com/ci)

Widget is a small library for making widgets. See the [documentation][docs] and
the [contributing guide](CONTRIBUTING.md).

Overview
========

Widget does three things well. For background read <https://example.com/about>.

## Installation

```bash
cargo add widget
```

## Usage

```rust
use widget::Widget;

fn main() {
    let w = Widget::new("hi");
    println!("{w}");
}
```

## Features

| Feature   | Status | Notes                     |
|-----------|:------:|---------------------------|
| Parsing   |   ✅   | stable                    |
| Rendering |   ✅   | see [the guide](docs/guide.md) |
| Export    |   🚧   | tracked in [#42][issue42] |

## Roadmap

- [x] Parse input
- [ ] Render output, see [the guide](docs/guide.md)
- [ ] Export to <https://example.com/format>

## Notes

Widget uses a fast parser.[^parser] Inline HTML is allowed:
<img src="docs/logo.png" alt="logo" width="120">.

[^parser]: The parser is hand-written for speed.

[docs]: docs/guide.md
[issue42]: https://example.com/issues/42
