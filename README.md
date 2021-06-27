# imosid

instant manager of sections in dotfiles

Planned features

- manage parts of files through comments
- autodetect comment syntax for files
- compare hashes of sections with upstream files
- each section contains its own hash, dont update if hash doesnt match

usage: imosid sourcefile target

sourcefile has same sections, if hashes differ then section in target gets
replaced with section from sourcefile

## Commands

imosid update target

- updates target file from sources
- requres upstream source comments

imosid apply source

- requires target comment
- applies all sections present in all files

imosid compile

- regenerates section hashes
- creates main section if not present already
  - respect special 1st line like hashbangs

## Problems

- section updates can clash with existing configs
  - duplicate key detection?
  - keep sections to a single function?

## Installation

```sh
git clone https://github.com/instantOS/imosid
cd imosid
cargo build --release
```

## Syntax

```txt
#...sectionname begin
#...sectionname hash 123aojd981uenc821y3
#...sectionname source /usr/share/instantdotfiles/stuff
Content
#...sectionname end

#...sectionname2 begin
#...sectionname2 hash asduvhnw42377
#...sectionname2 source https://raw.github.stuff
Content
#...sectionname2 end

```

## Disclaimer

This is my first time using rust, the program is in an extremely basic state.
Syntax and options are still subject to change
