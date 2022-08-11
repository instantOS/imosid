# imosid

## Links

- Docs: instantos.io/docs/development/imosid


instant manager of sections in dotfiles

[![asciicast](https://asciinema.org/a/423508.svg)](https://asciinema.org/a/423508)

Planned features

- [X] autodetect comment syntax for files
- [ ] compare hashes of sections with upstream files
- [ ] syntax to combine multiple imosid comments into one line
- [ ] Other section sources
    - [ ] http
    - [ ] git
    - [ ] ipfs
    - [ ] ipns
- [X] colored/styled output

Refactor stuff
- traits for metafile/specialfile?

## Installation from source

```sh
git clone https://github.com/instantOS/imosid
cd imosid
cargo build --release
```
## Disclaimer

**imosid is my first time using rust, the program is in an extremely basic state.
Syntax and options are still subject to change**

