# imosid

instant manager of sections in dotfiles

[![asciicast](https://asciinema.org/a/423508.svg)](https://asciinema.org/a/423508)

Planned features

- autodetect comment syntax for files
- compare hashes of sections with upstream files
- syntax to combine multiple imosid comments into one line
- Other section sources
    - http
    - git
    - ipfs
    - ipns
- colored/styled output

## Installation

```sh
git clone https://github.com/instantOS/imosid
cd imosid
cargo build --release
```
## General information

Imosid works by dividing dotfiles into sections using special comments. These
sections can be updated, deleted or modified independently of one another. A section
can have two states, unmodified and modified. Modified sections are ignored by
imosid but do not interrupt or otherwise impact the processing of other
unmodified sections. 

For a section to be considered unmodified it needs intact beginning and ending
markers and a comment that contains the hash of the section content (excluding
the marker comments).  If any of the marker comments have incorrect syntax, are
missing or the hash of the section content does not match the hash in the
comment then the section is considered modified. 

Again, breaking the syntax of a section or modifying any part of it still leaves
other sections fully functional and processable by imosid. 

## Terminology

### Updating a section

replace the section content with something different and update the hash
comment. The new content is often from a newer (completely unmodified) version
of the same file

### Compiling a section

Insert/modify a hash comment to match the section content and create an
unmodified section. Used to create files that are used as update sources

## Section syntax

Imosid supports a wide range of different file formats and adapts to using the
correct comment syntax as a prefix. This means that all imosid comments start
with // if the file being processed is c source code and \# if the file is a
shell script. For the purposes of all examples, // will be used as the
placeholder for any kind of language/format specific commenting syntax.

All imosid comments begin with the comment sign and followed by "..."  and the
name of the section they are a part of.

Example
```txt
//...helloworld begin
```
This marks the beginning of the section hello world

## Commands

### Update

```sh
imosid update targetfile
```

- updates target file from sources
- requires upstream source comments

Example

hello.txt
```txt
#...sectionname2 begin
#...sectionname2 hash asduvhnw42377
#...sectionname2 source https://raw.github.stuff
Content
#...sectionname2 end
```

``` txt
imosid update hello.txt 
```

This command will fetch https://raw.github.stuff and if that file contains
sectionname2 will update sectionname2 in hello.txt to match that version of the
section
#...sectionname2 end

### Apply

```sh
imosid apply sourcefile
```

- requires target comment
- applies all sections present in all files

### Compile

```sh
imosid compile
```

- regenerates section hashes
- creates main section if not present already
  - respect special 1st line like hashbangs

## Problems

- section updates can clash with existing configs
  - duplicate key detection?
  - keep sections to a single function?


## Syntax

```txt
#... all target /path/to/targetfile
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

