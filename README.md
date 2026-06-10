# theshfmt

A shell script **formatter** and **linter**, written in portable Bash.

`theshfmt` reads shell scripts as text and proposes small, conservative
fixes. Its guiding principle is that **readability and maintainability are
second only to correctness**: it will never apply a transformation that could
change a script's behavior. When a construct is ambiguous or risky, it is left
untouched.

## Compatibility

- **The tool itself** is written to run on **Bash 3.2** (the version that
  ships on, e.g., macOS). It avoids Bash 4+ features such as associative
  arrays, `mapfile`/`readarray`, `${var^^}`/`${var,,}`, and namerefs.
- **The scripts it processes** can target any version of `sh`/`bash`/`zsh` —
  they are treated purely as text, so no particular runtime is assumed.

## Usage

```
theshfmt [-a|--apply-changes] [-p|--print-diff] [-h|--help] <inputs...>
```

| Mode | Flag | Behavior |
| --- | --- | --- |
| Apply | `-a`, `--apply-changes` | Rewrite the input files in place with all fixes. |
| Diff | `-p`, `--print-diff` | Print git-style diffs only; files are not modified. |
| Interactive | *(none)* | Prompt `y/N` for each change (default **N**). |

`-a` and `-p` are mutually exclusive.

In `-p` mode the exit status is non-zero when any change is suggested, so it
can be used as a CI check.

### Examples

```sh
# Show what would change, without touching anything:
theshfmt -p script.sh

# Apply all fixes:
theshfmt -a script.sh lib/*.sh

# Review each change one at a time:
theshfmt script.sh
```

## Rules

### Formatting

- **`if`/`elif` then-joining.** A condition followed by a lone `then` on the
  next line is joined into the canonical `if <cond>; then` form.

  ```sh
  if [[ -n "$x" ]]      ->   if [[ -n "$x" ]]; then
  then                           echo hi
      echo hi                fi
  fi
  ```

  The join is skipped when the previous line ends with an operator (`&&`, `||`,
  `|`, `&`, `{`, `(`) or a line-continuation, or carries a trailing comment —
  any case where blindly appending `; then` would be unsafe.

- **No trailing semicolon.** A redundant `;` at the end of a line is removed.
  A `;` that separates two statements *within* a line is preserved.

  ```sh
  echo hello;     ->   echo hello
  foo; bar;       ->   foo; bar
  ```

  Case terminators (`;;`), semicolons inside quotes, escaped semicolons, and
  lines ending in a comment are left alone.

### Linting

- **`[ ... ]` → `[[ ... ]]`.** A POSIX test is upgraded to the Bash conditional
  form when it is provably safe. The conversion preserves all interior spacing
  byte-for-byte and is **skipped** when the meaning could differ, including:
  - binary `-a` / `-o` (and/or in `[`, but file tests in `[[`);
  - bare or escaped comparison operators (`<`, `>`, `\<`, `\>`, `\(`, `\)`);
  - an unquoted right-hand side of `=`/`==`/`!=` containing glob
    metacharacters (`*`, `?`, `[`), which would become a pattern under `[[`;
  - an empty test (`[ ]`), since `[[ ]]` is a syntax error.

  A `[` that is an argument to another command (e.g. `echo [ x ]`) is not a
  test and is never touched.

## Correctness safeguards

`theshfmt` lexes each line while tracking single/double quotes, comments,
arithmetic context (`(( ... ))`, `$(( ... ))`), here-documents (`<<`, `<<-`)
and here-strings (`<<<`). Content inside heredoc bodies and multi-line quoted
strings is never modified, and metacharacters inside quotes/arithmetic are not
mistaken for command separators.

## Tests

```sh
bash tests/run_tests.sh
```

Each case under `tests/cases/` is an `<name>.in` / `<name>.expected` pair: the
runner formats the input with `-a` and compares against the expected output.
